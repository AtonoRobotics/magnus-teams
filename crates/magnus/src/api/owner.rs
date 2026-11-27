use super::{err, internal, who, K};
use crate::api_error::{refuse, ApiResult};
use crate::config::Adapter;
use crate::extract::{Body, Caller, Owner};
use crate::kernel::Kernel;
use crate::policy::layers::Layer;
use crate::state::Status;
use crate::{api_bodies as req, events::Ev, people::rights};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) async fn policies(State(k): K, Caller(p): Caller) -> ApiResult {
    let all = k.read(|s| s.org.policies().clone());
    // The layers of their own tasks, which they may set (`put_policy`).
    let own_task = |t: &str| k.read(|s| s.tasks.get(t).is_some_and(|t| t.person == p.id));
    let mine: serde_json::Map<String, Value> =
        all.into_iter().filter(|(l, _)| Layer::parse(l).is_some_and(|l| l.shown_to(&p, own_task))).map(|(l, c)| (l, Value::String(c))).collect();
    Ok(Json(json!({ "layers": mine, "default_org": crate::policy::DEFAULT_ORG })).into_response())
}

pub(super) async fn put_policy(State(k): K, Caller(p): Caller, Path(layer): Path<String>, Body(b): Body<req::Policy>) -> ApiResult {
    let layer = layer.trim_matches('/').to_string();
    // A task's layer: the task's own, which only narrows, so its person
    // may set it as well as the owner (DECISIONS, Layering).
    let parsed = Layer::parse(&layer);
    let task_of = if let Some(Layer::Task(t)) = parsed { k.read(|s| s.tasks.get(t).map(|t| t.person.clone())) } else { None };
    let valid = match parsed {
        Some(Layer::User(u)) => k.people().exists(u),
        Some(Layer::Task(_)) => task_of.is_some(),
        other => other.is_some(),
    };
    if !valid {
        return refuse(StatusCode::NOT_FOUND, "no such policy layer");
    }
    if !(rights(&p).may_write_policy_layer(&layer) || task_of.as_deref() == Some(p.id.as_str())) {
        return refuse(StatusCode::FORBIDDEN, "you may not set this layer's policy");
    }
    let cedar = b.cedar;
    if let Err(e) = crate::policy::validate(&cedar) {
        return refuse(StatusCode::BAD_REQUEST, format!("the policy does not parse: {e}"));
    }
    if let Err(why) = crate::policy::layers::tightens_only(&layer, &cedar) {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, why);
    }
    Ok(match k.emit(&format!("person:{}", p.id), Ev::PolicySet { layer, cedar }).await {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => internal(e),
    })
}

pub(super) async fn secret_names(State(k): K, _: Owner) -> ApiResult {
    Ok(match k.vault.names() {
        Ok(n) => Json(json!({ "names": crate::secret_scope::server_names(&k, n) })).into_response(),
        Err(e) => internal(e),
    })
}

pub(super) async fn put_secret(State(k): K, Owner(p): Owner, Path(name): Path<String>, Body(b): Body<req::Secret>) -> ApiResult {
    let Some(value) = b.value.as_deref() else { return refuse(StatusCode::BAD_REQUEST, "no value") };
    if crate::secret_scope::personal(&k, &name) {
        return refuse(StatusCode::FORBIDDEN, crate::secret_scope::REFUSED);
    }
    if k.vault.is_sealed() {
        return refuse(StatusCode::CONFLICT, "the kill switch is engaged; turn it off before entering secrets");
    }
    k.vault.put(&name, value)?;
    Ok(match k.emit(&format!("person:{}", p.id), Ev::SecretSet { name }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    })
}

/// The backups on disk and whether the latest one restored (K10).
pub(super) async fn backups(State(k): K, _: Owner) -> ApiResult {
    Ok(Json(crate::backup::status(&k)).into_response())
}

pub(super) async fn back_up(State(k): K, Owner(p): Owner) -> ApiResult {
    Ok(match k.back_up(&format!("person:{}", p.id)).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(e) => internal(e),
    })
}

