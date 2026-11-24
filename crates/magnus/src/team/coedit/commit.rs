//! Changing a co-edited file. Every change lands under the version check:
//! it is made on a base version, and lands as the version after the head,
//! merged by the file's format with whatever landed since its base. One
//! that conflicts is rejected and kept as its author's (`Step::Proposed`
//! with its conflicts), so nothing is lost, and its author decides from both
//! versions. Changes land one at a time (`WRITES`), so two never land on
//! the same head.

use super::merge::{self, Choices, Conflict, Format, Merged};
use super::{Author, Open, Proposal, Step, Version, CHECKOUT_LEASE_SECS, FORMER_MEMBER};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::team::agent::volume_dir;
use crate::team::{Change, File};
use anyhow::{Context, Result};
use base64::Engine;
use chrono::Utc;
use sha2::{Digest, Sha256};

/// Held from reading a file's head to writing the version on it, and from
/// finding a new name free to sharing a file under it.
static WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Why a change of someone who no longer sees the file does not land.
const NOT_OPEN: &str = "that file is no longer open to you, so your change to it does not land";

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// What a change came to.
#[derive(Debug, PartialEq)]
pub enum Landed {
    /// It is this version now.
    Version(u32),
    /// An agent's edit, kept as its person's until its lane is released.
    Proposed(String),
    /// Its base was stale and parts of it conflict: kept as its author's
    /// (`id`), with both versions of each part.
    Rejected { id: String, head: u32, conflicts: Vec<Conflict> },
    /// It cannot land as it is, and why.
    Refused(String),
}

/// Co-editing's writes, held while a new name is found free and a file
/// shared under it, so two shares of one name never both land.
pub(in crate::team) async fn share_lock() -> tokio::sync::MutexGuard<'static, ()> {
    WRITES.lock().await
}

fn coedit(workspace: &str, step: Step) -> Ev {
    Ev::Team(Change::Coedit { workspace: workspace.into(), coedit: step })
}

/// A change to land: its bytes, the version it was made on and its author.
pub(in crate::team) struct Draft<'a> {
    pub(in crate::team) ws: &'a str,
    pub(in crate::team) name: &'a str,
    pub(in crate::team) base: u32,
    pub(in crate::team) mine: &'a [u8],
    pub(in crate::team) author: &'a Author,
}

impl Kernel {
    /// The bytes of version `n` of `name` in `ws`.
    pub(in crate::team) async fn coedit_bytes(&self, ws: &str, name: &str, n: u32) -> Result<Option<Vec<u8>>> {
        let seq = self.read(|s| s.team.workspace(ws).and_then(|w| w.coedit.files.get(name)).and_then(|f| f.versions.get(&n)).map(|m| m.seq));
        let Some(seq) = seq else { return Ok(None) };
        self.step_bytes(seq).await
    }

    /// The bytes the step at `seq` holds: a version's, or a proposal's.
    pub(super) async fn step_bytes(&self, seq: i64) -> Result<Option<Vec<u8>>> {
        let bodies = self.log.contents(&[seq]).await?;
        let Some(data) = bodies.get(&seq).and_then(|b| b["coedit"]["data_b64"].as_str()) else { return Ok(None) };
        Ok(Some(B64.decode(data).context("a version's bytes")?))
    }

    /// The events that share `bytes` as a co-edited file of `ws`: the file,
    /// the project's from now on, and its first version, `person`'s.
    pub(in crate::team) fn coedit_share(&self, ws: &str, person: &str, name: &str, bytes: &[u8]) -> std::result::Result<Vec<Ev>, String> {
        if let Some(why) = Format::of(name).refusal(name, bytes) {
            return Err(why);
        }
        let file = File { name: name.into(), size: bytes.len() as u64, by: String::new(), coedit: true };
        let first = Version {
            name: name.into(),
            number: 1,
            base: 0,
            author: Author { person: person.into(), run: None },
            at: Utc::now(),
            data_b64: B64.encode(bytes),
            diff: vec![],
            sha256: hex::encode(Sha256::digest(bytes)),
            size: bytes.len() as u64,
        };
        Ok(vec![Ev::Team(Change::File { workspace: ws.into(), file }), coedit(ws, Step::Version(first))])
    }

