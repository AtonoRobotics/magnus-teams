//! Standing authority (behavior spec, Standing authority; behavior.md step
//! 5; #235). A grant is a person's standing authority over a category of
//! their own work: a title, the apps it covers, the exclusions they lifted,
//! and a model-cost budget per rolling period. The kernel writes it as a
//! Cedar policy in the person's layer (`policy::grant_cedar`) and stores the
//! text with it.
//!
//! A grant comes only from the person: "Handle things like this" on a task
//! proposal (`decide_card`, decision `handle`), or accepting a grant card
//! Magnus put to them when they asked in conversation (`propose_grant`).
//! Approving a proposal never creates one, and the model never sets one
//! (invariant 6): it proposes, they accept.
//!
//! A task started under a grant (`create_task` with `grant`) runs with its
//! usual checks; where the organisation's policy asks before an action, the
//! grant covers it when the action is in the grant's apps, not excluded,
//! one its person could approve themselves, and the period's budget is not
//! spent (`boundary.rs`, the approval stage). The kernel's own asks (the
//! Rule of Two, the audience rule) are never covered, and what the
//! organisation's policy forbids never reaches the question. Each covered
//! action is a `grant_used` event, listed under its grant.

use crate::api::{err, internal, who};
use crate::boundary::Action;
use crate::config::Person;
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::{Grant, State as Projection};
use crate::tools::Label;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

/// The exclusions a person may lift per grant: sending on their behalf,
/// and what cannot be undone (deleting, committing them to anything: the
/// kernel's one class `irreversible`). Spending money is not among them.
pub const LIFTABLE: [&str; 2] = ["send", "irreversible"];

/// The periods a grant's budget runs over, rolling back from now: a day, a
/// week, a month of 30 days.
pub fn period_days(p: &str) -> Option<i64> {
    match p {
        "day" => Some(1),
        "week" => Some(7),
        "month" => Some(30),
        _ => None,
    }
}

/// What still asks under a grant, in the person's words.
pub fn still_asks(allow: &[String]) -> Vec<&'static str> {
    let mut v = vec!["spending money"];
    if !allow.iter().any(|a| a == "send") {
        v.push("sending on your behalf");
    }
    if !allow.iter().any(|a| a == "irreversible") {
        v.push("deleting, or committing you to anything");
    }
    v
}

/// The model cost charged to a grant in its current period: every model
/// call of its tasks within it, whether or not the task is still there
/// (#280).
pub fn spent(_s: &Projection, g: &Grant, now: DateTime<Utc>) -> f64 {
    let from = now - chrono::Duration::days(g.days());
    g.charges.iter().filter(|(at, _)| *at > from).map(|(_, usd)| usd).sum()
}

/// A grant whose budget for its period is spent: what it spent of what.
pub struct Spent {
    pub grant: String,
    pub spent: f64,
    pub max: f64,
    pub period: String,
}

impl Kernel {
    /// The grant this run's task runs under, when its budget for the period
    /// is spent and the run's person has not resumed it past that. Every
    /// paid model call of the run checks it first: the agent's, the
    /// verifier's, the memory pass's and an extraction's (#280, #286).
    pub fn grant_spent(&self, run: &str) -> Option<Spent> {
        let now = chrono::Utc::now();
        self.read(|s| {
            let r = s.runs.get(run).filter(|r| !r.past_grant)?;
            let g = s.tasks.get(&r.task).and_then(|t| t.grant.as_ref()).and_then(|g| s.grants.get(g))?;
            let spent = spent(s, g, now);
            (spent >= g.max_usd).then(|| Spent { grant: g.id.clone(), spent, max: g.max_usd, period: g.period.clone() })
        })
    }
}

/// A grant's fields, checked.
#[derive(Debug, Clone)]
pub struct Fields {
    pub title: String,
    pub apps: Vec<String>,
    pub allow: Vec<String>,
    pub period: String,
    pub max_usd: f64,
}

/// `over` laid on `base`: each field `over` gives (not null) replaces it.
pub fn over(base: &Value, over: &Value) -> Value {
    let mut v = base.clone();
    for k in ["title", "apps", "allow", "period", "max_usd"] {
        if over.get(k).is_some_and(|x| !x.is_null()) {
            v[k] = over[k].clone();
        }
    }
    v
}

/// The app a tool belongs to, by its name (`<app>__<tool>`).
fn app_of(tool: &str) -> Option<&str> {
    tool.split_once("__").map(|(a, _)| a)
}

impl Kernel {
    /// The names of the apps `person` may use: what a grant may cover.
    fn app_names(&self, person: &Person) -> Vec<String> {
        self.apps_of(person).into_iter().map(|(c, _)| c.name).collect()
    }

