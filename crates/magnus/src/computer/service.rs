//! The operations on a computer: who may, what the log says, what computerd does.

use super::access::Access;
use super::client::{self, GuestRequest, Request};
use super::{apps, Change, Computer, KillMode, Owner, Role, Status};
use crate::config::Person;
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::people::Authority;
use crate::state::State;
use crate::team::Role as WorkspaceRole;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BOOT_WAIT: Duration = Duration::from_secs(60);
const BOOT_POLL: Duration = Duration::from_millis(200);
const QUEUE_POLL: Duration = Duration::from_secs(5);
const ID_BYTES: usize = 16;
const MAX_LABEL_CHARS: usize = 200;
pub(super) const USER_FILES: &str = "/workspace/";
const MAX_HOST_CHARS: usize = 253;
/// Where the guest looks for certificate authorities to trust.
const GUEST_CA_FILE: &str = "/usr/local/share/ca-certificates/magnus-computer.crt";
/// How long the guest gives `update-ca-certificates`.
const CA_UPDATE_SECONDS: u64 = 60;
/// As long as filling a whole volume can take.
const ZERO_FREE_WAIT: Duration = Duration::from_secs(30 * 60);

/// A person's computer is theirs alone to change, read or run things on:
/// "Nothing on one user's computer is reachable from another's" (spec rev
/// 145, Multi-agent and multi-user boundaries), the server's owner and
/// admins included. A workspace's is its owners' and admins'.
pub(super) fn may_control(s: &State, who: &Person, owner: &Owner) -> bool {
    match owner {
        Owner::Person(p) => who.id == *p,
        Owner::Workspace(w) => s.team.role(w, &who.id).is_some_and(|r| r >= WorkspaceRole::Admin),
    }
}

/// Stopping is always safe, so a server owner or admin may pause anyone's
/// computer (spec rev 145, Agents: "An admin can pause or stop any one of
/// them"); nothing more.
pub(super) fn may_stop(s: &State, who: &Person, authority: Authority, owner: &Owner) -> bool {
    may_control(s, who, owner) || authority.administers()
}

pub(super) fn failed(e: anyhow::Error) -> Refused {
    Refused::Failed(format!("{e:#}"))
}

/// A host name, or `*.` and a host name.
pub(super) fn checked_host(raw: &str) -> Result<String, Refused> {
    let host = raw.trim().to_ascii_lowercase();
    let bare = host.strip_prefix("*.").unwrap_or(&host);
    if bare.is_empty() || bare.len() > MAX_HOST_CHARS || !bare.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err(Refused::Invalid(format!("{raw} is not a host name")));
    }
    // `*.com` or `*.github.io` would open every site under a public suffix.
    if host.starts_with("*.") && !crate::net_guard::may_grant_subdomains(bare) {
        return Err(Refused::Invalid(format!("{raw} covers sites nobody here owns: name the host itself")));
    }
    Ok(host)
}

