//! The rest of an admin's job: ceilings, who approves, the audit, and ending
//! things properly when someone or something goes.
use super::agent::{held_dir, volume_dir};
use super::routes::{member, send, Reply, K};
use super::{Change, Role};
use crate::api::err;
use crate::computer::{KillMode, Owner};
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::log::Holding;
use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn team_root(cfg: &crate::config::Config) -> std::path::PathBuf {
    cfg.appliance.state_dir.join("team")
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            walk(&e.path(), out)?;
        } else if e.file_type()?.is_file() {
            out.push(e.path());
        }
    }
    Ok(())
}

/// Team volumes and held files, for the backup. A deleted workspace's leave
/// the backups as those backups age out, within the purge window.
pub fn pack_files(cfg: &crate::config::Config) -> Result<Vec<Value>> {
    pack(&team_root(cfg))
}

fn pack(root: &std::path::Path) -> Result<Vec<Value>> {
    let mut found = vec![];
    if root.is_dir() {
        walk(root, &mut found)?;
    }
    found
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(root)?.to_string_lossy().into_owned();
            Ok(json!({ "path": rel, "data_b64": B64.encode(std::fs::read(p)?) }))
        })
        .collect()
}

/// Each path was written by `pack_files`; one that climbs out is refused anyway.
pub fn unpack_files(cfg: &crate::config::Config, backup: &Value) -> Result<()> {
    unpack(&team_root(cfg), backup)
}

fn unpack(root: &std::path::Path, backup: &Value) -> Result<()> {
    for f in backup["team_files"].as_array().into_iter().flatten() {
        let rel = std::path::Path::new(f["path"].as_str().unwrap_or_default());
        anyhow::ensure!(
            !rel.as_os_str().is_empty() && rel.components().all(|c| matches!(c, std::path::Component::Normal(_))),
            "a team file in the backup has a bad path: {}",
            rel.display()
        );
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, B64.decode(f["data_b64"].as_str().unwrap_or_default())?)?;
    }
    Ok(())
}

/// How far back the audit of one workspace looks in the log of team events.
const AUDIT_CHANGES: i64 = 5000;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/workspaces/{ws}/local_ceiling", post(set_ceiling))
        .route("/v1/workspaces/{ws}/approvers", post(set_approvers))
        .route("/v1/workspaces/{ws}/audit", get(audit))
        .route("/v1/workspaces/{ws}/channels/{channel}/notes", get(notes))
}

#[derive(Deserialize)]
struct Ceiling {
    mode: crate::local::Mode,
}

async fn set_ceiling(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<Ceiling>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    ok_or_return!(send(&k, &p, Change::LocalCeiling { workspace: ws, mode: b.mode }).await);
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct Approvers {
    resource: String,
    people: Vec<String>,
}

async fn set_approvers(State(k): K, h: HeaderMap, Path(ws): Path<String>, Json(b): Json<Approvers>) -> Reply {
    let (p, _) = ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let members = k.read(|s| s.team.workspace(&ws).map(|w| w.members.clone())).unwrap_or_default();
    if b.people.iter().any(|a| members.get(a).is_none_or(|r| *r == Role::Guest)) {
        return err(StatusCode::BAD_REQUEST, "an approver is a member of this workspace, and not a guest");
    }
    ok_or_return!(send(&k, &p, Change::Approvers { workspace: ws, resource: b.resource, people: b.people }).await);
    Json(json!({ "ok": true })).into_response()
}

/// A thread's working notes, while its tasks last.
async fn notes(State(k): K, h: HeaderMap, Path((ws, channel)): Path<(String, String)>) -> Reply {
    ok_or_return!(member(&k, &h, &ws, Role::Guest).await);
    let view: Vec<Value> = k.read(|s| {
        s.team
            .runs_in(&ws, &channel)
            .into_iter()
            .filter(|r| s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id)).is_some_and(|run| !run.status.terminal()))
            .flat_map(|r| s.team.notes(&r.task).iter().map(|n| json!({ "task": r.task, "thread": r.thread, "text": n.text, "by": n.by, "at": n.at })))
            .collect()
    });
    Json(view).into_response()
}

