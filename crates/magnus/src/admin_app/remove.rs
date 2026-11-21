//! Removing a person (spec rev 141, People): "Removing a person transfers or
//! deletes their projects and agents and deletes their data, and the last
//! owner cannot be removed."
//!
//! - Their own data, their personal projects and their own agent's work
//!   (runs, schedules) are deleted by the console's deletion
//!   (`Kernel::erase_person`): a personal project holds what is theirs alone,
//!   so it is never handed to someone else.
//! - What they hold for a team is transferred to the person the admin names,
//!   or deleted: the team projects they own and the team agent's routines
//!   they set up. A workspace they own always passes to that person, because
//!   a workspace with members is never left without an owner; deleting asks
//!   for someone to pass it to when there is such a workspace.
//! - They leave every workspace.

use super::mount::{audited, body_json, control, Audit};
use super::people::{last_owner, refuse};
use super::Admin;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::team::{Change, Role};
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;

/// What a person holds for teams, as removing them finds it.
#[derive(Debug, Default, PartialEq)]
pub struct Held {
    /// (workspace, project) they own.
    pub projects: Vec<(String, String)>,
    /// (workspace, routine) they set up.
    pub routines: Vec<(String, String)>,
    /// Workspaces they own that have someone else in them.
    pub owned: Vec<String>,
    /// Every workspace they are in.
    pub member_of: Vec<String>,
}

pub fn held(team: &crate::team::Ledger, id: &str) -> Held {
    let mut h = Held::default();
    for w in team.all() {
        h.projects.extend(w.projects.values().filter(|p| p.owner == id).map(|p| (w.id.clone(), p.id.clone())));
        h.routines.extend(w.routines.iter().filter(|(_, r)| r.by == id).map(|(r, _)| (w.id.clone(), r.clone())));
        if w.members.get(id) == Some(&Role::Owner) && w.members.keys().any(|m| m != id) {
            h.owned.push(w.id.clone());
        }
        if w.members.contains_key(id) {
            h.member_of.push(w.id.clone());
        }
    }
    h
}

/// Pick the existing workspace member who receives `id`'s responsibilities in
/// `ws`: `to` when `to` is already a member of `ws`, otherwise the
/// highest-ranked remaining member of `ws`. An outsider (including a server
/// admin) is never added to a workspace they do not belong to.
fn successor_in(team: &crate::team::Ledger, ws: &str, id: &str, to: Option<&str>) -> Option<String> {
    if let Some(to) = to.filter(|t| *t != id && team.role(ws, t).is_some()) {
        return Some(to.to_string());
    }
    let w = team.workspace(ws)?;
    w.members.iter().filter(|(m, _)| m.as_str() != id).max_by(|(a_id, a_role), (b_id, b_role)| a_role.cmp(b_role).then_with(|| b_id.cmp(a_id))).map(|(m, _)| m.clone())
}

/// Workspaces where `id` is the only member: removing `id` deletes the
/// workspace rather than leaving an empty, ownerless workspace behind.
pub fn sole_workspaces(team: &crate::team::Ledger, id: &str) -> Vec<String> {
    team.all().filter(|w| w.members.len() == 1 && w.members.contains_key(id)).map(|w| w.id.clone()).collect()
}