pub(super) fn new_id() -> String {
    use rand::RngCore;
    let mut raw = [0u8; ID_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

/// Only under the volume's mount point, and no way back out of it.
pub(super) fn user_path(path: &str) -> Result<&str, Refused> {
    if !path.starts_with(USER_FILES) || path.len() == USER_FILES.len() || path.split('/').any(|p| p == "..") {
        return Err(Refused::Invalid(format!("a file on a computer is under {USER_FILES}")));
    }
    Ok(path)
}

impl Kernel {
    pub(super) fn computer_socket(&self) -> Result<PathBuf, Refused> {
        self.cfg.computer.as_ref().map(|c| c.socket.clone()).ok_or_else(|| Refused::Conflict("this server has no computers".into()))
    }

    /// The person sees "no such computer" for one that is not theirs.
    pub(super) fn controlled_computer(&self, who: &Person, id: &str) -> Result<Computer, Refused> {
        self.computer_as(who, id, Access::Control)
    }

    /// A computer `who` may pause, and nothing else.
    async fn stoppable_computer(&self, who: &Person, id: &str) -> Result<Computer, Refused> {
        let authority = self.authority(who).await;
        self.computer_as(who, id, Access::Stop(authority))
    }

    pub(super) async fn record_computer(&self, who: &Person, change: Change) -> Result<(), Refused> {
        self.emit(&format!("person:{}", who.id), Ev::Computer(change)).await.map(|_| ()).map_err(failed)
    }

    /// The person's own computer and those of the workspaces they look
    /// after: one each, never a choice between several (spec rev 142).
    /// Another person's computer is not listed here, even to an admin.
    pub fn computers_of(&self, who: &Person) -> Value {
        let computers: Vec<Value> = self.read(|s| {
            let queue: Vec<&str> = s.computers.queue().iter().map(|c| c.id.as_str()).collect();
            let shown = |c: &Computer| {
                let mut v = json!(c);
                v["name"] = json!(super::driving::computer_name(s, c));
                v["driving"] = super::driving::driving(s, c);
                v["waking"] = json!(super::wake::waking(&c.id));
                if let Some(i) = queue.iter().position(|q| *q == c.id) {
                    v["queue_position"] = json!(i + 1);
                }
                v
            };
            let listed = |o: &Owner| match o {
                Owner::Person(p) => *p == who.id,
                Owner::Workspace(_) => may_control(s, who, o),
            };
            let owners: Vec<&Owner> = s.computers.all().filter(|c| c.role == Role::Computer && listed(&c.owner)).map(|c| &c.owner).collect();
            let mut one_each: Vec<&Computer> = vec![];
            for o in owners {
                if let Some(c) = s.computers.one_of(o).filter(|c| !one_each.iter().any(|k| k.id == c.id)) {
                    one_each.push(c);
                }
            }
            one_each.into_iter().map(shown).collect()
        });
        json!({ "computers": computers.into_iter().map(|v| super::driving::with_names(self, v)).collect::<Vec<_>>() })
    }

    pub fn computer_json(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        let (position, driving, name) = self.read(|s| (s.computers.queue().iter().position(|q| q.id == id), super::driving::driving(s, &c), super::driving::computer_name(s, &c)));
        let mut v = json!(c);
        v["name"] = json!(name);
        v["driving"] = driving;
        v["waking"] = json!(super::wake::waking(id));
        if let Some(i) = position {
            v["queue_position"] = json!(i + 1);
        }
        Ok(super::driving::with_names(self, v))
    }

    /// Wakes or rebuilds a computer, or puts it in the queue at the limit.
    pub async fn start_computer(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        self.start_this(who, &c).await
    }

    /// Starts `c`, once the caller has decided `who` may.
    pub(super) async fn start_this(&self, who: &Person, c: &Computer) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        let id = c.id.as_str();
        match c.status {
            Status::Running => return Err(Refused::Conflict("the computer is already running".into())),
            Status::Queued => return Err(Refused::Conflict("the computer is already waiting to start".into())),
            Status::Paused if c.held_by.as_deref().is_some_and(|r| self.switch_still_on(r)) => {
                return Err(Refused::Conflict("a kill switch holds the computer; it resumes when the switch is released".into()));
            }
            Status::Paused => {
                client::ask(&socket, Request::Resume { id: id.to_string() }).await.map_err(failed)?;
                self.record_computer(who, Change::Started { id: id.into() }).await?;
                return Ok(json!({ "status": "running" }));
            }
            Status::Stopped | Status::Hibernated | Status::Killed => {}
        }
        if !super::wake::claim(id) {
            return Err(Refused::Conflict("the computer is already waking".into()));
        }
        // Wake boots cold when there is no snapshot it can use.
        self.wake_claimed(id, &format!("person:{}", who.id)).await
    }

    /// Opening a computer's screen or files wakes it, as any use does; the
    /// person does not wait for the wake, and sees it go.
    pub fn wake_computer(self: &Arc<Self>, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.opened_computer(who, id).or_else(|_| self.controlled_computer(who, id))?;
        self.wake_in_background(id, &format!("person:{}", who.id));
        Ok(json!({ "status": c.status, "waking": super::wake::waking(id) }))
    }

    /// A new overlay has none of the computer's apps yet.
    pub(super) async fn after_boot(&self, socket: &Path, id: &str, woke: &Value) {
        self.apply_desktop_theme(socket, id).await;
        if woke["resumed"] != true {
            // A resumed desktop is running already.
            self.start_desktop(socket, id).await;
            // On every cold boot, so a CA change reaches an overlay that stayed.
            if let Err(e) = self.trust_computer_ca(socket, id).await {
                tracing::warn!("computer {id} does not trust its proxy yet: {e:#}");
            }
        }
        if woke["overlay_fresh"] != true {
            return;
        }
        let list = self.read(|s| s.computers.get(id).map(|c| c.apps.clone())).unwrap_or_default();
        for app in &list {
            if let Err(e) = apps::replay(&self.computer_installers(), socket, id, app).await {
                tracing::warn!("computer {id} could not replay {}: {e:#}", app.name);
                if let Err(e) = self.emit("kernel", Ev::Computer(Change::ReplayFailed { id: id.into(), app: app.name.clone(), error: format!("{e:#}") })).await {
                    tracing::warn!("computer {id}'s failed replay could not be recorded: {e:#}");
                }
                return;
            }
            self.add_app_to_menu(socket, id, &app.name).await;
        }
    }

    /// Reachable from this computer. Goes through the boundary once the agent
    /// can ask for it; for now only a person who controls the computer can.
    pub async fn add_computer_destination(&self, who: &Person, id: &str, host: &str) -> Result<Value, Refused> {
        self.controlled_computer(who, id)?;
        let host = checked_host(host)?;
        self.record_computer(who, Change::DestinationAdded { id: id.into(), host: host.clone() }).await?;
        Ok(json!({ "added": host }))
    }

    pub async fn open_computer_page(&self, who: &Person, id: &str, url: &str) -> Result<Value, Refused> {
        self.controlled_computer(who, id)?;
        let parsed = url::Url::parse(url).map_err(|_| Refused::Invalid("not a URL".into()))?;
        let host = checked_host(parsed.host_str().unwrap_or_default())?;
        let page = new_id();
        self.record_computer(who, Change::PageOpened { id: id.into(), page: page.clone(), host }).await?;
        Ok(json!({ "page": page }))
    }

    pub async fn close_computer_page(&self, who: &Person, id: &str, page: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        if !c.pages.iter().any(|p| p.id == page) {
            return Err(Refused::NotFound("page"));
        }
        self.record_computer(who, Change::PageClosed { id: id.into(), page: page.into() }).await?;
        Ok(json!({ "closed": page }))
    }

    /// TLS through the proxy works for what the guest trusts, and nothing else.
    async fn trust_computer_ca(&self, socket: &Path, id: &str) -> Result<()> {
        use base64::Engine;
        let pem = super::egress::certificate_pem(&self.vault, id)?;
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(pem);
        client::guest(socket, id, GuestRequest::write(GUEST_CA_FILE, data_b64)).await?;
        client::guest(socket, id, GuestRequest::exec("update-ca-certificates", CA_UPDATE_SECONDS)).await?;
        Ok(())
    }

    pub async fn hibernate_computer(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        let c = self.controlled_computer(who, id)?;
        if c.status != Status::Running {
            return Err(Refused::Conflict("only a running computer can hibernate".into()));
        }
        let Some(_held) = super::wake::claim_hibernation(id) else {
            return Err(Refused::Conflict("the computer is waking or already going to sleep".into()));
        };
        self.zero_and_hibernate(&socket, &c).await.map_err(failed)?;
        self.record_computer(who, Change::Hibernated { id: id.into() }).await?;
        Ok(json!({ "status": "hibernated" }))
    }

    /// How many computers run and exist on this machine, against its limits,
    /// as the computer service counts them (Admin, Operations).
    pub async fn computer_capacity(&self) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        client::ask(&socket, Request::Capacity).await.map_err(failed)
    }

    pub async fn backup_computer(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        self.controlled_computer(who, id)?;
        client::ask(&socket, Request::Backup { id: id.to_string() }).await.map_err(failed)
    }

    pub async fn restore_computer(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        self.controlled_computer(who, id)?;
        let v = client::ask(&socket, Request::RestoreBackup { id: id.to_string() }).await.map_err(failed)?;
        self.record_computer(who, Change::Restored { id: id.into(), at: v["restored"].as_u64().unwrap_or_default() }).await?;
        Ok(v)
    }

    pub async fn delete_computer_file(&self, who: &Person, id: &str, path: &str) -> Result<Value, Refused> {
        let c = self.computer_as(who, id, Access::Open)?;
        let path = user_path(path)?;
        self.computer_file_for(&c, GuestRequest::Erase { path: path.to_string() }).await?;
        self.record_computer(who, Change::FileDeleted { id: id.into(), path: path.into(), at: chrono::Utc::now() }).await?;
        Ok(json!({ "deleted": path }))
    }

    pub async fn grant_computer(&self, who: &Person, id: &str, label: &str) -> Result<Value, Refused> {
        let c = self.controlled_computer(who, id)?;
        if c.status == Status::Killed {
            return Err(Refused::Conflict("the computer has been killed".into()));
        }
        let label = label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
            return Err(Refused::Invalid(format!("a grant has a label of 1 to {MAX_LABEL_CHARS} characters")));
        }
        let grant = new_id();
        self.record_computer(who, Change::Granted { id: id.into(), grant: grant.clone(), label: label.into() }).await?;
        Ok(json!({ "grant": grant }))
    }

    /// One step: the log first, so nothing can act on it from here on, then the VM.
    pub async fn kill_computer(&self, who: &Person, id: &str, mode: KillMode, delete_volume: bool) -> Result<Value, Refused> {
        let c = if mode == KillMode::Pause && !delete_volume { self.stoppable_computer(who, id).await? } else { self.controlled_computer(who, id)? };
        self.kill_this(who, &c, mode, delete_volume).await
    }

    /// Kills `c`, once the caller has decided `who` may.
    pub(super) async fn kill_this(&self, who: &Person, c: &Computer, mode: KillMode, delete_volume: bool) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        let id = c.id.as_str();
        if delete_volume && mode != KillMode::Destroy {
            return Err(Refused::Invalid("only destroying a computer can delete its volume".into()));
        }
        // Destroy became reset (spec rev 142): deleting the volume erases it,
        // and the same computer is rebuilt with an empty one.
        if delete_volume {
            return self.erase_computer(who, c).await;
        }
        let actor = format!("person:{}", who.id);
        let mut evs = vec![];
        for g in c.grants.iter().filter(|g| !g.revoked) {
            evs.push((actor.as_str(), Ev::Computer(Change::Revoked { id: id.into(), grant: g.id.clone() })));
        }
        let revoked = evs.len();
        evs.push((actor.as_str(), Ev::Computer(Change::Killed { id: id.into(), mode, delete_volume })));
        self.emit_all(evs).await.map_err(failed)?;
        let req = match mode {
            KillMode::Pause if c.status == Status::Running => Request::Pause { id: id.to_string() },
            KillMode::Pause => return Ok(json!({ "revoked": revoked })),
            KillMode::Destroy => Request::Destroy { id: id.to_string(), delete_volume: false },
        };
        client::ask(&socket, req).await.map_err(failed)?;
        Ok(json!({ "revoked": revoked }))
    }

    /// The monitor's kill switch: a pause and every grant revoked, one step.
    pub(crate) async fn pause_computer_from_monitor(&self, id: &str) -> Result<()> {
        self.pause_computer_as("monitor", id, None).await
    }

    /// A pause and every grant revoked, one step, with `also` in the same
    /// write (a kill switch's mark).
    pub(crate) async fn pause_computer_as(&self, actor: &str, id: &str, also: Option<Change>) -> Result<()> {
        let socket = self.computer_socket().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let Some(c) = self.read(|s| s.computers.get(id).cloned()) else { return Ok(()) };
        let mut evs: Vec<(&str, Ev)> = c.grants.iter().filter(|g| !g.revoked).map(|g| (actor, Ev::Computer(Change::Revoked { id: id.into(), grant: g.id.clone() }))).collect();
        evs.push((actor, Ev::Computer(Change::Killed { id: id.into(), mode: KillMode::Pause, delete_volume: false })));
        evs.extend(also.map(|ch| (actor, Ev::Computer(ch))));
        self.emit_all(evs).await?;
        client::ask(&socket, Request::Pause { id: id.to_string() }).await?;
        Ok(())
    }

    pub async fn write_computer_file(&self, who: &Person, id: &str, path: &str, data_b64: &str) -> Result<Value, Refused> {
        let c = self.computer_as(who, id, Access::Open)?;
        let path = user_path(path)?;
        let v = self.computer_file_for(&c, GuestRequest::write(path, data_b64)).await?;
        Ok(json!({ "written": path, "reply": v }))
    }

    /// One directory of the person's files. The top is their files' root.
    pub async fn list_computer_files(&self, who: &Person, id: &str, path: &str) -> Result<Value, Refused> {
        let c = self.computer_as(who, id, Access::Open)?;
        let path = if path == USER_FILES { path } else { user_path(path)? };
        self.computer_file_for(&c, GuestRequest::List { path: path.to_string() }).await
    }

    /// One file, whole, read in pieces.
    pub async fn read_computer_file(&self, who: &Person, id: &str, path: &str) -> Result<Value, Refused> {
        let bytes = self.computer_file_raw(who, id, path).await?;
        Ok(super::files::as_reply(path, &bytes))
    }

    /// One file's bytes, for a download.
    pub async fn computer_file_raw(&self, who: &Person, id: &str, path: &str) -> Result<Vec<u8>, Refused> {
        let c = self.computer_as(who, id, Access::Open)?;
        let path = user_path(path)?;
        self.with_files(&c, true, |f| async move { f.read_whole(path, super::files::MAX_FILE_BYTES).await }).await
    }

    /// A deleted file's earlier versions linger in free blocks, so free space
    /// is zeroed once the computer goes idle, where filling it harms no one.
    /// The host gets those blocks back as soon as the VM has stopped.
    pub(super) async fn zero_and_hibernate(&self, socket: &Path, c: &Computer) -> anyhow::Result<()> {
        // Kept to show while it sleeps and wakes again.
        if let Err(e) = super::wake::screenshot(socket, &c.id).await {
            tracing::warn!("computer {}: no screenshot was kept before it slept: {e:#}", c.id);
        }
        let zeroed = self.zero_deleted_blocks(socket, c).await;
        client::ask(socket, Request::Hibernate { id: c.id.clone() }).await?;
        if zeroed {
            if let Err(e) = client::ask(socket, Request::DigHoles { id: c.id.clone() }).await {
                tracing::warn!("computer {}: the host did not get its blocks back: {e:#}", c.id);
            }
        }
        Ok(())
    }

    async fn zero_deleted_blocks(&self, socket: &Path, c: &Computer) -> bool {
        if c.deleted_files.last().is_none_or(|d| c.free_space_zeroed.is_some_and(|at| at >= d.at)) {
            return false;
        }
        match client::ask(socket, Request::ZeroRoom { id: c.id.clone() }).await {
            Ok(v) if v["room"] == true => {}
            Ok(_) => {
                tracing::warn!("computer {}: the host has no room to zero its free space, so deleted files may linger there", c.id);
                return false;
            }
            Err(e) => {
                tracing::warn!("computer {}: free space was not zeroed: {e:#}", c.id);
                return false;
            }
        }
        match client::guest_for(socket, &c.id, GuestRequest::ZeroFree, ZERO_FREE_WAIT).await {
            Ok(v) if v["error"].is_null() => {
                let change = Change::FreeSpaceZeroed { id: c.id.clone(), at: chrono::Utc::now() };
                if let Err(e) = self.emit("kernel", Ev::Computer(change)).await {
                    tracing::warn!("computer {}'s zeroing could not be recorded: {e:#}", c.id);
                }
                true
            }
            Ok(v) => {
                tracing::warn!("computer {}: free space was not zeroed: {}", c.id, v["error"]);
                false
            }
            Err(e) => {
                tracing::warn!("computer {}: free space was not zeroed: {e:#}", c.id);
                false
            }
        }
    }
}