/// The kill switch and the secrets it took out (owner only): what to sign
/// in to or enter again, and whether each provider revoked it.
pub(super) async fn kill_switch_state(State(k): K, _: Owner) -> ApiResult {
    let (engaged, revoked, sealed) = k.read(|s| (s.vault.kill_switch(), s.vault.revoked().clone(), s.vault.sealed().to_vec()));
    Ok(Json(json!({ "engaged": engaged, "revoked": revoked, "sealed": if engaged { sealed } else { vec![] } })).into_response())
}

impl Kernel {
    /// The one kill switch (B11; owner's ruling 10): the app's and the admin
    /// console's are this. Engaged, it seals the vault, revokes every
    /// sign-in at its provider and suspends every running run; released,
    /// it tells each person what it stopped, and resumes nothing (K53).
    /// `actor` is who did it, as the log names them. The answer, or the
    /// status and body of a failure.
    pub async fn kill_switch(self: &Arc<Self>, actor: &str, engaged: bool) -> Result<Value, (StatusCode, Value)> {
        let k = self.clone();
        let actor = actor.to_string();
        // One at a time: a release racing an engage could unseal before the
        // engage sealed, or seal after the release.
        static SWITCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one = SWITCHING.lock().await;
        if !engaged {
            // Released: the vault takes secrets again, but holds none; nothing
            // resumes by itself (K53). Each person's tasks are their own: each
            // is told which of theirs the switch stopped, and continues them,
            // in the same write as the release, so none is released untold.
            let stopped: Vec<crate::state::Run> = k.read(|s| crate::halts::held_by_server(s).cloned().collect());
            let why = json!({ "reason": "kill_switch", "evidence": { "why": "the owner engaged the kill switch, and has released it; Continue goes on from where it stopped" } });
            let mut evs = vec![(actor.as_str(), Ev::KillSwitch { engaged })];
            for r in &stopped {
                // Background work is not the person's to continue: it ends,
                // and the next wake starts fresh (owner, 2026-11-07).
                if k.run_kind(&r.id).is_background() {
                    evs.push(("kernel", Ev::Failed { run: r.id.clone(), reason: "background work stopped by the kill switch".into(), by: "kernel".into() }));
                    continue;
                }
                let title = k.title_of(r).await;
                evs.push(("kernel", k.notice_of(r, format!("Stopped by the kill switch: {title}"), why.clone())));
            }
            if let Err(e) = k.emit_all(evs).await {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") })));
            }
            k.vault.unseal();
            k.release_computers(&actor, crate::halts::SERVER_REASON).await;
            for r in stopped.iter().filter(|r| k.run_kind(&r.id).is_background()) {
                k.on_stop(&r.id, true).await;
            }
            return Ok(json!({ "engaged": false, "told": stopped.iter().filter(|r| !k.run_kind(&r.id).is_background()).count() }));
        }
        // One control (B11). The vault is sealed first, and each OAuth sign-in
        // taken out of it and revoked at its provider, whatever happens to the
        // records after: the switch must work when the log cannot be written.
        // API keys and app secrets stay sealed: only their provider's console
        // can revoke them.
        k.vault.seal();
        // Every brokered connector's microVM, with whatever call it was making.
        k.brokered.stop(None, None).await;
        let names = match k.vault.names() {
            Ok(n) => n,
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") }))),
        };
        let mut revoking = vec![];
        // Every person's own sign-in to every app people sign in to, turned on
        // or off: an app turned off keeps its sign-ins.
        let signed_in: Vec<String> = k.all_connectors().into_iter().map(|(c, _)| c).filter(|c| c.auth.as_deref() == Some(crate::connectors::OAUTH)).map(|c| c.name).collect();
        for n in &names {
            if let Some(adapter) = k.adapters.oauth_owner_of(n).await {
                revoking.push((n.clone(), adapter));
            } else if let Some(app) = signed_in.iter().find(|app| crate::connectors::is_sign_in_of(n, app)) {
                revoking.push((n.clone(), format!("connector:{app}")));
            }
        }
        let take: Vec<String> = revoking.iter().map(|(n, _)| n.clone()).collect();
        // With any an earlier switch took and a crash kept from revoking.
        let taken = match k.vault.seal_and_take(&take) {
            Ok(t) => t,
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": format!("{e:#}") }))),
        };
        let take: Vec<String> = taken.iter().map(|(n, _)| n.clone()).collect();
        // Revoked at once; each result is recorded only after the vault's
        // sealing below (or its failure), which starts the list it goes in: a
        // result recorded first was wiped, and the owner told it was not done.
        let (sealed_tx, sealed_rx) = tokio::sync::oneshot::channel::<()>();
        k.revoke_taken(taken, revoking, Some(sealed_rx));
        // Then the records: the policy is deny-all from this event on, and every
        // run is suspended.
        let kept: Vec<String> = names.iter().filter(|n| !take.contains(n)).cloned().collect();
        let recorded = async {
            k.emit(&actor, Ev::KillSwitch { engaged }).await?;
            let active: Vec<String> = k.read(|s| s.runs.values().filter(|r| r.status == Status::Running).map(|r| r.id.clone()).collect());
            for run in active {
                k.suspend(&run, "kill_switch", None).await?;
            }
            k.emit(&actor, Ev::VaultSealed { revoking: take.clone(), kept: kept.clone() }).await?;
            anyhow::Ok(())
        };
        let recorded = recorded.await;
        let _ = sealed_tx.send(());
        // Every run's pages in the browser, the running ones' and those already
        // waiting (a suspended run keeps its context otherwise), whether or not
        // the records were written. After the suspensions, so no run opens a
        // page after it; a run continued later starts on a blank one.
        k.stop_work(&actor, crate::halts::SERVER_REASON, &[], crate::computer::Reach::Everyone).await;
        if let Err(e) = recorded {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": format!("The vault is sealed and the sign-ins are being revoked, but recording the kill switch failed: {e:#}. Engage it again once the server can write."), "sealed": true }),
            ));
        }
        Ok(json!({ "engaged": true, "revoking": take, "sealed": kept }))
    }

