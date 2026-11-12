//! Admin → Spend and Admin → Models as the kernel's own operations: the
//! app's admin routes (`routes::admin_router`) call them with the admin
//! the session names.

use super::requests::Refusal;
use super::{enforce, month, valid_usd, Change, Scope};
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn failed(e: anyhow::Error) -> Refusal {
    tracing::error!("spend admin: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}

/// The largest number of models one rule names.
pub const MAX_RULE_MODELS: usize = 200;

fn rows<T>(m: &BTreeMap<String, T>, f: impl Fn(&String, &T) -> Value) -> Vec<Value> {
    let mut v: Vec<Value> = m.iter().map(|(k, x)| f(k, x)).collect();
    v.sort_by(|a, b| b["usd"].as_f64().partial_cmp(&a["usd"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    v
}

impl Kernel {
    fn name_of(&self, id: &str) -> String {
        self.person(id).map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string())
    }

    /// Admin → Spend: the limits, the month's spend broken down by person,
    /// agent and model (not by project: a project's name is its person's or
    /// its team's), the team agent's limits, the requests and
    /// the alerts. `month` is `YYYY-MM`, this month when absent.
    pub fn spend_overview(&self, month: Option<&str>) -> Result<Value, Refusal> {
        let now = Utc::now();
        let m = month.map(String::from).unwrap_or_else(|| month::key(now));
        if month::bounds(&m).is_none() {
            return Err((StatusCode::BAD_REQUEST, "a month is YYYY-MM".into()));
        }
        let (spent, l, workspaces) = self.read(|s| {
            let workspaces: Vec<Value> = s
                .team
                .all()
                .map(|w| {
                    let team = crate::team::spend::spent_this_month(s, &w.id, now);
                    // A workspace's total and its limit, never its channels' or
                    // projects' names: those are its team's, whose Team
                    // Admin sets their limits in the workspace (the owner,
                    // 2026-11-11).
                    json!({ "id": w.id, "name": w.name, "limit_usd": w.chat.limit_usd, "spent_usd": team.total_usd, "channels": w.channels.len() })
                })
                .collect();
            (month::spent(s, &m), s.spend.clone(), workspaces)
        });
        let mut people: Vec<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        people.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        people.extend(spent.by_person.keys().cloned());
        people.extend(l.people_usd.keys().cloned());
        people.sort();
        people.dedup();
        let server_left = l.server_usd.map(|x| (x - spent.total_usd).max(0.0));
        let by_person: Vec<Value> = people
            .iter()
            .map(|p| {
                let usd = spent.by_person.get(p).copied().unwrap_or_default();
                let own = l.person_limit(p, &m);
                // The cascade: nobody may spend past what the server has left.
                let effective = match (own.map(|x| (x - usd).max(0.0)), server_left) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                json!({ "id": p, "name": self.name_of(p), "usd": usd, "override_usd": l.people_usd.get(p), "limit_usd": own,
                        "extra_usd": l.extra_usd.get(&(p.clone(), m.clone())), "left_usd": effective })
            })
            .collect();
        let mut by_person = by_person;
        by_person.sort_by(|a, b| b["usd"].as_f64().partial_cmp(&a["usd"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        let ws_name = |id: &str| workspaces.iter().find(|w| w["id"] == id).and_then(|w| w["name"].as_str()).unwrap_or(id).to_string();
        let by_agent = rows(&spent.by_agent, |k, usd| {
            let name = k.strip_prefix("team:").map(|w| format!("Team agent · {}", ws_name(w))).unwrap_or_else(|| "Personal agents".into());
            json!({ "id": k, "name": name, "usd": usd })
        });
        let by_model = rows(&spent.by_model, |k, usd| json!({ "id": k, "name": k, "usd": usd }));
        let mut requests: Vec<Value> = l
            .requests
            .values()
            .filter(|r| r.month == m || r.decided.is_none())
            .map(|r| json!({ "id": r.id, "person": r.person, "name": self.name_of(&r.person), "month": r.month, "usd": r.usd, "reason": r.reason, "at": r.at, "decided": r.decided }))
            .collect();
        requests.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
        let alerts: Vec<Value> = l.alerted.iter().filter(|(_, mm, _)| *mm == m).map(|(limit, _, p)| json!({ "limit": limit, "percent": p })).collect();
        Ok(json!({
            "month": m,
            "limits": { "server_usd": l.server_usd, "default_usd": l.default_usd },
            "total_usd": spent.total_usd, "server_left_usd": server_left,
            "by_person": by_person, "by_agent": by_agent, "by_model": by_model,
            "workspaces": workspaces, "requests": requests, "alerts": alerts,
        }))
    }

    /// Sets or lifts (None) the server's limit, the default per person, or
    /// one person's override.
    pub async fn set_spend_limit(&self, actor: &str, scope: Scope, usd: Option<f64>) -> Result<Value, Refusal> {
        if usd.is_some_and(|u| !valid_usd(u)) {
            return Err((StatusCode::BAD_REQUEST, format!("a limit is an amount of dollars from 0 to {}", super::MAX_LIMIT_USD)));
        }
        if let Scope::Person(p) = &scope {
            if self.person(p).is_none() {
                return Err((StatusCode::NOT_FOUND, "no such person".into()));
            }
        }
        self.emit(actor, Ev::Spend(Change::LimitSet { scope: scope.clone(), usd })).await.map_err(failed)?;
        Ok(json!({ "scope": scope, "usd": usd }))
    }

    /// Sets or lifts the team agent's limit for a workspace, or for one of
    /// its channels: the same change a workspace owner makes.
    pub async fn set_team_limit(&self, actor: &str, workspace: &str, channel: Option<String>, usd: Option<f64>) -> Result<Value, Refusal> {
        if usd.is_some_and(|u| !valid_usd(u)) {
            return Err((StatusCode::BAD_REQUEST, format!("a limit is an amount of dollars from 0 to {}", super::MAX_LIMIT_USD)));
        }
        let known = self.read(|s| s.team.workspace(workspace).map(|w| channel.as_ref().is_none_or(|c| w.channels.contains_key(c))));
        match known {
            None => return Err((StatusCode::NOT_FOUND, "no such workspace".into())),
            Some(false) => return Err((StatusCode::NOT_FOUND, "no such channel".into())),
            Some(true) => {}
        }
        let chat = crate::team::chat::Chat::SpendLimit { channel: channel.clone(), usd };
        self.emit(actor, Ev::Team(crate::team::Change::Chat { workspace: workspace.into(), chat })).await.map_err(failed)?;
        Ok(json!({ "workspace": workspace, "channel": channel, "usd": usd }))
    }

    /// Admin → Models: the default model and who may use which (the
    /// providers and their models are the console's `admin_providers`).
    pub fn models_overview(&self) -> Value {
        let (adapter, model) = self.default_model();
        let access = self.read(|s| s.spend.access.clone());
        let mut roles: Vec<String> = vec![super::access::EVERYONE.into(), "admin".into()];
        for p in &self.cfg.people {
            if let Some(p) = self.person(&p.id) {
                roles.extend(p.roles.into_iter().filter(|r| r != "owner"));
            }
        }
        roles.extend(access.roles.keys().cloned());
        roles.sort();
        roles.dedup();
        let mut people: Vec<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        people.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        people.sort();
        people.dedup();
        let people: Vec<Value> = people.iter().map(|p| json!({ "id": p, "name": self.name_of(p), "owner": self.person(p).is_some_and(|x| x.is_owner()) })).collect();
        json!({ "default": { "adapter": adapter, "model": model }, "access": access, "roles": roles, "people": people })
    }

    /// Sets or removes (None) the models a role or a person may use. Each
    /// is `adapter/model` of a provider the server has.
    pub async fn set_model_access(&self, actor: &str, role: Option<String>, person: Option<String>, models: Option<Vec<String>>) -> Result<Value, Refusal> {
        match (&role, &person) {
            (Some(_), Some(_)) | (None, None) => return Err((StatusCode::BAD_REQUEST, "name a role or a person".into())),
            (Some(r), None) if r.trim().is_empty() || r == "owner" => return Err((StatusCode::BAD_REQUEST, "the owner may use every model".into())),
            (None, Some(p)) if self.person(p).is_none_or(|x| x.is_owner()) => {
                return Err((StatusCode::BAD_REQUEST, "no such person, or the owner, who may use every model".into()))
            }
            _ => {}
        }
        if let Some(m) = &models {
            if m.len() > MAX_RULE_MODELS {
                return Err((StatusCode::BAD_REQUEST, format!("a rule names at most {MAX_RULE_MODELS} models")));
            }
            let adapters: Vec<String> = self.adapters.list().await.into_iter().map(|(id, ..)| id).collect();
            if let Some(bad) = m.iter().find(|x| x.split_once('/').is_none_or(|(a, model)| model.is_empty() || !adapters.iter().any(|id| id == a))) {
                return Err((StatusCode::BAD_REQUEST, format!("{bad} is not adapter/model of a provider the server has")));
            }
        }
        self.emit(actor, Ev::Spend(Change::ModelsAllowed { role: role.clone(), person: person.clone(), models: models.clone() })).await.map_err(failed)?;
        Ok(json!({ "role": role, "person": person, "models": models }))
    }

    /// The models a person may choose, as the app's model picker lists
    /// them, and the one their work goes to when they choose none.
    pub async fn models_for(&self, person: &str) -> (Option<(String, String)>, Vec<(String, Vec<String>)>) {
        let allowed = self.allowed_models(person).await;
        let lists = self.models().await.into_iter().map(|(a, ms)| (a.clone(), ms.into_iter().filter(|m| super::access::permits(&allowed, &a, m)).collect::<Vec<_>>()));
        let lists: Vec<(String, Vec<String>)> = lists.filter(|(_, ms)| !ms.is_empty()).collect();
        (enforce::choose(&allowed, None, self.default_model()).ok(), lists)
    }
}
