//! The server's own update channel (spec rev 141, Operations: "Updates
//! (channel, install, roll back)"). The desktop apps had channels; the server
//! followed production only. An admin now picks the channel the server
//! updates from, among the same three: production (`appliance/`), test
//! (`appliance/test/`) or development (`appliance/development/`), as
//! `release/publish.sh appliance` writes them with `MAGNUS_CHANNEL`. A
//! channel with no release of its own follows production's, as the desktop
//! apps' do. The setting is an `admin_setting` event, taken through the
//! step-up and written to the audit before it is made.

use super::mount::{audited, Audit};
use super::people::refuse;
use super::settings::Change;
use super::Admin;
use crate::config::Config;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::update::CHANNELS;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::put;
use axum::{Extension, Json, Router};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// The channel a server follows until an admin picks another.
pub const DEFAULT_SERVER_CHANNEL: &str = "production";

pub(super) fn routes() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/admin/updates/server-channel", put(set))
}

/// Where `channel`'s server releases are published.
pub fn channel_dir(cfg: &Config, channel: &str) -> PathBuf {
    let appliance = cfg.updates_dir().join("appliance");
    if channel == DEFAULT_SERVER_CHANNEL {
        appliance
    } else {
        appliance.join(channel)
    }
}

fn release_in(cfg: &Config, channel: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(channel_dir(cfg, channel).join("latest.json")).ok()?).ok()
}

/// The channel the server follows, as an admin set it.
pub fn chosen(k: &Kernel) -> &'static str {
    k.read(|s| s.admin_settings.server_channel.clone()).and_then(|c| CHANNELS.iter().copied().find(|x| *x == c)).unwrap_or(DEFAULT_SERVER_CHANNEL)
}

/// Where the server takes its next release from: its channel's directory,
/// or production's while the channel has published nothing.
pub fn release_dir(k: &Kernel) -> PathBuf {
    let c = chosen(k);
    if release_in(&k.cfg, c).is_some() {
        channel_dir(&k.cfg, c)
    } else {
        channel_dir(&k.cfg, DEFAULT_SERVER_CHANNEL)
    }
}

/// The channel, and each channel's latest release, for Operations › Updates.
pub fn shown(k: &Kernel) -> Value {
    let channels: Vec<Value> = CHANNELS.iter().map(|c| json!({ "channel": c, "published": release_in(&k.cfg, c).and_then(|m| m["version"].as_str().map(String::from)) })).collect();
    json!({ "channel": chosen(k), "channels": channels })
}

async fn set(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Json(b): Json<Value>) -> Response {
    let Some(channel) = b["channel"].as_str().and_then(|c| CHANNELS.iter().copied().find(|x| *x == c)) else {
        return refuse(StatusCode::BAD_REQUEST, "invalid", &format!("A channel is one of {}.", CHANNELS.join(", ")));
    };
    let audit = Audit {
        action: "server_channel_set".into(),
        target: Some("updates/server-channel".into()),
        before: Some(json!({ "channel": chosen(&k) })),
        after: Some(json!({ "channel": channel })),
    };
    let change = async {
        match k.emit(&a.actor(), Ev::AdminSetting(Change::ServerChannel { channel: channel.into() })).await {
            Ok(_) => Json(json!({ "channel": channel })).into_response(),
            Err(e) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}")),
        }
    };
    audited(&k, &a, audit, change).await
}
