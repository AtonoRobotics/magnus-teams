//! Connecting an outside agent, admitting it, and disconnecting it. A
//! member connects one by its agent card URL, as they connect an app: the
//! card is fetched through the egress client and trusted only once its
//! signature verifies; its security schemes choose the authentication; the
//! credential the member gives is kept in the vault. The agent is then a
//! Cedar principal no policy admits, until its member admits it for
//! themselves, or, as a Team Admin, for a shared project, where it is a guest.

use super::client::credential_name;
use super::ledger::{Change, OutsideAgent, Scheme, Target};
use crate::connectors::Refusal;
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::team::Role;
use axum::http::StatusCode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};

/// What a member gives to connect an outside agent: its card, and the
/// credential its card's scheme asks for (a key or token, OAuth 2 client
/// credentials, or a PEM client certificate with its key).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connect {
    pub card_url: String,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// The vault name of the key an outside agent presents to the server.
pub(crate) fn inbound_key_name(agent: &str) -> String {
    format!("outside-agent/{agent}/inbound")
}

fn refusal(st: StatusCode, m: impl Into<String>) -> Refusal {
    (st, m.into())
}

fn failed(e: anyhow::Error) -> Refusal {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
}

/// An outside agent as its member's app shows it.
pub(crate) fn view(a: &OutsideAgent, admissions: &[(Target, String)]) -> Value {
    let admitted: Vec<Value> = admissions
        .iter()
        .map(|(t, _)| match t {
            Target::Person(p) => json!({ "person": p }),
            Target::Project(w) => json!({ "project": w }),
        })
        .collect();
    json!({ "id": a.id, "name": a.name, "description": a.description, "card_url": a.card_url, "endpoint": a.endpoint, "scheme": a.scheme.as_str(), "skills": a.skills, "admitted": admitted })
}