/// The team changes that move what `id` holds to `to` (or an existing member
/// of each workspace), or delete it.
pub fn changes(team: &crate::team::Ledger, id: &str, h: &Held, to: Option<&str>) -> Vec<Change> {
    let mut out = vec![];
    let promote = |out: &mut Vec<Change>, ws: &str, person: &str, min_role: Role| {
        if let Some(current) = team.role(ws, person) {
            if current < min_role {
                out.push(Change::Member { workspace: ws.into(), person: person.into(), role: min_role });
            }
        }
    };
    for ws in &h.owned {
        if let Some(next_owner) = successor_in(team, ws, id, to) {
            promote(&mut out, ws, &next_owner, Role::Owner);
        }
    }
    for (ws, p) in &h.projects {
        let Some(def) = team.workspace(ws).and_then(|w| w.projects.get(p)).cloned() else { continue };
        let recipient = if to.is_some() { successor_in(team, ws, id, to) } else { None };
        match recipient {
            Some(rec) => {
                promote(&mut out, ws, &rec, Role::Member);
                let mut def = def;
                def.owner = rec.clone();
                def.members.retain(|m| m != id);
                if !def.members.iter().any(|m| m == &rec) {
                    def.members.push(rec);
                }
                out.push(Change::Project { workspace: ws.clone(), id: p.clone(), def });
            }
            None => out.push(Change::ProjectRemoved { workspace: ws.clone(), id: p.clone() }),
        }
    }
    for (ws, r) in &h.routines {
        let Some(routine) = team.workspace(ws).and_then(|w| w.routines.get(r)).cloned() else { continue };
        let recipient = if to.is_some() { successor_in(team, ws, id, to) } else { None };
        match recipient {
            Some(rec) => out.push(Change::Routine { workspace: ws.clone(), id: r.clone(), routine: crate::team::routines::Routine { by: rec, ..routine } }),
            None => out.push(Change::RoutineRemoved { workspace: ws.clone(), id: r.clone() }),
        }
    }
    for ws in &h.member_of {
        if team.workspace(ws).is_some_and(|w| w.members.len() <= 1 && w.members.contains_key(id)) {
            out.push(Change::Deleted { workspace: ws.clone() });
        } else {
            out.push(Change::Removed { workspace: ws.clone(), person: id.into() });
        }
    }
    out
}

