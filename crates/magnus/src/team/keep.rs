//! What a member puts into a workspace stays theirs. The owner, 2026-11-11:
//! "only what they share should be shared through teams. The user should
//! still own it. The teams admin can moderate and delete it only from
//! teams." So a file built from one member's data, held for them in the
//! workspace (spec rev 145, Shared files: it "goes to that member's space"),
//! is copied into their own library when they share it, when they leave or
//! are removed, and when the workspace is deleted: whatever then happens to
//! the team's copy, theirs is untouched. And nobody, the team agent acting
//! for someone included, writes over a file someone else put on the volume.
use super::agent::held_dir;
use super::{File, Workspace};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use anyhow::Result;
use base64::Engine;

/// Whether `name` on the team volume was put there by someone other than
/// `person`, so writing it would replace their file.
pub(super) fn someone_elses(w: &Workspace, name: &str, person: &str) -> bool {
    w.files.get(name).is_some_and(|f| f.by != person)
}

/// What the refusal says.
pub(super) const SOMEONE_ELSES: &str = "a file by that name is someone else's on the team volume; give yours another name";

impl Kernel {
    /// A copy of `bytes` as `name` in `person`'s own library.
    pub(super) async fn keep_in_library(&self, actor: &str, person: &str, name: &str, bytes: &[u8]) -> Result<String> {
        let id = new_id("file");
        let ev = Ev::FileUploaded {
            id: id.clone(),
            person: person.to_string(),
            name: name.to_string(),
            media_type: crate::boundary::media_type_for(name),
            data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
            thread: None,
        };
        self.emit(actor, ev).await?;
        Ok(id)
    }

    /// Every file still held for `person` in `ws` goes to their library,
    /// before the held folder goes.
    pub(super) async fn keep_held(&self, actor: &str, ws: &str, person: &str) -> Result<()> {
        let held: Vec<File> = self.read(|s| s.team.workspace(ws).and_then(|w| w.held.get(person)).map(|f| f.values().cloned().collect()).unwrap_or_default());
        let dir = held_dir(self, ws, person);
        for f in held {
            let bytes = tokio::fs::read(dir.join(&f.name)).await?;
            self.emit_all(vec![
                (
                    actor,
                    Ev::FileUploaded {
                        id: new_id("file"),
                        person: person.into(),
                        name: f.name.clone(),
                        media_type: crate::boundary::media_type_for(&f.name),
                        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
                        thread: None,
                    },
                ),
                (actor, Ev::Team(super::Change::Released { workspace: ws.into(), person: person.into(), name: f.name })),
            ])
            .await?;
        }
        Ok(())
    }

    /// `keep_held` for everyone who holds something in `ws`.
    pub(super) async fn keep_all_held(&self, actor: &str, ws: &str) -> Result<()> {
        let people: Vec<String> = self.read(|s| s.team.workspace(ws).map(|w| w.held.iter().filter(|(_, f)| !f.is_empty()).map(|(p, _)| p.clone()).collect()).unwrap_or_default());
        for p in &people {
            self.keep_held(actor, ws, p).await?;
        }
        Ok(())
    }

    /// Revocation is immediate; failed preservation is retried from its durable metadata.
    pub(crate) async fn finish_removed_files(&self) {
        let _files = self.team_files.lock().await;
        let pending: Vec<(String, String)> =
            self.read(|s| s.team.all().flat_map(|w| w.held.keys().filter(|p| !w.members.contains_key(*p)).map(|p| (w.id.clone(), p.clone()))).collect());
        for (ws, person) in pending {
            match self.keep_held("kernel", &ws, &person).await {
                Ok(()) => self.remove_held(&ws, &person).await,
                Err(e) => tracing::warn!("preserving removed member's files in {ws}: {e:#}"),
            }
        }
    }
}
