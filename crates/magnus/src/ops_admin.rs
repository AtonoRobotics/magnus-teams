//! The appliance's operations as an admin runs them (admin console spec,
//! Operations), as the kernel's own operations: the run modules (what each
//! provides, on or off for the organisation), the model providers (enabled,
//! where they answer, the models they offer, a connection test, the last
//! error), the updates (this version, the one available with its notes,
//! install and roll back, the desktop app's channels), and what the kill
//! switch holds (the microVMs it froze).

use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use serde_json::{json, Map, Value};

/// How long the provider list waits for each adapter's models.
const OVERVIEW_CATALOG_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// How long one adapter's models are waited for when it is tested or its model made the default.
const CATALOG_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

pub type Refusal = (StatusCode, String);

fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("operations: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

/// A provider's settings as anyone may see them: what names a secret is
/// left out (the provider's keys are write-only, here as everywhere).
pub fn shown_settings(s: &Map<String, Value>) -> Map<String, Value> {
    s.iter().filter(|(k, _)| !["key", "secret", "token", "password", "credential"].iter().any(|w| k.to_lowercase().contains(w))).map(|(k, v)| (k.clone(), v.clone())).collect()
}

impl Kernel {
    /// The modules a run on `adapter`/`model` starts with: the config's,
    /// with what an admin set for the organisation on top.
    pub fn modules_for(&self, adapter: &str, model: &str) -> (std::collections::BTreeMap<String, bool>, Option<String>) {
        let (mut on, to) = self.cfg.modules_for(adapter, model);
        for (name, set) in self.read(|s| s.module_settings.clone()) {
            on.insert(name, set);
        }
        (on, to)
    }

    /// Every run module: what it provides, whether it is on for runs on the
    /// default model, whether an admin set it, and how often it acted.
    pub fn admin_modules(&self) -> Vec<Value> {
        let (a, m) = self.default_model();
        let (on, to) = self.modules_for(&a, &m);
        let set = self.read(|s| s.module_settings.clone());
        let acted = self.read(|s| {
            let mut n: std::collections::HashMap<&str, usize> = Default::default();
            for r in s.runs.values() {
                *n.entry("lost_reset").or_default() += r.cuts.len();
                *n.entry("escalation").or_default() += r.escalated.is_some() as usize;
            }
            n.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<std::collections::HashMap<String, usize>>()
        });
        crate::dev::MODULES
            .iter()
            .map(|md| {
                json!({
                    "name": md.name, "version": crate::update::version(), "provides": md.compensates,
                    "on": on.get(md.name).copied().unwrap_or(false), "set_by_admin": set.contains_key(md.name),
                    "escalate_to": (md.name == "escalation").then(|| to.clone()).flatten(),
                    "acted": acted.get(md.name).copied().unwrap_or(0), "ablation": md.ablation_suite,
                })
            })
            .collect()
    }

    /// Turn a module on or off for every run that starts from now (None:
    /// back to the config's). Runs already going keep theirs (K9).
    pub async fn set_module(&self, actor: &str, name: &str, on: Option<bool>) -> Result<Value, Refusal> {
        if crate::dev::MODULES.iter().all(|m| m.name != name) {
            return Err((StatusCode::NOT_FOUND, "no such module".into()));
        }
        self.emit(actor, Ev::ModuleSet { name: name.into(), on }).await.map_err(failed)?;
        Ok(json!({ "name": name, "on": on }))
    }

    /// Every model provider (an adapter): whether it is enabled, where it
    /// answers, the models it offers now, its last conformance, and the
    /// last error a run met on it. Its credentials never appear.
    pub async fn admin_providers(&self) -> Vec<Value> {
        let (da, dm) = self.default_model();
        let disabled = self.read(|s| s.providers_off.clone());
        let mut out = vec![];
        for (id, command, running) in self.adapters.list().await {
            let models = match tokio::time::timeout(OVERVIEW_CATALOG_WAIT, self.adapters.catalog(&id)).await {
                Ok(Ok(magnus_adapter_proto::Event::Catalog { models })) => Ok(models.into_iter().map(|m| m.id).collect::<Vec<_>>()),
                Ok(Ok(magnus_adapter_proto::Event::Error { message, .. })) => Err(message),
                Ok(Ok(other)) => Err(format!("unexpected answer: {other:?}")),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(_) => Err("it did not answer in 5 seconds".into()),
            };
            let settings = self.cfg.adapters.iter().find(|a| a.id == id).map(|a| shown_settings(&a.settings)).unwrap_or_default();
            let endpoint = ["base_url", "endpoint", "url", "host"].iter().find_map(|k| settings.get(*k).and_then(|v| v.as_str()).map(String::from));
            let (conformance, last_error) = self.read(|s| {
                let c = s.conformance.get(&id).map(|(v, m, ok, _)| json!({ "version": v, "model": m, "passed": ok }));
                let mut failed: Vec<&crate::state::Run> =
                    s.runs.values().filter(|r| r.config.adapter == id && r.status == crate::state::Status::Failed && r.reason.is_some()).collect();
                failed.sort_by(|a, b| b.ended_at.cmp(&a.ended_at));
                (c, failed.first().map(|r| json!({ "at": r.ended_at, "error": r.reason })))
            });
            out.push(json!({
                "id": id, "program": command.first().and_then(|c| std::path::Path::new(c).file_name()).map(|f| f.to_string_lossy().to_string()),
                "running": running.map(|(a, v)| json!({ "adapter": a, "version": v })),
                "enabled": !disabled.contains(&id), "default": da == id, "default_model": (da == id).then(|| dm.clone()),
                "endpoint": endpoint, "settings": settings,
                "answers": models.is_ok(), "models": models.as_ref().ok(), "error": models.as_ref().err(),
                "conformance": conformance, "last_error": last_error,
            }));
        }
        out
    }

    /// A provider's connection test: it is asked for its models, and how
    /// long that took is said.
    pub async fn test_provider(&self, id: &str) -> Result<Value, Refusal> {
        if self.adapters.list().await.iter().all(|(a, ..)| a != id) {
            return Err((StatusCode::NOT_FOUND, "no such provider".into()));
        }
        let t = std::time::Instant::now();
        let r = tokio::time::timeout(CATALOG_WAIT, self.adapters.catalog(id)).await;
        let ms = t.elapsed().as_millis() as u64;
        Ok(match r {
            Ok(Ok(magnus_adapter_proto::Event::Catalog { models })) => json!({ "ok": true, "ms": ms, "models": models.into_iter().map(|m| m.id).collect::<Vec<_>>() }),
            Ok(Ok(magnus_adapter_proto::Event::Error { message, .. })) => json!({ "ok": false, "ms": ms, "error": message }),
            Ok(Ok(other)) => json!({ "ok": false, "ms": ms, "error": format!("unexpected answer: {other:?}") }),
            Ok(Err(e)) => json!({ "ok": false, "ms": ms, "error": format!("{e:#}") }),
            Err(_) => json!({ "ok": false, "ms": ms, "error": "it did not answer in 15 seconds" }),
        })
    }

    /// Enable or disable a provider, or make one of its models the one new
    /// tasks run on. The default model's provider stays enabled.
    pub async fn set_provider(&self, actor: &str, id: &str, enabled: Option<bool>, default_model: Option<&str>) -> Result<Value, Refusal> {
        if self.adapters.list().await.iter().all(|(a, ..)| a != id) {
            return Err((StatusCode::NOT_FOUND, "no such provider".into()));
        }
        let (da, _) = self.default_model();
        if enabled == Some(false) && da == id && default_model.is_none() {
            return Err((StatusCode::CONFLICT, "new tasks run on this provider's model: choose another default first".into()));
        }
        let mut evs = vec![];
        if let Some(on) = enabled {
            evs.push((actor, Ev::ProviderSet { id: id.into(), enabled: on }));
        }
        if let Some(m) = default_model {
            if enabled == Some(false) || (enabled.is_none() && self.read(|s| s.providers_off.contains(id))) {
                return Err((StatusCode::CONFLICT, "a disabled provider's model cannot be the default".into()));
            }
            let offered = match tokio::time::timeout(CATALOG_WAIT, self.adapters.catalog(id)).await {
                Ok(Ok(magnus_adapter_proto::Event::Catalog { models })) => models.iter().any(|x| x.id == m),
                _ => false,
            };
            if !offered {
                return Err((StatusCode::BAD_REQUEST, format!("{id} does not offer {m} now")));
            }
            evs.push((actor, Ev::ModelSet { adapter: id.into(), model: m.into() }));
        }
        if evs.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "nothing to change".into()));
        }
        self.emit_all(evs).await.map_err(failed)?;
        let (a, m) = self.default_model();
        Ok(json!({ "id": id, "enabled": !self.read(|s| s.providers_off.contains(id)), "default": { "adapter": a, "model": m } }))
    }

    /// The updates: this version, the latest release with its notes, what
    /// is going on now and what happened last, the release kept to go back
    /// to, and the desktop app's channels with each one's published version.
    pub fn admin_updates(&self) -> Value {
        let latest = self.latest_release().map(|m| json!({ "version": m["version"], "notes": m["notes"] }));
        let prev = crate::update::previous_version();
        let (updating, last, skipped, channel) = self.read(|s| (s.updating.clone(), s.last_update.clone(), s.skipped_releases.clone(), s.desktop_channel.clone()));
        let channels: Vec<Value> = crate::update::CHANNELS
            .iter()
            .map(|c| {
                let published = crate::update::desktop_manifest(&self.cfg, "", c).and_then(|m| m["version"].as_str().map(String::from));
                json!({ "channel": c, "published": published, "who": match *c { "development" => "the owner", "test" => "testers", _ => "everyone else" } })
            })
            .collect();
        json!({
            "version": crate::update::version(), "automatic": self.cfg.updates.automatic, "latest": latest,
            "available": latest.as_ref().and_then(|l| l["version"].as_str()).is_some_and(|v| crate::update::newer(v, &crate::update::version()) && !skipped.contains(v)),
            "updating": updating.map(|(from, to, rollback)| json!({ "from": from, "to": to, "rollback": rollback })),
            "last": last, "skipped": skipped, "previous": prev,
            "desktop": { "channels": channels, "everyone": channel.unwrap_or_else(|| "production".into()) },
            "server": crate::admin_app::server_channel::shown(self),
        })
    }

    /// The channel the desktop apps of everyone but the owner and testers
    /// follow.
    pub async fn set_desktop_channel(&self, actor: &str, channel: &str) -> Result<Value, Refusal> {
        if !crate::update::CHANNELS.contains(&channel) {
            return Err((StatusCode::BAD_REQUEST, format!("a channel is one of {}", crate::update::CHANNELS.join(", "))));
        }
        self.emit(actor, Ev::DesktopChannelSet { channel: channel.into() }).await.map_err(failed)?;
        Ok(json!({ "everyone": channel }))
    }
}