/// `DELETE /v1/admin/people/{id}` with `{projects: "transfer"|"delete", to?}`.
pub(super) async fn remove(State(k): State<Arc<Kernel>>, Extension(a): Extension<Admin>, Path(id): Path<String>, body: Option<Json<Value>>) -> Response {
    let id = id.trim().to_lowercase();
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let Some(target) = k.person(&id) else { return refuse(StatusCode::NOT_FOUND, "not_found", "No such person.") };
    if id == a.person.id {
        return refuse(StatusCode::CONFLICT, "self", "You cannot remove yourself: another admin can.");
    }
    if k.last_owner(&id).await {
        return last_owner();
    }
    if crate::people::rights(&target).is_server_owner() && !crate::people::rights(&a.person).may_change_owners() {
        return refuse(StatusCode::FORBIDDEN, "owner_only", "Only an owner removes an owner.");
    }
    let to = b["to"].as_str().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
    let transfer = match b["projects"].as_str() {
        Some("transfer") => true,
        Some("delete") => false,
        _ => return refuse(StatusCode::BAD_REQUEST, "invalid", "Say whether their projects and agents are transferred or deleted."),
    };
    if let Some(to) = &to {
        if *to == id || k.person(to).is_none() || k.accounts.status(to).await == crate::accounts::Status::Disabled {
            return refuse(StatusCode::BAD_REQUEST, "invalid", "Transfer to someone else who is here and turned on.");
        }
    }
    let h = k.read(|s| held(&s.team, &id));
    if transfer && to.is_none() {
        return refuse(StatusCode::BAD_REQUEST, "invalid", "Name who receives their projects and agents.");
    }
    let given = if transfer { to.as_deref() } else { None };
    let counts = json!({ "projects": h.projects.len(), "routines": h.routines.len(), "workspaces_owned": h.owned.len(), "workspaces_left": h.member_of.len() });
    let audit = Audit {
        action: "user_deleted".into(),
        target: Some(format!("people/{id}")),
        before: None,
        after: Some(json!({ "projects": if transfer { "transfer" } else { "delete" }, "to": to, "held": counts })),
    };
    let actor = a.actor();
    let change = async {
        let sole = k.read(|s| sole_workspaces(&s.team, &id));
        for ws in &sole {
            if let Err(e) = k.delete_workspace(&actor, ws).await {
                return refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}"));
            }
        }
        let evs = k.read(|s| {
            let h = held(&s.team, &id);
            changes(&s.team, &id, &h, given)
        });
        let team: Vec<(&str, Ev)> = evs.into_iter().map(|c| (actor.as_str(), Ev::Team(c))).collect();
        if !team.is_empty() {
            if let Err(e) = k.emit_all(team).await {
                return refuse(StatusCode::INTERNAL_SERVER_ERROR, "failed", &format!("{e:#}"));
            }
        }
        let r = control(&k, &a, Method::DELETE, &format!("/v1/admin/people/{id}"), None).await;
        if !r.status().is_success() {
            return r;
        }
        let mut v = body_json(r).await;
        v["team"] = counts.clone();
        v["projects"] = json!(if transfer { "transferred" } else { "deleted" });
        Json(v).into_response()
    };
    audited(&k, &a, audit, change).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team() -> crate::team::Ledger {
        let mut l = crate::team::Ledger::default();
        l.apply(&Change::Created { workspace: "w".into(), name: "W".into(), owner: "ann".into() });
        l.apply(&Change::Member { workspace: "w".into(), person: "bo".into(), role: Role::Member });
        let p = crate::team::knowledge::Project { id: "p".into(), owner: "ann".into(), members: vec!["ann".into()], ..Default::default() };
        l.apply(&Change::Project { workspace: "w".into(), id: "p".into(), def: p });
        let r = crate::team::routines::Routine { channel: "c".into(), text: "t".into(), every_hours: 1, by: "ann".into(), last_at: None, ..Default::default() };
        l.apply(&Change::Routine { workspace: "w".into(), id: "r".into(), routine: r });
        l
    }

    /// Transferring hands the projects, routines and the workspace to the
    /// person named; deleting removes the projects and routines. Either way
    /// they leave every workspace.
    #[test]
    fn removing_transfers_or_deletes_what_they_hold_for_teams() {
        let mut l = team();
        let h = held(&l, "ann");
        assert_eq!((h.projects.len(), h.routines.len(), h.owned.clone(), h.member_of.clone()), (1, 1, vec!["w".to_string()], vec!["w".to_string()]));
        for c in changes(&l, "ann", &h, Some("bo")) {
            l.apply(&c);
        }
        let w = l.workspace("w").unwrap();
        assert_eq!(w.projects["p"].owner, "bo");
        assert_eq!(w.routines["r"].by, "bo");
        assert_eq!(l.role("w", "bo"), Some(Role::Owner), "the workspace was left without its owner");
        assert_eq!(l.role("w", "ann"), None, "they stayed in the workspace");

        let mut l = team();
        let h = held(&l, "ann");
        for c in changes(&l, "ann", &h, None) {
            l.apply(&c);
        }
        let w = l.workspace("w").unwrap();
        assert!(w.projects.is_empty() && w.routines.is_empty(), "deleting kept their team projects or routines");
        assert_eq!(l.role("w", "bo"), Some(Role::Owner), "deleting left a populated workspace without an owner");
        assert_eq!(held(&l, "bo"), Held { member_of: vec!["w".into()], ..Default::default() });
    }

    #[test]
    fn removing_never_adds_an_outsider_or_admin_to_a_workspace() {
        let mut l = team();
        let h = held(&l, "ann");
        for c in changes(&l, "ann", &h, Some("server_admin")) {
            l.apply(&c);
        }
        assert_eq!(l.role("w", "server_admin"), None, "an outsider was added to the workspace during removal");
        assert_eq!(l.role("w", "bo"), Some(Role::Owner), "existing workspace member was not promoted to owner");
        let w = l.workspace("w").unwrap();
        assert_eq!(w.projects["p"].owner, "bo");
        assert_eq!(w.routines["r"].by, "bo");
    }
}
