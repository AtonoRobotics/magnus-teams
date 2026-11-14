//! Admin › Agents (spec rev 141): every agent on the server, what each is
//! running now (tasks, schedules, sub-agents) with who asked and what it
//! has spent, and what an agent did. The list names no one's content: no
//! task's words, only what kind of work it is. What an agent did is read
//! from the log; for a person's agent that is the person's data, and the
//! team agent's is its teams', so only the person reads their own.
use super::{agent_of, Scope, TEAM_AGENT};
use crate::kernel::Kernel;
use crate::state::{Run, State};
use axum::http::StatusCode;
use serde_json::{json, Value};

/// The most of an agent's past runs its activity shows.
pub const MAX_ACTIVITY_RUNS: usize = 200;

/// Who asked for a run: the person, a schedule, Magnus itself, or (for
/// the team agent) the member who asked in a thread.
fn asked_by(s: &State, run: &Run) -> Value {
    let top = s.authority(&run.id);
    if let Some(t) = s.team.run_of(&run.task) {
        return json!({ "kind": if t.sub { "sub_agent" } else { "member" }, "person": t.requester, "workspace": t.workspace });
    }
    match top {
        Some(t) if t.background() => json!({ "kind": "magnus", "person": t.person }),
        Some(t) if t.schedule.is_some() => json!({ "kind": "schedule", "schedule": t.schedule, "person": t.person }),
        Some(t) => json!({ "kind": "person", "person": t.person }),
        None => json!({ "kind": "person", "person": run.person }),
    }
}

/// What kind of work a run is, without its words.
fn kind(s: &State, run: &Run) -> &'static str {
    s.run_kind(&run.id).label()
}

fn run_row(s: &State, r: &Run) -> Value {
    let subs: Vec<Value> = s
        .sub_agents
        .below(&r.id)
        .into_iter()
        .filter_map(|(depth, sa)| s.runs.get(&sa.child).map(|c| json!({ "run": c.id, "depth": depth, "status": c.status, "spent_usd": c.usd })))
        .collect();
    json!({
        "task": r.task, "run": r.id, "kind": kind(s, r), "status": r.status, "waiting_on": r.reason,
        "asked_by": asked_by(s, r), "started": r.created_at, "spent_usd": r.spent_usd(), "actions": r.actions,
        "sub_agents": subs,
    })
}

fn pulled(s: &State, scope: &Scope) -> Value {
    json!(s.halts.on.get(scope))
}

/// One agent as the list shows it.
fn agent_row(s: &State, id: &str, name: &str, person: Option<&str>) -> Value {
    let mine = |r: &&Run| agent_of(s, r) == id;
    let runs: Vec<&Run> = s.runs.values().filter(mine).collect();
    let mut now: Vec<&Run> = runs.iter().copied().filter(|r| !r.status.terminal() && s.sub_agents.get(&r.id).is_none()).collect();
    now.sort_by_key(|r| r.created_at);
    let schedules: Vec<Value> = match person {
        Some(p) => s
            .schedules
            .values()
            .filter(|x| x.person == p && x.ended.is_none())
            .map(|x| json!({ "id": x.id, "next": x.next_due(), "last_fired": x.last_fired, "paused": x.paused.is_some() || x.paused_on.is_some(), "asked_by": { "kind": "person", "person": p } }))
            .collect(),
        None => s
            .team
            .all()
            .flat_map(|w| w.routines.iter().map(move |(rid, r)| json!({ "id": rid, "workspace": w.id, "every_hours": r.every_hours, "last_fired": r.last_at, "asked_by": { "kind": "member", "person": r.by } })))
            .collect(),
    };
    json!({
        "id": id, "name": name, "person": person,
        "running": now.into_iter().map(|r| run_row(s, r)).collect::<Vec<_>>(),
        "schedules": schedules,
        "spent_usd": runs.iter().map(|r| r.usd).sum::<f64>(),
        "paused": s.halts.paused.get(id),
        "stopped": pulled(s, &Scope::Agent(id.into())),
        "person_stopped": person.map(|p| pulled(s, &Scope::Person(p.into()))),
    })
}

/// Every agent: the team agent first, then each person's, by name.
pub fn agents(s: &State, people: &[(String, String)]) -> Value {
    let mut rows = vec![agent_row(s, TEAM_AGENT, "Team agent", None)];
    for (id, name) in people {
        rows.push(agent_row(s, &super::agent_of_person(id), name, Some(id)));
    }
    json!({ "agents": rows, "server": { "engaged": s.kill_switch, "by": s.kill_switch_by.as_ref().filter(|_| s.kill_switch).map(|(by, at)| json!({ "by": by, "at": at })) } })
}