    /// Revoke at their providers the sign-ins the kill switch took out of
    /// the vault, each let go by the vault once its provider has answered,
    /// and record each result once `after` says the sealing is recorded.
    fn revoke_taken(self: &Arc<Self>, taken: Vec<(String, crate::vault::Secret)>, known: Vec<(String, String)>, after: Option<tokio::sync::oneshot::Receiver<()>>) {
        let k = self.clone();
        tokio::spawn(async move {
            let mut done = vec![];
            for (name, secret) in taken {
                let adapter = match known.iter().find(|(n, _)| *n == name) {
                    Some((_, a)) => a.clone(),
                    None => k.revoker_of(&name).await,
                };
                let (at_provider, detail) = if adapter.starts_with("connector:") {
                    crate::connectors::revoke(&k, secret.expose()).await
                } else {
                    k.adapters.revoke(&adapter, secret.expose().to_string()).await
                };
                if let Err(e) = k.vault.revoked(&name) {
                    tracing::error!("could not let go of {name} after revoking it: {e:#}");
                }
                done.push(Ev::SecretRevoked { name, adapter, at_provider, detail });
            }
            if let Some(after) = after {
                let _ = after.await;
            }
            for ev in done {
                if let Err(e) = k.emit("kernel", ev).await {
                    tracing::error!("could not record a revocation: {e:#}");
                }
            }
        });
    }

    /// Who revokes a sign-in the switch took: its adapter, or the app it
    /// signs in to (`connector:<app>`), even one removed since.
    async fn revoker_of(&self, name: &str) -> String {
        if let Some(adapter) = self.adapters.oauth_owner_of(name).await {
            return adapter;
        }
        let app = self.all_connectors().into_iter().map(|(c, _)| c.name).find(|app| crate::connectors::is_sign_in_of(name, app));
        format!("connector:{}", app.unwrap_or_default())
    }

