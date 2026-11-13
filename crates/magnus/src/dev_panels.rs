//! The developer view's panels (V3), policy history and playbook (V5, I2),
//! moved out of `dev` to keep it in size. Each shows only the runs and
//! people its viewer's lens allows (`crate::anonymize`).

use crate::anonymize::{Lens, Sight};
use crate::api::developer;
use crate::dev::{failure, num};
use crate::kernel::Kernel;
use crate::state::{Run, Status};
use axum::{
    extract::State as AxState,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

type K = AxState<Arc<Kernel>>;

macro_rules! dev {
    ($k:expr, $h:expr) => {
        match developer(&$k, &$h).await {
            Ok(p) => p,
            Err(r) => return r,
        }
    };
}

/// The ids of the runs `lens` shows at all, and of those it hides.
pub(crate) fn runs_seen<'a>(lens: &Lens, s: &'a crate::state::State) -> (BTreeSet<&'a str>, BTreeSet<String>) {
    let (seen, hidden): (Vec<&Run>, Vec<&Run>) = s.runs.values().partition(|r| lens.run_sight(&r.id, &r.person) != Sight::Hidden);
    (seen.iter().map(|r| r.id.as_str()).collect(), hidden.iter().map(|r| r.id.clone()).collect())
}

fn panel(id: &str, title: &str, value: Value, unit: &str, runs: Vec<String>, breakdown: Vec<Value>) -> Value {
    json!({ "id": id, "title": title, "value": value, "unit": unit, "runs": runs, "breakdown": breakdown })
}

fn rows(m: BTreeMap<String, BTreeSet<String>>) -> Vec<Value> {
    m.into_iter().map(|(label, runs)| json!({ "label": label, "value": runs.len(), "runs": runs })).collect()
}

