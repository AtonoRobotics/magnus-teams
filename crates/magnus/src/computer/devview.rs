//! The developer view of the computers: every computer, what each one
//! reached through the egress proxy, live, and pause or start. Behind the
//! developer role, like the rest of `/v1/dev` (spec: "Developer view. Full
//! observability of agents, the computer and its policy decisions, with the
//! ability to pause or start"). The screen and files stay behind a session
//! the user grants.

use super::egress::watch;
use super::{KillMode, Owner, Role, Status};
use crate::anonymize::{Lens, Sight};
use crate::api::developer;
use crate::config::Person;
use crate::controls::Refused;
use crate::grants::answered;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type K = State<Arc<Kernel>>;

/// Rows one poll returns when the view does not say.
const DEFAULT_ROWS: i64 = 200;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new()
        .route("/v1/dev/computers", get(list))
        .route("/v1/dev/computers/{id}/egress", get(egress))
        .route("/v1/dev/computers/{id}/pause", post(pause))
        .route("/v1/dev/computers/{id}/start", post(start))
}

macro_rules! dev {
    ($k:expr, $h:expr) => {
        match developer(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

/// Whether `who` is the person the computer belongs to, who sees full paths.
fn own(who: &Person, owner: &Owner) -> bool {
    matches!(owner, Owner::Person(p) if *p == who.id)
}

/// Whether a developer's pause applies: the kill switch on a running
/// computer, never a destroy.
fn pausable(status: Status) -> Result<(), Refused> {
    match status {
        Status::Running => Ok(()),
        _ => Err(Refused::Conflict("the computer is not running".into())),
    }
}

impl Kernel {
    fn dev_computer(&self, id: &str) -> Result<super::Computer, Refused> {
        self.read(|s| s.computers.get(id).filter(|c| c.role == Role::Computer).cloned()).ok_or(Refused::NotFound("computer"))
    }

    /// How `lens` sees a computer: as its person allows, and a workspace's
    /// only as far as every member allows (`crate::anonymize`).
    fn sight_of(&self, lens: &Lens, owner: &Owner) -> Sight {
        match owner {
            Owner::Person(p) => lens.sight(p),
            Owner::Workspace(w) => self.read(|s| s.team.workspace(w).map_or(Sight::Hidden, |w| lens.sight_of_all(w.members.keys()))),
        }
    }

    pub fn dev_computers(&self, who: &Person) -> Value {
        let lens = self.lens(who);
        let all: Vec<(Owner, Value)> = self.read(|s| {
            s.computers
                .all()
                .filter(|c| c.role == Role::Computer)
                .map(|c| {
                    let v = json!({
                        "id": c.id, "owner": c.owner, "status": c.status, "controller": c.controller,
                        "destinations": c.destinations, "apps": c.apps.iter().map(|a| &a.name).collect::<Vec<_>>(),
                    });
                    (c.owner.clone(), v)
                })
                .collect()
        });
        let shown: Vec<Value> = all.into_iter().filter_map(|(owner, v)| lens.show(self.sight_of(&lens, &owner), v)).collect();
        json!({ "computers": shown })
    }

    /// What the computer reached: in full for its own person or a tester,
    /// hostnames alone for someone who shares anonymous usage data, and
    /// nothing at all for anyone else.
    pub async fn dev_egress(&self, who: &Person, id: &str, after: i64, limit: i64) -> Result<Value, Refused> {
        let c = self.dev_computer(id)?;
        let lens = self.lens(who);
        let sight = self.sight_of(&lens, &c.owner);
        if sight == Sight::Hidden {
            return Err(Refused::NotFound("computer"));
        }
        let full = sight == Sight::Full || own(who, &c.owner);
        let mut rows = watch::since(&self.cfg.database.url, id, after, limit, full).await.map_err(super::service::failed)?;
        if sight == Sight::Anonymous {
            for r in &mut rows {
                r.path = String::new();
                r.address = None;
                r.detail = None;
            }
        }
        Ok(json!({ "status": c.status, "egress": rows }))
    }

    /// The same kill switch as the computer page's Pause now: every grant
    /// revoked and the VM frozen, in one step.
    pub async fn dev_pause(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.dev_computer(id)?;
        pausable(c.status)?;
        self.kill_this(who, &c, KillMode::Pause, false).await
    }

    pub async fn dev_start(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let c = self.dev_computer(id)?;
        self.start_this(who, &c).await
    }
}

async fn list(State(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    answered(Ok(k.dev_computers(&p)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct After {
    #[serde(default)]
    after: i64,
    #[serde(default)]
    limit: Option<i64>,
}

async fn egress(State(k): K, h: HeaderMap, Path(id): Path<String>, Query(q): Query<After>) -> Response {
    let p = dev!(k, h);
    answered(k.dev_egress(&p, &id, q.after, q.limit.unwrap_or(DEFAULT_ROWS)).await)
}

async fn pause(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = dev!(k, h);
    answered(k.dev_pause(&p, &id).await)
}

async fn start(State(k): K, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = dev!(k, h);
    answered(k.dev_start(&p, &id).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str) -> Person {
        serde_json::from_value(json!({ "id": id, "groups": [], "roles": ["developer"] })).unwrap()
    }

    #[test]
    fn only_the_person_it_belongs_to_counts_as_its_own() {
        assert!(own(&person("ada"), &Owner::Person("ada".into())));
        assert!(!own(&person("sam"), &Owner::Person("ada".into())));
        assert!(!own(&person("ada"), &Owner::Workspace("ada".into())));
    }

    #[test]
    fn a_developer_pauses_only_a_running_computer() {
        assert!(pausable(Status::Running).is_ok());
        for s in [Status::Paused, Status::Stopped, Status::Hibernated, Status::Killed, Status::Queued] {
            assert!(matches!(pausable(s), Err(Refused::Conflict(_))), "{s:?}");
        }
    }
}