    /// A person's change to a co-edited file, made on version `base`.
    pub(in crate::team) async fn coedit_write(&self, actor: &str, c: Draft<'_>) -> Result<Landed> {
        let _held = WRITES.lock().await;
        self.land(actor, &c, &Choices::new(), None).await
    }

    /// Land `c` as the next version, merged with what landed since its base,
    /// `choices` deciding what conflicts. It settles `proposal` when it lands.
    /// `WRITES` is held.
    async fn land(&self, actor: &str, c: &Draft<'_>, choices: &Choices, proposal: Option<&Open>) -> Result<Landed> {
        let now = Utc::now();
        // Its author must still see the file: one who left the project, or
        // the workspace, writes nothing to it and reads nothing of it.
        if !self.sees(c.ws, c.name, &c.author.person) {
            return Ok(Landed::Refused(NOT_OPEN.into()));
        }
        let file = self.read(|s| s.team.workspace(c.ws).and_then(|w| w.coedit.files.get(c.name)).map(|f| (f.format, f.head().map_or(0, |h| h.number), f.held(now).cloned())));
        let Some((format, head, lease)) = file else { return Ok(Landed::Refused(format!("{} is not a co-edited file here", c.name))) };
        if c.base == 0 || c.base > head {
            return Ok(Landed::Refused(format!("{} has no version {}; it is at version {head}", c.name, c.base)));
        }
        if let Some(why) = format.refusal(c.name, c.mine) {
            return Ok(Landed::Refused(why));
        }
        if let Some(l) = lease.filter(|l| l.person != c.author.person) {
            let who = crate::team::chat_routes::name_of(self, &l.person);
            return Ok(Landed::Refused(format!(
                "{who} has {} checked out until {}; others read the last version until it is checked in",
                c.name,
                l.until.format("%Y-%m-%d %H:%M UTC")
            )));
        }
        let base = self.coedit_bytes(c.ws, c.name, c.base).await?.context("the base version's content is gone")?;
        let merged = if c.base == head {
            Merged::Clean(c.mine.to_vec())
        } else {
            let theirs = self.coedit_bytes(c.ws, c.name, head).await?.context("the head version's content is gone")?;
            merge::merge(format, &base, c.mine, &theirs, choices)
        };
        let diff = merge::diff(format, &base, c.mine);
        let bytes = match merged {
            Merged::Clean(b) => b,
            Merged::Conflicts(conflicts) => {
                let id = proposal.map_or_else(|| new_id("edit"), |p| p.id.clone());
                let kept = Proposal {
                    id: id.clone(),
                    name: c.name.into(),
                    base: c.base,
                    author: c.author.clone(),
                    at: now,
                    data_b64: B64.encode(c.mine),
                    diff,
                    mission: proposal.and_then(|p| p.mission.clone()),
                    lane: proposal.and_then(|p| p.lane.clone()),
                    conflicts: conflicts.clone(),
                };
                self.emit(actor, coedit(c.ws, Step::Proposed(kept))).await?;
                return Ok(Landed::Rejected { id, head, conflicts });
            }
        };
        let version = Version {
            name: c.name.into(),
            number: head + 1,
            base: c.base,
            author: c.author.clone(),
            at: now,
            data_b64: B64.encode(&bytes),
            diff,
            sha256: hex::encode(Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        };
        let mut evs = vec![(actor, coedit(c.ws, Step::Version(version)))];
        if let Some(p) = proposal {
            evs.push((actor, coedit(c.ws, Step::Settled { id: p.id.clone(), version: Some(head + 1) })));
        }
        self.emit_all(evs).await?;
        self.write_working_copy(c.ws, c.name, &bytes).await;
        Ok(Landed::Version(head + 1))
    }

    /// The volume keeps the head, so what reads the volume reads the last version.
    async fn write_working_copy(&self, ws: &str, name: &str, bytes: &[u8]) {
        let dir = volume_dir(self, ws);
        let wrote = async {
            tokio::fs::create_dir_all(&dir).await?;
            tokio::fs::write(dir.join(name), bytes).await
        };
        if let Err(e) = wrote.await {
            tracing::error!("workspace {ws}: the volume's copy of {name}: {e}");
        }
    }

    /// An agent's edit, as `team_propose_edit` makes it: checked against the
    /// head now, so a conflict goes back to its run at once, else kept with
    /// its lane's result. An earlier edit of the same file in the lane gives
    /// way to it.
    pub(in crate::team) async fn coedit_propose(&self, actor: &str, c: Draft<'_>, mission: &str, lane: &str) -> Result<Landed> {
        let _held = WRITES.lock().await;
        let file = self.read(|s| s.team.workspace(c.ws).and_then(|w| w.coedit.files.get(c.name)).map(|f| (f.format, f.head().map_or(0, |h| h.number))));
        let Some((format, head)) = file else { return Ok(Landed::Refused(format!("{} is not a co-edited file here", c.name))) };
        if !format.merges() {
            return Ok(Landed::Refused(format!("{} is a binary file, edited by one person at a time who checks it out in the app; an agent does not edit it", c.name)));
        }
        if c.base == 0 || c.base > head {
            return Ok(Landed::Refused(format!("{} has no version {}; it is at version {head}", c.name, c.base)));
        }
        if let Some(why) = format.refusal(c.name, c.mine) {
            return Ok(Landed::Refused(why));
        }
        let base = self.coedit_bytes(c.ws, c.name, c.base).await?.context("the base version's content is gone")?;
        if c.base != head {
            let theirs = self.coedit_bytes(c.ws, c.name, head).await?.context("the head version's content is gone")?;
            if let Merged::Conflicts(conflicts) = merge::merge(format, &base, c.mine, &theirs, &Choices::new()) {
                let id = new_id("edit");
                let kept = Proposal {
                    id: id.clone(),
                    name: c.name.into(),
                    base: c.base,
                    author: c.author.clone(),
                    at: Utc::now(),
                    data_b64: B64.encode(c.mine),
                    diff: merge::diff(format, &base, c.mine),
                    mission: None,
                    lane: None,
                    conflicts: conflicts.clone(),
                };
                self.emit(actor, coedit(c.ws, Step::Proposed(kept))).await?;
                return Ok(Landed::Rejected { id, head, conflicts });
            }
        }
        let earlier: Vec<String> = self
            .read(|s| s.team.workspace(c.ws).map(|w| w.coedit.of_lane(mission, lane).into_iter().filter(|o| o.name == c.name).map(|o| o.id.clone()).collect()).unwrap_or_default());
        let id = new_id("edit");
        let p = Proposal {
            id: id.clone(),
            name: c.name.into(),
            base: c.base,
            author: c.author.clone(),
            at: Utc::now(),
            data_b64: B64.encode(c.mine),
            diff: merge::diff(format, &base, c.mine),
            mission: Some(mission.into()),
            lane: Some(lane.into()),
            conflicts: vec![],
        };
        let mut evs: Vec<(&str, Ev)> = earlier.into_iter().map(|id| (actor, coedit(c.ws, Step::Settled { id, version: None }))).collect();
        evs.push((actor, coedit(c.ws, Step::Proposed(p))));
        self.emit_all(evs).await?;
        Ok(Landed::Proposed(id))
    }

    /// Land an open proposal: its author's own decision, `choices` settling
    /// what conflicted, or its lane's release.
    async fn land_open(&self, actor: &str, ws: &str, open: &Open, choices: &Choices) -> Result<Landed> {
        let mine = self.step_bytes(open.seq).await?.context("the proposal's content is gone")?;
        let c = Draft { ws, name: &open.name, base: open.base, mine: &mine, author: &open.author };
        self.land(actor, &c, choices, Some(open)).await
    }

    fn sees(&self, ws: &str, name: &str, person: &str) -> bool {
        self.read(|s| s.team.workspace(ws).is_some_and(|w| w.sees_file(name, person)))
    }

    /// `person`'s open proposal `id`, while they still see its file. One
    /// whose file they no longer see is dropped: it can never land.
    async fn own_open(&self, actor: &str, ws: &str, person: &str, id: &str) -> Result<Option<Open>> {
        let open = self.read(|s| s.team.workspace(ws).and_then(|w| w.coedit.open.get(id)).filter(|o| o.author.person == person).cloned());
        let Some(open) = open else { return Ok(None) };
        if !self.sees(ws, &open.name, person) {
            self.emit(actor, coedit(ws, Step::Settled { id: id.into(), version: None })).await?;
            return Ok(None);
        }
        Ok(Some(open))
    }

    /// `person` lands their open proposal `id` (theirs, or their agent's).
    pub(in crate::team) async fn coedit_settle(&self, actor: &str, ws: &str, person: &str, id: &str, choices: &Choices) -> Result<Landed> {
        let _held = WRITES.lock().await;
        let Some(open) = self.own_open(actor, ws, person, id).await? else { return Ok(Landed::Refused("you have no such open change".into())) };
        self.land_open(actor, ws, &open, choices).await
    }

    /// `person` throws their open proposal `id` away.
    pub(in crate::team) async fn coedit_discard(&self, actor: &str, ws: &str, person: &str, id: &str) -> Result<bool> {
        let _held = WRITES.lock().await;
        if self.own_open(actor, ws, person, id).await?.is_none() {
            return Ok(false);
        }
        self.emit(actor, coedit(ws, Step::Settled { id: id.into(), version: None })).await?;
        Ok(true)
    }

    /// Land every edit waiting with a lane's result, as its release does.
    /// `Ok(Err)` names each file that conflicts now (its proposal kept with
    /// both versions for the lane's owner to decide); the lane then waits.
    pub(in crate::team) async fn coedit_land_lane(
        &self,
        actor: &str,
        ws: &str,
        mission: &str,
        lane: &str,
    ) -> Result<std::result::Result<Vec<u32>, Vec<(String, String, Vec<Conflict>)>>> {
        let _held = WRITES.lock().await;
        let waiting: Vec<Open> = self.read(|s| s.team.workspace(ws).map(|w| w.coedit.of_lane(mission, lane).into_iter().cloned().collect()).unwrap_or_default());
        let (mut landed, mut stuck) = (vec![], vec![]);
        for open in waiting {
            // An edit whose person no longer sees its file never lands, and
            // its conflicts (the file as it is now) are not shown to them.
            if !self.sees(ws, &open.name, &open.author.person) {
                self.emit(actor, coedit(ws, Step::Settled { id: open.id.clone(), version: None })).await?;
                continue;
            }
            if !open.conflicts.is_empty() {
                stuck.push((open.id.clone(), open.name.clone(), open.conflicts.clone()));
                continue;
            }
            match self.land_open(actor, ws, &open, &Choices::new()).await? {
                Landed::Version(n) => landed.push(n),
                Landed::Rejected { id, conflicts, .. } => stuck.push((id, open.name.clone(), conflicts)),
                Landed::Proposed(_) => {}
                // The file went, or cannot take it: the edit goes too.
                Landed::Refused(why) => {
                    tracing::info!("an edit to {} waiting with lane {lane} cannot land: {why}", open.name);
                    self.emit(actor, coedit(ws, Step::Settled { id: open.id.clone(), version: None })).await?;
                }
            }
        }
        Ok(if stuck.is_empty() { Ok(landed) } else { Err(stuck) })
    }

    /// `person` checks out a binary file: nobody else writes it until they
    /// check it in or the lease ends. Checking out again renews the lease.
    pub(in crate::team) async fn coedit_check_out(&self, actor: &str, ws: &str, name: &str, person: &str) -> Result<std::result::Result<chrono::DateTime<Utc>, String>> {
        let _held = WRITES.lock().await;
        let now = Utc::now();
        let file = self.read(|s| s.team.workspace(ws).and_then(|w| w.coedit.files.get(name)).map(|f| (f.format, f.held(now).cloned())));
        let Some((format, lease)) = file else { return Ok(Err(format!("{name} is not a co-edited file here"))) };
        if format.merges() {
            return Ok(Err(format!("{name} merges by {}, so it is never checked out", if format == Format::Sheet { "cell" } else { "part" })));
        }
        if let Some(l) = lease.filter(|l| l.person != person) {
            let who = crate::team::chat_routes::name_of(self, &l.person);
            return Ok(Err(format!("{who} has it checked out until {}", l.until.format("%Y-%m-%d %H:%M UTC"))));
        }
        let until = now + chrono::Duration::from_std(CHECKOUT_LEASE_SECS)?;
        self.emit(actor, coedit(ws, Step::CheckedOut { name: name.into(), person: person.into(), until })).await?;
        Ok(Ok(until))
    }

    /// `person` checks a file in: others may write it again.
    pub(in crate::team) async fn coedit_check_in(&self, actor: &str, ws: &str, name: &str, person: &str) -> Result<bool> {
        let _held = WRITES.lock().await;
        let holds = self.read(|s| s.team.workspace(ws).and_then(|w| w.coedit.files.get(name)).and_then(|f| f.lease.as_ref()).is_some_and(|l| l.person == person));
        if holds {
            self.emit(actor, coedit(ws, Step::CheckedIn { name: name.into(), person: person.into() })).await?;
        }
        Ok(holds)
    }

    /// Version `to` again, as the newest version: a revert is one more version.
    pub(in crate::team) async fn coedit_revert(&self, actor: &str, ws: &str, name: &str, to: u32, person: &str) -> Result<Landed> {
        let _held = WRITES.lock().await;
        let head = self.read(|s| s.team.workspace(ws).and_then(|w| w.coedit.files.get(name)).and_then(|f| f.head()).map(|h| h.number));
        let (Some(head), Some(bytes)) = (head, self.coedit_bytes(ws, name, to).await?) else { return Ok(Landed::Refused(format!("{name} has no version {to}"))) };
        let author = Author { person: person.into(), run: None };
        self.land(actor, &Draft { ws, name, base: head, mine: &bytes, author: &author }, &Choices::new(), None).await
    }

    /// Erasing `person`: each version they wrote is written again with "a
    /// former member" as its author, its content kept (owner, 2026-11-23: a
    /// quote's line items are business records, and the personal data in
    /// them is the attribution). The events that named them are returned,
    /// for the erasure to take their content out of the log.
    pub(in crate::team) async fn coedit_reattribute(&self, actor: &str, person: &str) -> Result<Vec<i64>> {
        let _held = WRITES.lock().await;
        let theirs: Vec<(String, i64)> = self.read(|s| {
            let ws = s.team.workspaces.values();
            ws.flat_map(|w| w.coedit.files.values().flat_map(|f| f.versions.values()).filter(|m| m.author.person == person).map(|m| (w.id.clone(), m.seq))).collect()
        });
        if theirs.is_empty() {
            return Ok(vec![]);
        }
        let seqs: Vec<i64> = theirs.iter().map(|(_, seq)| *seq).collect();
        let bodies = self.log.contents(&seqs).await?;
        let mut evs = vec![];
        for (ws, seq) in &theirs {
            let step = bodies.get(seq).and_then(|b| serde_json::from_value::<Step>(b["coedit"].clone()).ok());
            let Some(Step::Version(mut v) | Step::Reattributed(mut v)) = step else { continue };
            v.author = Author { person: FORMER_MEMBER.into(), run: None };
            evs.push((actor, coedit(ws, Step::Reattributed(v))));
        }
        if !evs.is_empty() {
            self.emit_all(evs).await?;
        }
        Ok(seqs)
    }
}