/// Who asked, what acted and where, for every request here, and who changed
/// what in the workspace.
async fn audit(State(k): K, h: HeaderMap, Path(ws): Path<String>) -> Reply {
    ok_or_return!(member(&k, &h, &ws, Role::Admin).await);
    let runs: Vec<Value> = k.read(|s| {
        s.team
            .runs_of(&ws)
            .into_iter()
            .map(|r| {
                let run = s.tasks.get(&r.task).and_then(|t| s.runs.get(&t.run_id));
                let actions: Vec<Value> = run.into_iter().flat_map(|run| run.replies.iter().flat_map(|reply| reply.calls().into_iter().map(|(_, name, _)| json!(name)))).collect();
                json!({ "workspace": ws, "agent": if r.sub { "team sub-agent" } else { "team" }, "requester": r.requester, "task": r.task, "channel": r.channel, "actions": actions })
            })
            .collect()
    });
    let rows = match k.log.of_kinds(&["team"], AUDIT_CHANGES).await {
        Ok(rows) => rows,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    };
    let changes: Vec<Value> = rows
        .into_iter()
        .filter(|(_, _, _, b)| b["workspace"] == json!(ws))
        .map(|(seq, at, actor, b)| json!({ "seq": seq, "at": at, "actor": actor, "change": b["change"], "id": b["id"] }))
        .collect();
    Json(json!({ "runs": runs, "changes": changes })).into_response()
}

impl Kernel {
    /// Stops these tasks whoever is asking: a removal is what ends them,
    /// not the removed person's own say.
    pub(super) async fn end_team_tasks(&self, actor: &str, tasks: &[String]) {
        for task in tasks {
            let run = self.read(|s| s.tasks.get(task).and_then(|t| s.runs.get(&t.run_id)).filter(|r| !r.status.terminal()).map(|r| r.id.clone()));
            let Some(run) = run else { continue };
            match self.emit(actor, Ev::Cancelled { run: run.clone(), source: Some("team".into()), turn: None }).await {
                Ok(_) => self.on_stop(&run, true).await,
                Err(e) => tracing::warn!("{task}: {e:#}"),
            }
        }
    }

    pub(super) async fn erase(&self, actor: &str, item: String, holding: Holding) -> Result<()> {
        let seqs = self.log.content_seqs(&holding).await?;
        if seqs.is_empty() {
            return Ok(());
        }
        self.emit(actor, Ev::Removed { item, seqs: seqs.clone() }).await?;
        self.log.delete_content(&seqs).await?;
        if let Err(e) = self.scrub_backups().await {
            tracing::error!("taking removed team content out of the backups: {e:#}");
        }
        Ok(())
    }

    pub(super) async fn erase_entry(&self, actor: &str, id: &str) -> Result<()> {
        let holding = Holding { matches: vec![("team", "id", id.to_string())], ..Default::default() };
        self.erase(actor, format!("entry:{id}"), holding).await
    }

    /// Everything of a workspace: its work, its computers, its volume and
    /// what the log and the backups hold of it.
    pub(super) async fn delete_workspace(&self, p: &Person, ws: &str) -> Result<()> {
        let actor = format!("person:{}", p.id);
        let tasks: Vec<String> = self.read(|s| s.team.runs_of(ws).into_iter().map(|r| r.task.clone()).collect());
        self.end_team_tasks(&actor, &tasks).await;
        let owner = Owner::Workspace(ws.to_string());
        let computers: Vec<String> = self.read(|s| s.computers.all().filter(|c| c.owner == owner).map(|c| c.id.clone()).collect());
        for id in computers {
            if let Err(e) = self.kill_computer(p, &id, KillMode::Destroy, true).await {
                tracing::warn!("workspace {ws}: computer {id}: {e:?}");
            }
        }
        let runs: Vec<String> = self.read(|s| tasks.iter().filter_map(|t| s.tasks.get(t).map(|t| t.run_id.clone())).collect());
        let dir = volume_dir(self, ws);
        let holding = Holding { runs, kinds: vec!["team"], field: "workspace", id: ws.to_string(), ..Default::default() };
        self.emit(&actor, Ev::Team(Change::Deleted { workspace: ws.to_string() })).await?;
        if let Some(root) = dir.parent() {
            if let Err(e) = tokio::fs::remove_dir_all(root).await {
                tracing::debug!("{}: {e}", root.display());
            }
        }
        self.erase(&actor, format!("workspace:{ws}"), holding).await
    }

    pub(super) async fn remove_held(&self, ws: &str, person: &str) {
        let dir = held_dir(self, ws, person);
        if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
            tracing::debug!("{}: {e}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_files_come_back_from_a_backup() {
        let (from, to) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir_all(from.path().join("w/files")).unwrap();
        std::fs::write(from.path().join("w/files/a.txt"), b"hello").unwrap();
        let packed = pack(from.path()).unwrap();
        assert_eq!(packed.len(), 1);
        unpack(to.path(), &json!({ "team_files": packed })).unwrap();
        assert_eq!(std::fs::read(to.path().join("w/files/a.txt")).unwrap(), b"hello");
    }

    #[test]
    fn a_backup_cannot_write_outside_the_team_directory() {
        let to = tempfile::tempdir().unwrap();
        let bad = json!({ "team_files": [{ "path": "../../escape", "data_b64": "aGk=" }] });
        assert!(unpack(&to.path().join("team"), &bad).is_err());
    }
}