    /// At start: a kill switch a crash cut short is finished. Sealed but not
    /// recorded, it is engaged again (which records it and revokes what it
    /// took); otherwise what it took and did not see revoked is revoked,
    /// released since or not.
    pub async fn finish_kill_switch(self: &Arc<Self>) {
        if self.vault.is_sealed() && !self.read(|s| s.vault.kill_switch()) {
            tracing::warn!("the vault is sealed but the kill switch is not recorded: engaging it again");
            if let Err((_, e)) = self.kill_switch("kernel", true).await {
                tracing::error!("could not engage the kill switch again: {e}");
            }
            return;
        }
        match self.vault.revoking() {
            Ok(taken) if !taken.is_empty() => self.revoke_taken(taken, vec![], None),
            Ok(_) => {}
            Err(e) => tracing::error!("could not read the sign-ins waiting to be revoked: {e:#}"),
        }
    }
}

pub(super) async fn kill_switch(State(k): K, Owner(p): Owner, Body(b): Body<req::KillSwitch>) -> ApiResult {
    Ok(match k.kill_switch(&format!("person:{}", p.id), b.engaged.unwrap_or(true)).await {
        Ok(v) => Json(v).into_response(),
        Err((st, v)) => (st, Json(v)).into_response(),
    })
}

pub(super) async fn adapters(State(k): K, _: Owner) -> ApiResult {
    let list: Vec<Value> = k
        .adapters
        .list()
        .await
        .into_iter()
        .map(|(id, cmd, running)| {
            let verified = k.read(|s| s.org.conformance().get(&id).cloned());
            json!({
                "id": id, "command": cmd,
                "running": running.map(|(a, v)| json!({ "adapter": a, "version": v })),
                "conformance": verified.map(|(v, m, ok, _)| json!({ "version": v, "model": m, "passed": ok })),
            })
        })
        .collect();
    Ok(Json(json!({ "adapters": list })).into_response())
}

/// Replace or add an adapter while the kernel runs. The candidate must pass
/// the conformance suite against the live provider before it takes requests,
/// unless the owner explicitly skips the gate.
pub(super) async fn put_adapter(State(k): K, Owner(p): Owner, Path(id): Path<String>, Body(b): Body<req::PutAdapter>) -> ApiResult {
    let cfg: Adapter = match serde_json::from_value(json!({ "id": id, "command": b.command, "settings": b.settings })) {
        Ok(c) => c,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, format!("bad adapter: {e}")),
    };
    let candidate = match k.adapters.launch(&cfg).await {
        Ok(r) => r,
        Err(e) => return refuse(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    };
    let info = json!({ "adapter": candidate.adapter, "version": candidate.version });
    if b.skip_conformance != Some(true) {
        let model = match conformance_model(&k, &id, &b.model).await {
            Ok(m) => m,
            Err(m) => return refuse(StatusCode::BAD_GATEWAY, m),
        };
        let report = k.adapters.conformance(&candidate, &model).await;
        let rv = serde_json::to_value(&report).unwrap_or_default();
        let ev = Ev::AdapterVerified(crate::events::AdapterVerifiedPayload {
            adapter_id: id.clone(),
            adapter: report.adapter.clone(),
            version: report.version.clone(),
            model,
            passed: report.passed,
            report: rv.clone(),
        });
        k.emit(&format!("person:{}", p.id), ev).await?;
        if !report.passed {
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "installed": false, "conformance": rv }))).into_response());
        }
    }
    k.adapters.install(candidate).await;
    Ok(Json(json!({ "installed": true, "running": info })).into_response())
}

/// The model a conformance run uses when none is named: the appliance's model
/// if this adapter serves it, otherwise the first model the adapter lists
/// (a fixed version first). Another provider's model would fail every check.
async fn conformance_model(k: &Kernel, id: &str, asked: &Value) -> Result<String, String> {
    if let Some(m) = asked.as_str() {
        return Ok(m.to_string());
    }
    let (adapter, model) = k.read(|s| s.org.model().clone()).unwrap_or_else(|| (k.cfg.model.adapter.clone(), k.cfg.model.name.clone()));
    if adapter == id {
        return Ok(model);
    }
    match k.adapters.catalog(id).await {
        Ok(magnus_adapter_proto::Event::Catalog { models }) => {
            models.iter().find(|m| m.fixed_version).or(models.first()).map(|m| m.id.clone()).ok_or_else(|| format!("adapter {id} lists no models; name one to test"))
        }
        Ok(magnus_adapter_proto::Event::Error { message, .. }) => Err(format!("could not list adapter {id}'s models: {message}")),
        Ok(other) => Err(format!("unexpected answer to the model list: {other:?}")),
        Err(e) => Err(format!("{e:#}")),
    }
}