pub(super) async fn wait_for_guest(socket: &Path, id: &str) -> Result<()> {
    let started = Instant::now();
    loop {
        match client::guest(socket, id, GuestRequest::Ping).await {
            Ok(_) => return Ok(()),
            Err(e) if started.elapsed() > BOOT_WAIT => return Err(e.context("the computer did not boot")),
            Err(_) => tokio::time::sleep(BOOT_POLL).await,
        }
    }
}

/// A kill recorded just before a crash still happens, and waiting wakes start
/// in their turn.
pub fn schedule(k: Arc<Kernel>) {
    let Some(socket) = k.cfg.computer.as_ref().map(|c| c.socket.clone()) else { return };
    tokio::spawn(super::egress::serve(k.clone(), socket.clone()));
    super::service_version::schedule(socket.clone());
    super::idle::schedule(k.clone());
    super::one::schedule(k.clone());
    k.sweep_computer_secrets();
    tokio::spawn(async move {
        let killed: Vec<String> = k.read(|s| s.computers.all().filter(|c| c.status == Status::Killed).map(|c| c.id.clone()).collect());
        for id in killed {
            if let Err(e) = client::ask(&socket, Request::Stop { id: id.to_string() }).await {
                tracing::warn!("could not finish killing computer {id}: {e:#}");
            }
        }
        let mut tick = tokio::time::interval(QUEUE_POLL);
        loop {
            tick.tick().await;
            let next = k.read(|s| s.computers.queue().first().map(|c| c.id.clone()));
            let Some(id) = next else { continue };
            if !super::wake::claim(&id) {
                continue;
            }
            match client::ask(&socket, Request::Wake { id: id.to_string() }).await {
                Ok(woke) => {
                    if let Err(e) = k.emit("kernel", Ev::Computer(Change::Started { id: id.clone() })).await {
                        tracing::warn!("computer {id} started but could not be recorded: {e:#}");
                    }
                    super::wake::set_phase(&id, super::wake::Phase::Booting);
                    let (k, socket) = (k.clone(), socket.clone());
                    tokio::spawn(async move {
                        match wait_for_guest(&socket, &id).await {
                            Ok(()) => {
                                super::wake::set_phase(&id, super::wake::Phase::Restoring);
                                k.after_boot(&socket, &id, &woke).await;
                            }
                            Err(e) => tracing::warn!("computer {id} did not come up from the queue: {e:#}"),
                        }
                        super::wake::release(&id);
                    });
                }
                Err(e) => {
                    super::wake::release(&id);
                    if !client::at_capacity(&e) {
                        tracing::warn!("computer {id} could not start from the queue: {e:#}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_path_stays_on_the_volume() {
        assert!(user_path("/workspace/a/b.txt").is_ok());
        for bad in ["/etc/passwd", "/workspace/", "/workspace/../etc/passwd", "workspace/a", ""] {
            assert!(user_path(bad).is_err(), "{bad} was allowed");
        }
    }

    /// An admin in the server's own directory, with no admin role in the
    /// config, may pause anyone's computer, as a config admin may; a member
    /// may pause only their own, and nobody else may change one.
    #[test]
    fn a_directory_admin_stops_a_computer_and_changes_none() {
        use crate::accounts::Role as Directory;
        let s = State::default();
        let lee: Person = serde_json::from_value(json!({ "id": "lee" })).unwrap();
        let mo = Owner::Person("mo".into());
        assert!(may_stop(&s, &lee, Authority::of(&lee, Directory::Admin), &mo), "a directory admin could not pause another's computer");
        assert!(!may_stop(&s, &lee, Authority::of(&lee, Directory::Member), &mo), "a member paused another's computer");
        assert!(may_stop(&s, &lee, Authority::Member, &Owner::Person("lee".into())), "a member could not pause their own");
        assert!(!may_control(&s, &lee, &mo), "an admin may change another's computer");
    }

    #[test]
    fn a_new_id_is_one_computerd_accepts() {
        let id = new_id();
        assert!(id.len() <= 32 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "{id}");
    }
}