/// What an agent did, newest first: each run with every boundary decision
/// (the tool, what was decided and where). No call's words.
pub fn activity(s: &State, agent: &str) -> Vec<Value> {
    let mut runs: Vec<&Run> = s.runs.values().filter(|r| agent_of(s, r) == agent).collect();
    runs.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    runs.into_iter()
        .take(MAX_ACTIVITY_RUNS)
        .map(|r| {
            let mut row = run_row(s, r);
            row["ended"] = json!(r.ended_at);
            row["decisions"] = json!(r.decisions.iter().map(|d| json!({ "seq": d.seq, "tool": d.tool, "stage": d.stage, "outcome": d.outcome })).collect::<Vec<_>>());
            row
        })
        .collect()
}

/// `Kernel::stops_below_server`.
pub fn stops_for(s: &State, person: &str) -> Vec<Value> {
    let team = !s.team.of(person).is_empty();
    s.halts
        .on
        .iter()
        .filter(|(scope, _)| match scope {
            Scope::Person(x) => x == person,
            Scope::Agent(a) => *a == super::agent_of_person(person) || (a == TEAM_AGENT && team),
            Scope::Connector(_) => true,
        })
        .map(|(scope, p)| json!({ "level": scope.level(), "target": scope.id(), "by": p.by, "by_name": p.name, "at": p.at, "said": scope.said() }))
        .collect()
}

impl Kernel {
    /// Everyone the server keeps, by name: config people and local ones.
    pub fn everyone(&self) -> Vec<(String, String)> {
        let mut ids: std::collections::BTreeSet<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        ids.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        let mut v: Vec<(String, String)> =
            ids.into_iter().map(|id| (id.clone(), self.person(&id).map(|p| p.name).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()))).collect();
        v.sort_by_key(|p| p.1.to_lowercase());
        v
    }

    /// Admin › Agents.
    pub fn agents_view(&self) -> Value {
        let people = self.everyone();
        let mut v = self.read(|s| agents(s, &people));
        // What a connector's switch can name.
        let mut names: Vec<String> = self.all_connectors().into_iter().map(|(c, _)| c.name).collect();
        names.sort();
        v["connectors"] = json!(names);
        v
    }

    /// What an agent did, read by `reader` (`by` is their name). A person's
    /// agent's activity is their data: refused where policy keeps it
    /// private, and otherwise recorded, with the person told.
    pub async fn agent_activity(&self, reader: &str, _by: &str, agent: &str) -> Result<Value, (StatusCode, String)> {
        let person = match Scope::parse("agent", agent) {
            Some(_) => agent.strip_prefix("person:").map(String::from),
            None => return Err((StatusCode::NOT_FOUND, "no such agent".into())),
        };
        // A person's agent's activity is theirs, and the team agent's is its
        // teams': a Magnus admin reads neither (the owner, 2026-11-11). The
        // Agents list, with no one's words, stays for pausing and stopping.
        let own = person.as_deref().is_some_and(|p| reader == format!("person:{p}"));
        if !own {
            return Err((StatusCode::FORBIDDEN, crate::person_admin::NOT_THE_ADMINS.into()));
        }
        let rows = self.read(|s| activity(s, agent));
        Ok(json!({ "agent": agent, "runs": rows }))
    }

    /// The switches below the server's that reach `person` now, for their
    /// banner (`admin_app::kill::stops_for`): their own, their agent's, the
    /// team agent's when they are in a workspace, and every connector's.
    /// Each as `{level, target, by, by_name, at, said}`.
    pub fn stops_below_server(&self, person: &str) -> Vec<Value> {
        self.read(|s| stops_for(s, person))
    }

    /// Every switch on, and every pause, for the admin's top bar.
    pub fn switches_all(&self) -> Value {
        self.read(|s| {
            let on: Vec<Value> =
                s.halts.on.iter().map(|(x, p)| json!({ "level": x.level(), "target": x.id(), "by": p.by, "by_name": p.name, "at": p.at, "said": x.said() })).collect();
            let paused: Vec<Value> = s.halts.paused.iter().map(|(a, p)| json!({ "agent": a, "by": p.by, "name": p.name, "at": p.at })).collect();
            let server = s.kill_switch_by.as_ref().filter(|_| s.kill_switch).map(|(by, at)| json!({ "by": by, "at": at }));
            json!({ "server": { "engaged": s.kill_switch, "pulled": server }, "on": on, "paused": paused })
        })
    }
}