pub(super) async fn conformance(State(k): K, Owner(p): Owner, Path(id): Path<String>, Body(b): Body<req::Conformance>) -> ApiResult {
    let running = match k.adapters.get(&id).await {
        Ok(r) => r,
        Err(e) => return refuse(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    };
    let model = match conformance_model(&k, &id, &b.model).await {
        Ok(m) => m,
        Err(m) => return refuse(StatusCode::BAD_GATEWAY, m),
    };
    let report = k.adapters.conformance(&running, &model).await;
    let rv = serde_json::to_value(&report).unwrap_or_default();
    let ev = Ev::AdapterVerified(crate::events::AdapterVerifiedPayload {
        adapter_id: id,
        adapter: report.adapter.clone(),
        version: report.version.clone(),
        model,
        passed: report.passed,
        report: rv.clone(),
    });
    k.emit(&format!("person:{}", p.id), ev).await?;
    Ok(Json(rv).into_response())
}

pub(super) async fn catalog(State(k): K, _: Owner, Path(id): Path<String>) -> ApiResult {
    Ok(match k.adapters.catalog(&id).await {
        Ok(ev) => Json(serde_json::to_value(ev).unwrap_or_default()).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

pub(super) async fn sign_in_begin(State(k): K, _: Owner, Path(id): Path<String>) -> ApiResult {
    Ok(match k.adapters.auth_begin(&id).await {
        Ok(ev) => Json(serde_json::to_value(ev).unwrap_or_default()).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

pub(super) async fn sign_in_complete(State(k): K, Owner(p): Owner, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    Ok(match k.adapters.auth_complete(&id, b["flow"].clone(), b["input"].clone()).await {
        Ok(Ok(())) => {
            let name = k.cfg.adapters.iter().find(|a| a.id == id).and_then(|a| a.settings.get("credential")).and_then(|v| v.as_str()).unwrap_or(&id).to_string();
            let _ = k.emit(&format!("person:{}", p.id), Ev::SecretSet { name }).await;
            Json(json!({ "ok": true })).into_response()
        }
        Ok(Err(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    })
}

pub(super) async fn get_model(State(k): K, _: Caller) -> ApiResult {
    let (adapter, model) = k.read(|s| s.org.model().clone()).unwrap_or_else(|| (k.cfg.model.adapter.clone(), k.cfg.model.name.clone()));
    Ok(Json(json!({ "adapter": adapter, "model": model })).into_response())
}

/// The models anyone signed in may choose for a task or a chat message,
/// and the one used when they choose none.
pub(super) async fn models(State(k): K, h: HeaderMap) -> ApiResult {
    let Ok(p) = who(&k, &h).await else { return refuse(StatusCode::UNAUTHORIZED, "sign in") };
    let (default, lists) = k.models_for(&p.id).await;
    let list: Vec<Value> = lists.into_iter().map(|(id, models)| json!({ "adapter": id, "models": models })).collect();
    Ok(Json(json!({ "default": default.map(|(adapter, model)| json!({ "adapter": adapter, "model": model })), "adapters": list })).into_response())
}

/// Choose the model new tasks run on. Running tasks keep their pinned model (K9).
pub(super) async fn put_model(State(k): K, Owner(p): Owner, Body(b): Body<req::Model>) -> ApiResult {
    let (Some(adapter), Some(model)) = (b.adapter.as_deref(), b.model.as_deref()) else {
        return refuse(StatusCode::BAD_REQUEST, "adapter and model are required");
    };
    if k.adapters.list().await.iter().all(|(id, _, _)| id != adapter) {
        return refuse(StatusCode::NOT_FOUND, "no such adapter");
    }
    Ok(match k.emit(&format!("person:{}", p.id), Ev::ModelSet { adapter: adapter.into(), model: model.into() }).await {
        Ok(_) => Json(json!({ "adapter": adapter, "model": model })).into_response(),
        Err(e) => internal(e),
    })
}
