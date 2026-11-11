//! How long things are kept, as an admin sets it (spec rev 141, Security:
//! "how long chats, files and logs are kept"). Two windows are the admin's:
//! how long a deleted chat, file or memory waits in the trash, restorable,
//! before it is removed, and how long each backup is kept. Both are bounded
//! by the purge window: whatever an admin sets, a deletion still reaches the
//! backups within `PURGE_WINDOW_DAYS` of leaving the trash, and nothing is
//! kept in the trash longer than that. The event log is the permanent record
//! (invariant: it is never edited); a person's content in it goes when it is
//! deleted, which these windows time.
//!
//! The windows are enforced by the sweeps that already run: the trash's
//! hourly purge and the backup round's pruning read the admin's setting each
//! time, and setting a shorter one sweeps at once. The setting is an
//! `admin_setting` event, taken through the step-up and audited first.

use super::mount::{audited, Audit};
use super::people::refuse;
use super::security::PURGE_WINDOW_DAYS;
use super::settings::Change;
use super::Admin;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::put;
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

/// The shortest either window may be: a day, so a deletion made by mistake
/// can still be undone, and a backup made is kept until the next.
pub const RETENTION_DAYS_MIN: u32 = 1;
/// The longest a deleted item may wait in the trash.
pub const TRASH_DAYS_MAX: u32 = PURGE_WINDOW_DAYS;
/// The longest a backup may be kept: a backup older than the purge window
/// would hold content deleted more than that window ago.
pub const BACKUP_DAYS_MAX: u32 = PURGE_WINDOW_DAYS;
// Checked when built: no window outlasts the purge window, none is zero.
const _: () = assert!(TRASH_DAYS_MAX <= PURGE_WINDOW_DAYS && BACKUP_DAYS_MAX <= PURGE_WINDOW_DAYS && RETENTION_DAYS_MIN >= 1);

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/retention", put(set))
}

impl Kernel {
    /// How long a deleted item waits in the trash: the admin's setting, else
    /// the config's.
    pub fn trash_keep_days(&self) -> u32 {
        self.read(|s| s.admin_settings.retention.map(|r| r.0)).unwrap_or(self.cfg.trash.keep_days)
    }

    /// How long each backup is kept: the admin's setting, else the config's.
    pub fn backup_keep_days(&self) -> u32 {
        self.read(|s| s.admin_settings.retention.map(|r| r.1)).unwrap_or(self.cfg.backup.keep_days)
    }

    /// Remove what the windows no longer keep: the trash past its window,
    /// and backups past theirs.
    pub async fn sweep_retention(&self) -> anyhow::Result<()> {
        self.purge_trash().await?;
        crate::backup::prune(&self.cfg.backup_dir(), self.backup_keep_days())?;
        Ok(())
    }
}

/// The windows and their bounds, for Security.
pub fn shown(k: &Kernel) -> Value {
    json!({
        "trash_days": k.trash_keep_days(), "backup_days": k.backup_keep_days(),
        "set_here": k.read(|s| s.admin_settings.retention.is_some()),
        "min_days": RETENTION_DAYS_MIN, "trash_days_max": TRASH_DAYS_MAX, "backup_days_max": BACKUP_DAYS_MAX,
    })
}

/// A window from the request, within its bounds.
fn days(b: &Value, field: &str, max: u32) -> Result<u32, String> {
    match b[field].as_u64() {
        Some(d) if (RETENTION_DAYS_MIN as u64..=max as u64).contains(&d) => Ok(d as u32),
        _ => Err(format!("{field} is a whole number of days from {RETENTION_DAYS_MIN} to {max}.")),
    }
}

async fn set(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let (trash_days, backup_days) = match (days(&b, "trash_days", TRASH_DAYS_MAX), days(&b, "backup_days", BACKUP_DAYS_MAX)) {
        (Ok(t), Ok(d)) => (t, d),
        (Err(m), _) | (_, Err(m)) => return refuse(StatusCode::BAD_REQUEST, "invalid", &m),
    };
    let audit = Audit {
        action: "retention_set".into(),
        target: Some("retention".into()),
        before: Some(json!({ "trash_days": k.trash_keep_days(), "backup_days": k.backup_keep_days() })),
        after: Some(json!({ "trash_days": trash_days, "backup_days": backup_days })),
    };
    let change = async {
        if let Err(e) = k.emit(&a.actor(), Ev::AdminSetting(Change::Retention { trash_days, backup_days })).await {
            return refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}"));
        }
        // A shorter window takes effect now, not at the next round.
        if let Err(e) = k.sweep_retention().await {
            tracing::error!("sweeping after a retention change: {e:#}");
        }
        Json(shown(&k)).into_response()
    };
    audited(&k, &a, audit, change).await
}