/// Live panels (V3). Every number lists the runs behind it (V4).
pub(crate) async fn metrics(AxState(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let stall = k.cfg.observability.stall_minutes;
    let capture = k.capture.lock().unwrap().clone();
    let (mut v, hidden) = k.read(|s| {
        // Only the runs of people the viewer may see count, even as numbers.
        let (seen, hidden) = runs_seen(&lens, s);
        let all: Vec<&Run> = s.runs.values().filter(|r| seen.contains(r.id.as_str())).collect();
        let ids = |f: &dyn Fn(&Run) -> bool| -> Vec<String> { all.iter().filter(|r| f(r)).map(|r| r.id.clone()).collect() };
        let mut panels = vec![];

        // Interventions per task: decisions a person had to make.
        let mut by_kind: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut interventions = 0usize;
        for i in s.items.values() {
            if let (Some(run), true) = (&i.run, ["approval", "question", "review", "notice"].contains(&i.kind.as_str()) && i.run.as_deref().is_some_and(|r| seen.contains(r))) {
                interventions += 1;
                by_kind.entry(i.kind.clone()).or_default().insert(run.clone());
            }
        }
        let n = all.len().max(1) as f64;
        panels.push(panel("interventions", "Interventions per task", num(interventions as f64 / n), "per task", ids(&|_| true), rows(by_kind)));

        let done = ids(&|r| r.status == Status::Completed);
        let spend: f64 = all.iter().map(|r| r.usd).sum();
        let cps = if done.is_empty() { f64::NAN } else { spend / done.len() as f64 };
        panels.push(panel(
            "cost_per_success",
            "Cost per successful task",
            num(cps),
            "USD",
            done.clone(),
            vec![json!({ "label": "total spend", "value": num(spend), "runs": ids(&|r| r.usd > 0.0) })],
        ));

        let mut outcomes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut asks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in &all {
            for d in &r.decisions {
                outcomes.entry(format!("{} · {}", d.stage, d.outcome)).or_default().insert(r.id.clone());
                if d.outcome == "ask" {
                    let reason = crate::asks::Reason::of(d.rule.as_deref()).map_or("before rev 140", crate::asks::Reason::as_str);
                    asks.entry(reason.to_string()).or_default().insert(r.id.clone());
                }
            }
        }
        let decisions: usize = all.iter().map(|r| r.decisions.len()).sum();
        panels.push(panel("boundary", "Boundary outcomes by stage", json!(decisions), "decisions", ids(&|r| !r.decisions.is_empty()), rows(outcomes)));
        let ask_runs: BTreeSet<String> = asks.values().flatten().cloned().collect();
        panels.push(panel("asks", "Asks by reason", json!(ask_runs.len()), "runs", ask_runs.into_iter().collect(), rows(asks)));

        let mut scopes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut granted = 0usize;
        for a in &s.approvals {
            let run = s.tasks.get(&a.task).map(|t| t.run_id.clone()).unwrap_or_default();
            if !seen.contains(run.as_str()) {
                continue;
            }
            granted += 1;
            scopes.entry(a.scope.clone()).or_default().insert(run);
        }
        panels.push(panel(
            "approval_scopes",
            "Approval scopes granted",
            json!(granted),
            "approvals",
            scopes.values().flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect(),
            rows(scopes),
        ));

        let unknown = ids(&|r| r.intents.values().any(|i| i.result.as_ref().is_some_and(|x| x.status == "unknown")));
        panels.push(panel("unknown_intents", "Intents with unknown outcome", json!(unknown.len()), "runs", unknown, vec![]));

        let budget = ids(&|r| r.suspensions.iter().any(|x| x == "budget"));
        panels.push(panel("budget_hits", "Budget hits", json!(budget.len()), "runs", budget, vec![]));

        let stalled = ids(&|r| failure(s, r, stall) == Some("stalled"));
        panels.push(panel("stalled", &format!("Stalled runs (no event for {stall} min)"), json!(stalled.len()), "runs", stalled, vec![]));

        let t = all.iter().fold((0u64, 0u64), |acc, r| (acc.0 + r.tokens.cache_read, acc.1 + r.tokens.input + r.tokens.cache_read + r.tokens.cache_write));
        let rate = if t.1 == 0 { f64::NAN } else { t.0 as f64 / t.1 as f64 };
        panels.push(panel("cache_hit_rate", "Prompt cache hit rate", num(rate), "of input tokens", ids(&|r| r.tokens.input > 0), vec![]));

        let mut mem: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let memory: Vec<_> = s.memory.iter().filter(|m| seen.contains(m.run.as_str())).collect();
        for m in &memory {
            let l = match (m.label.untrusted, m.label.private) {
                (true, true) => "private+untrusted",
                (true, false) => "untrusted",
                (false, true) => "private",
                _ => "principal",
            };
            mem.entry(l.into()).or_default().insert(m.run.clone());
        }
        panels.push(panel(
            "memory_writes",
            "Memory writes by label",
            json!(memory.len()),
            "entries",
            mem.values().flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect(),
            rows(mem),
        ));

        let mut fail: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for r in &all {
            if let Some(f) = failure(s, r, stall) {
                fail.entry(f.into()).or_default().insert(r.id.clone());
            }
        }
        panels.push(panel(
            "failures",
            "Where runs went wrong",
            json!(fail.values().map(|v| v.len()).sum::<usize>()),
            "runs",
            fail.values().flatten().cloned().collect::<BTreeSet<_>>().into_iter().collect(),
            rows(fail),
        ));

        let reviews = ids(&|r| s.items.values().any(|i| i.kind == "review" && i.run.as_deref() == Some(&r.id)));
        panels.push(panel("review_load", "Review cards (criteria without a check)", json!(reviews.len()), "runs", reviews, vec![]));

        let mut recent: Vec<u64> = capture.recent.iter().copied().collect();
        recent.sort_unstable();
        let p95 = recent.get(((recent.len() as f64) * 0.95) as usize).or(recent.last()).copied().unwrap_or(0);
        let avg = if capture.appends == 0 { f64::NAN } else { capture.total_us as f64 / capture.appends as f64 / 1000.0 };
        panels.push(json!({ "id": "capture", "title": "Capture overhead per step (log append)", "value": num(avg), "unit": "ms average since boot",
            "runs": [], "breakdown": [
                { "label": "p95 of recent appends", "value": num(p95 as f64 / 1000.0), "runs": [] },
                { "label": "slowest append", "value": num(capture.max_us as f64 / 1000.0), "runs": [] },
                { "label": "appends since boot", "value": capture.appends, "runs": [] }
            ] }));

        panels.extend(crate::measured::panels(s, &|r| seen.contains(r)));
        (json!({ "panels": panels, "runs": all.len(), "at": chrono::Utc::now() }), hidden)
    });
    crate::anonymize::scrub(&mut v, &hidden);
    Json(v).into_response()
}

/// A person's id as `lens` shows it: itself, or a pseudonym.
fn named(lens: &Lens, id: &str) -> String {
    let bare = id.strip_prefix("person:").unwrap_or(id);
    if lens.sight(bare) == Sight::Full {
        id.to_string()
    } else {
        lens.pseudonym(id)
    }
}

/// How `lens` sees a policy layer: a person's own layer as they allow, the
/// organisation's and groups' as they are, since they are settings, not
/// anyone's work.
fn layer_sight(lens: &Lens, layer: &str) -> Sight {
    layer.strip_prefix("user/").map_or(Sight::Full, |u| lens.sight(u))
}

/// Policy, versioned per layer, with the agent's proposals waiting for the
/// owner of each layer (A8, K35) and each tool's measured reliability (U5).
pub(crate) async fn policy(AxState(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let people = k.cfg.people.clone();
    let (mut v, hidden) = k.read(|s| {
        let (seen, hidden) = runs_seen(&lens, s);
        let mut layers: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for p in &s.policy_history {
            layers.entry(p.layer.clone()).or_default().push(json!({ "seq": p.seq, "at": p.at, "by": named(&lens, &p.by), "cedar": p.cedar }));
        }
        layers.entry("org".into()).or_default();
        let layers: Vec<Value> = layers
            .into_iter()
            .filter_map(|(layer, versions)| {
                let owners: Vec<String> = people.iter().filter(|p| crate::inbox::may_set_policy(p, &layer)).map(|p| named(&lens, &p.id)).collect();
                let current = s.policies.get(&layer).cloned().or_else(|| (layer == "org").then(|| crate::policy::DEFAULT_ORG.to_string()));
                let v = json!({ "layer": layer, "versions": versions, "current": current, "default": layer == "org" && !s.policies.contains_key("org"), "owners": owners });
                match layer_sight(&lens, &layer) {
                    Sight::Full => Some(v),
                    Sight::Anonymous => {
                        let mut v = crate::anonymize::shape(&lens, &v);
                        v["layer"] = json!(format!("user/{}", lens.pseudonym(layer.trim_start_matches("user/"))));
                        Some(v)
                    }
                    Sight::Hidden => None,
                }
            })
            .collect();
        let proposals: Vec<Value> = s
            .items
            .values()
            .filter(|i| i.kind == "policy_proposal")
            .filter_map(|i| {
                let layer = i.details["layer"].as_str().unwrap_or_default();
                let v = json!({
                    "id": i.id, "layer": layer, "cedar": i.details["cedar"], "rationale": i.details["rationale"],
                    "run": i.run, "created_at": i.created_at,
                    "status": if i.resolution.is_some() { "decided" } else { "waiting" },
                    "decision": i.resolution.as_ref().map(|r| json!({ "decision": r.0, "by": named(&lens, &r.3) })),
                    "waiting_for": people.iter().filter(|p| crate::inbox::may_set_policy(p, layer)).map(|p| named(&lens, &p.id)).collect::<Vec<_>>(),
                });
                lens.show(lens.sight(&i.person), v)
            })
            .collect();
        // Reliability per tool: how calls under each scope were decided and
        // how they and their runs turned out.
        #[derive(Default)]
        struct Rel {
            allow: BTreeSet<String>,
            ask: BTreeSet<String>,
            deny: BTreeSet<String>,
            calls: u64,
            errors: u64,
            runs: BTreeSet<String>,
            completed: BTreeSet<String>,
        }
        let mut rel: BTreeMap<String, Rel> = BTreeMap::new();
        for r in s.runs.values().filter(|r| seen.contains(r.id.as_str())) {
            let mut last: BTreeMap<&str, &crate::state::DecisionRec> = BTreeMap::new();
            for d in &r.decisions {
                last.insert(&d.call_id, d);
            }
            for d in last.values() {
                let e = rel.entry(d.tool.clone()).or_default();
                match d.outcome.as_str() {
                    "allow" | "dedupe" => e.allow.insert(r.id.clone()),
                    "ask" => e.ask.insert(r.id.clone()),
                    _ => e.deny.insert(r.id.clone()),
                };
            }
            for i in r.intents.values() {
                let e = rel.entry(i.tool.clone()).or_default();
                e.calls += 1;
                if i.result.as_ref().is_some_and(|x| x.is_error) {
                    e.errors += 1;
                }
                e.runs.insert(r.id.clone());
                if r.status == Status::Completed {
                    e.completed.insert(r.id.clone());
                }
            }
        }
        let scopes: Vec<Value> = rel
            .into_iter()
            .map(|(tool, r)| {
                let ok = if r.calls == 0 { f64::NAN } else { 1.0 - r.errors as f64 / r.calls as f64 };
                let done = if r.runs.is_empty() { f64::NAN } else { r.completed.len() as f64 / r.runs.len() as f64 };
                json!({
                    "tool": tool,
                    "decided": { "allow": r.allow.len(), "ask": r.ask.len(), "deny": r.deny.len() },
                    "calls": r.calls, "errors": r.errors, "call_success": num(ok),
                    "runs": r.runs, "runs_completed": num(done),
                })
            })
            .collect();
        (json!({ "layers": layers, "proposals": proposals, "scopes": scopes }), hidden)
    });
    crate::anonymize::scrub(&mut v, &hidden);
    Json(v).into_response()
}

/// The agent's playbook items with their measurements (I2, I3): a person's
/// items as they allow, the rest not at all.
pub(crate) async fn playbook(AxState(k): K, h: HeaderMap) -> Response {
    let p = dev!(k, h);
    let lens = k.lens(&p);
    let v = k.read(|s| {
        let items: Vec<Value> = s
            .playbook
            .values()
            .filter_map(|i| {
                let sight = lens.sight(&i.person);
                if sight == Sight::Hidden {
                    return None;
                }
                let mine: Vec<&Run> = s.runs.values().filter(|r| r.person == i.person && r.status.terminal()).collect();
                let live = |r: &&&Run| r.config.playbook >= i.added && i.retired.is_none_or(|x| r.config.playbook < x);
                let with: Vec<String> = mine.iter().filter(live).map(|r| r.id.clone()).collect();
                let before: Vec<String> = mine.iter().filter(|r| r.config.playbook < i.added).map(|r| r.id.clone()).collect();
                let rate = |ids: &[String]| {
                    let done = ids.iter().filter(|id| s.runs.get(*id).is_some_and(|r| r.status == Status::Completed)).count();
                    json!({ "runs": ids, "completed": done, "rate": num(if ids.is_empty() { f64::NAN } else { done as f64 / ids.len() as f64 }) })
                };
                lens.show(
                    sight,
                    json!({
                        "id": i.id, "person": i.person, "text": i.text, "addresses": i.addresses, "label": i.label,
                        "source_run": i.runs.first(), "edited_by": i.runs.iter().skip(1).collect::<Vec<_>>(),
                        "added": i.added, "retired": i.retired, "added_at": i.added_at,
                        "with_item": rate(&with), "before_item": rate(&before),
                    }),
                )
            })
            .collect();
        json!({ "version": s.playbook_version, "items": items })
    });
    Json(v).into_response()
}