    /// A grant's fields from `v`, or why they cannot be one: a title, one
    /// or more of the person's apps, exclusions lifted among `send` and
    /// `irreversible`, a period, and a budget above 0 and at most the
    /// appliance's limit for one task.
    pub fn grant_fields(&self, person: &Person, v: &Value) -> Result<Fields, String> {
        let title = v["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or("title is needed: the kind of work it covers, in a few words")?;
        let mine = self.app_names(person);
        let mut apps: Vec<String> = vec![];
        for a in v["apps"].as_array().into_iter().flatten() {
            let Some(a) = a.as_str().map(str::trim) else { return Err("apps are app names".into()) };
            if !mine.iter().any(|m| m == a) {
                return Err(format!("{a} is not one of the person's apps (they are: {})", if mine.is_empty() { "none".into() } else { mine.join(", ") }));
            }
            if !apps.iter().any(|x| x == a) {
                apps.push(a.into());
            }
        }
        if apps.is_empty() {
            return Err("apps are needed: one or more of the person's apps it covers".into());
        }
        let mut allow: Vec<String> = vec![];
        for a in v["allow"].as_array().into_iter().flatten() {
            match a.as_str() {
                Some(x) if LIFTABLE.contains(&x) => {
                    if !allow.iter().any(|y| y == x) {
                        allow.push(x.into());
                    }
                }
                _ => return Err(format!("allow lifts only send and irreversible, not {a}; spending money always asks")),
            }
        }
        let period = v["period"].as_str().filter(|p| period_days(p).is_some()).ok_or("period is day, week or month")?.to_string();
        let cap = self.cfg.budget.max_usd;
        let max_usd = v["max_usd"].as_f64().filter(|m| *m > 0.0 && *m <= cap).ok_or_else(|| format!("max_usd must be above 0 and at most the appliance's {cap}"))?;
        Ok(Fields { title: title.into(), apps, allow, period, max_usd })
    }

    /// Set a grant: the kernel writes its policy from the fields.
    pub async fn set_grant(&self, actor: &str, person: &str, id: &str, f: &Fields, card: Option<String>, label: Option<Label>) -> anyhow::Result<()> {
        let cedar = crate::policy::grant_cedar(person, &f.apps, &f.allow)?;
        let ev = Ev::GrantSet {
            id: id.into(),
            person: person.into(),
            title: f.title.clone(),
            apps: f.apps.clone(),
            allow: f.allow.clone(),
            period: f.period.clone(),
            max_usd: f.max_usd,
            card,
            cedar,
            label: label.filter(|l| l.untrusted || l.private),
        };
        self.emit(actor, ev).await.map(|_| ())
    }

    /// What "Handle things like this" on a task proposal starts from: its
    /// title, the person's apps its evidence read, nothing lifted, a week,
    /// and four times its estimate (at most the appliance's limit).
    pub(crate) fn handle_defaults(&self, person: &str, d: &Value) -> Value {
        let mine = self.person(person).map(|p| self.app_names(&p)).unwrap_or_default();
        let mut apps: Vec<String> = vec![];
        for e in d["evidence"].as_array().into_iter().flatten() {
            if let Some(a) = e["tool"].as_str().and_then(app_of).filter(|a| mine.iter().any(|m| m == a)) {
                if !apps.iter().any(|x| x == a) {
                    apps.push(a.into());
                }
            }
        }
        let cap = self.cfg.budget.max_usd;
        let max_usd = d["needs"]["estimated_cost_usd"].as_f64().map_or(cap, |e| (e * 4.0).min(cap));
        json!({ "title": d["title"], "apps": apps, "allow": [], "period": "week", "max_usd": max_usd })
    }

    /// The person's apps as a grant form offers them: name and title.
    pub(crate) fn grant_apps(&self, person: &str) -> Value {
        let Some(p) = self.person(person) else { return json!([]) };
        json!(self
            .apps_of(&p)
            .into_iter()
            .map(|(c, _)| json!({ "name": c.name, "title": c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name)) }))
            .collect::<Vec<_>>())
    }

    /// The person's own live grant and what is left of its budget this
    /// period, for a task to start under; or why not.
    pub fn grant_for_task(&self, person: &str, id: &str) -> Result<(Grant, f64), String> {
        let now = Utc::now();
        let found = self.read(|s| {
            s.grants.get(id).filter(|g| g.person == person).cloned().map(|g| {
                let spent = spent(s, &g, now);
                (g, spent)
            })
        });
        let Some((g, spent)) = found else { return Err(format!("{id} is not a grant of this person's")) };
        if g.revoked.is_some() {
            return Err(format!("{id} was revoked"));
        }
        let left = g.max_usd - spent;
        if left <= 0.0 {
            return Err(format!("{id}'s budget for this {} (${}) is spent; start the task without it, so what asks is asked", g.period, g.max_usd));
        }
        Ok((g, left))
    }

    /// A grant of `who`'s own, live.
    fn own_grant(&self, who: &Person, id: &str) -> Result<Grant, Refused> {
        let g = self.read(|s| s.grants.get(id).filter(|g| g.person == who.id).cloned()).ok_or(Refused::NotFound("grant"))?;
        if g.revoked.is_some() {
            return Err(Refused::Conflict("the grant was revoked".into()));
        }
        Ok(g)
    }

    /// Revoke a grant of `who`'s own: it covers nothing from now on.
    pub async fn revoke_grant(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        self.own_grant(who, id)?;
        self.emit(&format!("person:{}", who.id), Ev::GrantRevoked { id: id.into(), person: who.id.clone() }).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        Ok(json!({ "revoked": id }))
    }

    /// Edit a grant of `who`'s own: what `b` gives replaces what it had.
    pub async fn edit_grant(&self, who: &Person, id: &str, b: &Value) -> Result<Value, Refused> {
        let g = self.own_grant(who, id)?;
        let title = self.words_of(&g.person, g.seq).await.map_err(|e| Refused::Failed(format!("{e:#}")))?.str(g.seq, "title").to_string();
        let base = json!({ "title": title, "apps": g.apps, "allow": g.allow, "period": g.period, "max_usd": g.max_usd });
        let f = self.grant_fields(who, &over(&base, b)).map_err(Refused::Invalid)?;
        // A title they wrote is theirs; one kept is as it was.
        let label = if b["title"].as_str().is_some_and(|t| t.trim() != title) { None } else { Some(g.label) };
        self.set_grant(&format!("person:{}", who.id), &who.id, id, &f, None, label).await.map_err(|e| Refused::Failed(format!("{e:#}")))?;
        Ok(json!({ "id": id }))
    }

    /// The person's grants as the app lists them, live ones first: each
    /// with its policy, what it has spent this period and what is left,
    /// what still asks, and the actions taken under it (latest first).
    pub async fn grants_json(&self, person: &str) -> anyhow::Result<Value> {
        let now = Utc::now();
        let mut gs: Vec<(Grant, f64)> = self.read(|s| s.grants.values().filter(|g| g.person == person).map(|g| (g.clone(), spent(s, g, now))).collect());
        gs.sort_by_key(|(g, _)| (g.revoked.is_some(), std::cmp::Reverse(g.at)));
        let w = self.words(gs.iter().map(|(g, _)| (person.to_string(), g.seq))).await?;
        let titles: std::collections::HashMap<String, String> = {
            let tasks: Vec<crate::state::Task> = self.read(|s| gs.iter().flat_map(|(g, _)| g.actions.iter()).filter_map(|a| s.tasks.get(&a.task).cloned()).collect());
            let tw = self.words(tasks.iter().map(|t| (person.to_string(), t.seq))).await?;
            tasks.iter().map(|t| (t.task_id.clone(), crate::api::title_of(t, None, &tw))).collect()
        };
        let rows: Vec<Value> = gs
            .into_iter()
            .map(|(g, spent)| {
                let actions: Vec<Value> = g
                    .actions
                    .iter()
                    .rev()
                    .map(|a| json!({ "tool": a.tool, "task": a.task, "task_title": titles.get(&a.task), "run": a.run, "call_id": a.call_id, "at": a.at }))
                    .collect();
                json!({
                    "id": g.id, "title": w.str(g.seq, "title"), "apps": g.apps, "allow": g.allow, "still_asks": still_asks(&g.allow),
                    "period": g.period, "max_usd": g.max_usd, "spent": spent, "left": (g.max_usd - spent).max(0.0),
                    "cedar": g.cedar, "card": g.card, "at": g.at, "by": g.by, "edited": g.edited, "revoked": g.revoked, "actions": actions,
                })
            })
            .collect();
        Ok(json!({ "grants": rows, "apps": self.grant_apps(person), "max_usd": self.cfg.budget.max_usd }))
    }

    /// The section a chat turn's system context carries: the person's
    /// grants live when it started, so it starts matching work under one.
    /// A title whose words came from a run that read outside content is
    /// not shown.
    pub(crate) async fn grant_section(&self, run: &crate::state::Run) -> anyhow::Result<String> {
        let gs: Vec<Grant> =
            self.read(|s| s.grants.values().filter(|g| g.person == run.person && g.at < run.created_at && g.revoked.is_none_or(|r| r > run.created_at)).cloned().collect());
        if gs.is_empty() {
            return Ok(String::new());
        }
        let w = self.words(gs.iter().map(|g| (run.person.clone(), g.seq))).await?;
        let mut s = "\n\n# Work this person lets you do on your own\n\nTheir grants, each a standing authority over a kind of their own work. When they ask for work that fits one, start it with create_task and give the grant's id as grant: in that task, what their organisation's policy would ask about in the grant's apps goes ahead without asking, except what still asks. The kernel refuses a task under a grant whose budget for the period is spent. Never use one for work it does not cover. They change or revoke them in Settings, or by telling you (propose_grant with replaces, revoke_grant).\n\n".to_string();
        for g in &gs {
            let title = if g.label.untrusted || g.label.private {
                "(its title is not shown: it came from a run that read content from outside the conversation)".to_string()
            } else {
                format!("\"{}\"", w.str(g.seq, "title"))
            };
            s.push_str(&format!(
                "- {}: {title}; apps: {}; still asks before {}; up to ${} of model cost a {}\n",
                g.id,
                g.apps.join(", "),
                still_asks(&g.allow).join(", "),
                g.max_usd,
                g.period
            ));
        }
        Ok(s)
    }

    /// `propose_grant` from a chat turn, when the person asked for it: a
    /// card with the grant's fields, which the person accepts (with any
    /// changes of theirs) or declines. Nothing is granted until they accept.
    pub(crate) fn propose_grant(&self, a: &Action, key: &str) -> (String, bool, Option<Value>) {
        let no = |why: String| (format!("No card: {why}. A retry with the same values will not help."), true, None);
        let Some(run) = self.read(|s| s.runs.get(&a.run).filter(|r| s.tasks.get(&r.task).is_some_and(|t| t.is_chat_turn())).cloned()) else {
            return ("No card: propose_grant is for chat.".into(), true, None);
        };
        let Some(person) = self.person(&run.person) else { return no("no such person".into()) };
        let f = match self.grant_fields(&person, &a.input) {
            Ok(f) => f,
            Err(why) => return no(why),
        };
        let Some(why) = a.input["why"].as_str().map(str::trim).filter(|w| !w.is_empty()) else { return no("why is needed: what the person asked for".into()) };
        let replaces = a.input["replaces"].as_str().map(str::trim).filter(|r| !r.is_empty());
        if let Some(r) = replaces {
            if !self.read(|s| s.grants.get(r).is_some_and(|g| g.person == run.person && g.revoked.is_none())) {
                return no(format!("replaces names {r}, which is no live grant of this person's"));
            }
        }
        let card = crate::chat::derived("card", key);
        let data = json!({
            "card": card, "grant": true, "title": f.title, "apps": f.apps, "allow": f.allow, "still_asks": still_asks(&f.allow),
            "period": f.period, "max_usd": f.max_usd, "why": why, "replaces": replaces, "text": why,
        });
        (format!("A card for this grant ({card}) is in the thread. Nothing changes until the person accepts it."), false, Some(data))
    }

    /// `revoke_grant` from a chat turn: the person's own grant, at their word.
    pub(crate) async fn revoke_grant_tool(&self, run: &str, input: &Value) -> (String, bool, Option<Value>) {
        let person = self.read(|s| s.runs.get(run).map(|r| r.person.clone())).and_then(|p| self.person(&p));
        let Some(who) = person else { return ("no such person".into(), true, None) };
        match self.revoke_grant(&who, input["grant"].as_str().unwrap_or_default()).await {
            Ok(v) => (format!("Revoked {}: it covers nothing from now on.", v["revoked"].as_str().unwrap_or_default()), false, Some(v)),
            Err(e) => (e.to_string(), true, None),
        }
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/grants", get(list)).route("/v1/grants/{id}", put(edit).delete(revoke))
}

/// The person's own grants: no one else's, whoever asks, the owner too.
async fn list(State(k): State<Arc<Kernel>>, h: HeaderMap) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.grants_json(&p.id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn edit(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>, Json(b): Json<Value>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    answered(k.edit_grant(&p, &id, &b).await)
}

async fn revoke(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    answered(k.revoke_grant(&p, &id).await)
}

fn answered(r: Result<Value, Refused>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e @ Refused::NotFound(_)) => err(StatusCode::NOT_FOUND, e.to_string()),
        Err(Refused::Denied(m)) => err(StatusCode::FORBIDDEN, m),
        Err(Refused::Conflict(m)) => err(StatusCode::CONFLICT, m),
        Err(Refused::Invalid(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(Refused::Failed(m)) => err(StatusCode::INTERNAL_SERVER_ERROR, m),
    }
}