impl Kernel {
    /// Connect the outside agent whose card is at `b.card_url`, for `person`.
    /// Answers its view and, once, the key it presents to this server.
    pub(crate) async fn connect_outside_agent(&self, person: &str, b: Connect) -> Result<Value, Refusal> {
        let allow_http = self.cfg.web.allow_private_networks;
        let egress = crate::egress_client::Egress::new(allow_http, super::CONNECT_WAIT, super::CALL_WAIT);
        let v = super::card::fetch(&egress, &b.card_url, allow_http).await.map_err(|e| refusal(StatusCode::BAD_REQUEST, format!("{e:#}")))?;
        let given = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let secret = match &v.scheme {
            Scheme::None => None,
            Scheme::OAuth2 { .. } => match (given(&b.client_id), given(&b.client_secret)) {
                (Some(id), Some(secret)) => Some(json!({ "client_id": id, "client_secret": secret }).to_string()),
                _ => return Err(refusal(StatusCode::BAD_REQUEST, format!("{} signs in with OAuth 2: give its client id and client secret", v.name))),
            },
            Scheme::MutualTls => {
                let pem =
                    given(&b.credential).ok_or_else(|| refusal(StatusCode::BAD_REQUEST, format!("{} asks for a client certificate: give it with its key, in PEM", v.name)))?;
                reqwest::Identity::from_pem(pem.as_bytes()).map_err(|e| refusal(StatusCode::BAD_REQUEST, format!("that is not a client certificate with its key: {e}")))?;
                Some(pem)
            }
            Scheme::ApiKey { .. } | Scheme::Bearer => {
                Some(given(&b.credential).ok_or_else(|| refusal(StatusCode::BAD_REQUEST, format!("{} asks for a key: give the one its operator issued you", v.name)))?)
            }
        };
        let id = new_id("oa");
        if let Some(s) = &secret {
            self.vault.put(&credential_name(&id), s).map_err(failed)?;
        }
        let key = {
            use rand::RngCore;
            let mut b = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut b);
            format!("{id}.{}", B64.encode(b))
        };
        self.vault.put(&inbound_key_name(&id), &key).map_err(failed)?;
        let agent = OutsideAgent {
            id: id.clone(),
            name: v.name,
            description: super::bounded(&v.description),
            card_url: b.card_url.trim().to_string(),
            endpoint: v.endpoint,
            scheme: v.scheme,
            person: person.to_string(),
            skills: v.skills,
            key_id: v.key_id,
        };
        self.emit(&format!("person:{person}"), Ev::OutsideAgent(Change::Connected { agent: agent.clone() })).await.map_err(failed)?;
        Ok(json!({ "agent": view(&agent, &[]), "inbound_key": key }))
    }

    /// The agent `actor` connected, or a refusal naming why not.
    pub(crate) fn own_outside_agent(&self, actor: &str, id: &str) -> Result<OutsideAgent, Refusal> {
        self.read(|s| s.outside_agents.agent(id).filter(|a| a.person == actor).cloned()).ok_or_else(|| refusal(StatusCode::NOT_FOUND, "no such outside agent of yours"))
    }

    /// Admit `id` to ask `to`: its member for themselves, or a Team Admin
    /// of a shared project, who connected it, for the project, where it is
    /// then a guest. One Cedar policy each.
    pub(crate) async fn admit_outside_agent(&self, actor: &str, id: &str, to: Target) -> Result<Value, Refusal> {
        let agent = self.own_outside_agent(actor, id)?;
        if !to.valid() {
            return Err(refusal(StatusCode::BAD_REQUEST, "that is not a person's or a shared project's id"));
        }
        match &to {
            Target::Person(p) if p == actor => {}
            Target::Person(_) => return Err(refusal(StatusCode::FORBIDDEN, "an outside agent is admitted for a person by that person alone")),
            Target::Project(w) => {
                if !self.read(|s| s.team.role(w, actor)).is_some_and(|r| r >= Role::Admin) {
                    return Err(refusal(StatusCode::FORBIDDEN, "only a Team Admin of that shared project adds an outside agent to it as a guest"));
                }
            }
        }
        let policy = crate::policy::outside::admission(&agent.id, &to).map_err(failed)?;
        self.emit(&format!("person:{actor}"), Ev::OutsideAgent(Change::Admitted { agent: agent.id.clone(), to, policy })).await.map_err(failed)?;
        let admissions = self.read(|s| s.outside_agents.admissions(&agent.id).to_vec());
        Ok(view(&agent, &admissions))
    }

    /// Disconnect an outside agent: its open tasks are cancelled in both
    /// directions (CancelTask for each task sent it; each request it sent
    /// ends, and a run taken up for one stops), then its credentials leave
    /// the vault and it is no principal any more.
    pub(crate) async fn disconnect_outside_agent(&self, actor: &str, id: &str) -> Result<Value, Refusal> {
        let agent = self.own_outside_agent(actor, id)?;
        let sent = self.cancel_sent_tasks(&agent, |_| true).await;
        let received = self.end_requests(&agent.id, "the outside agent was disconnected").await.map_err(failed)?;
        for name in [credential_name(&agent.id), inbound_key_name(&agent.id)] {
            self.vault.remove(&name).map_err(failed)?;
        }
        self.emit(&format!("person:{actor}"), Ev::OutsideAgent(Change::Disconnected { agent: agent.id.clone() })).await.map_err(failed)?;
        Ok(json!({ "disconnected": agent.id, "cancelled_sent": sent, "ended_received": received }))
    }

    /// Disconnect every outside agent `person` connected (their erasure).
    pub(crate) async fn disconnect_outside_agents_of(&self, person: &str) -> anyhow::Result<usize> {
        let ids: Vec<String> = self.read(|s| s.outside_agents.of(person).into_iter().map(|a| a.id.clone()).collect());
        for id in &ids {
            self.disconnect_outside_agent(person, id).await.map_err(|(_, m)| anyhow::anyhow!(m))?;
        }
        Ok(ids.len())
    }
}
