//! Publish bytes and their visibility together. Immutable objects are durable
//! before the log points to them; a failed commit leaves an unreachable object.
use super::{Change, File};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use anyhow::Result;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

pub(super) fn path(k: &Kernel, ws: &str, name: &str) -> PathBuf {
    let blob = k.read(|s| s.team.workspace(ws).and_then(|w| w.file_blobs.get(name).cloned()));
    match blob {
        Some(blob) => super::agent::volume_dir(k, ws).with_file_name("objects").join(blob),
        None => super::agent::volume_dir(k, ws).join(name),
    }
}

impl Kernel {
    /// Caller holds team_files and has checked membership, ownership and scope.
    pub(super) async fn publish_team_file(&self, actor: &str, ws: &str, file: File, project: Option<String>, bytes: &[u8]) -> Result<()> {
        let old = path(self, ws, &file.name);
        let blob = new_id("blob");
        let dir = super::agent::volume_dir(self, ws).with_file_name("objects");
        tokio::fs::create_dir_all(&dir).await?;
        let mut out = tokio::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&blob)).await?;
        out.write_all(bytes).await?;
        out.sync_all().await?;
        // Linux is the server platform: make the new directory entry durable too.
        tokio::fs::File::open(&dir).await?.sync_all().await?;
        self.emit(actor, Ev::Team(Change::FilePublished { workspace: ws.into(), file, project, blob })).await?;
        if let Err(e) = tokio::fs::remove_file(old).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("reclaiming superseded team file: {e}");
            }
        }
        Ok(())
    }
}
