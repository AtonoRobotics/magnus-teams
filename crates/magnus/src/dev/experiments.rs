//! Experiments and ablation trials in the developer view (V2).

use super::{config_key, failure, modules_of, verdict, K, MODULES};
use crate::anonymize::{Lens, Sight};
use crate::api::{err, internal, status_str};
use crate::api_error::{refuse, ApiResult};
use crate::events::ExperimentTag;
use crate::extract::{Body, Developer};
use crate::state::{Run, State, Status};
use axum::{
    extract::{Path, Query, State as AxState},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Start one trial of an experiment case, acting for the developer: a task,
/// or with `"via": "chat"` a chat message (in a new thread, or in `thread`),
/// on the appliance's model or the one in `model`. `modules` switches
/// removable modules on or off for a task trial (their ablation).
pub(super) async fn add_trial(AxState(k): K, Developer(p): Developer, Path(id): Path<String>, Body(b): Body<Value>) -> ApiResult {
    let text = b["text"].as_str().unwrap_or_default().trim().to_string();
    let (Some(case), Some(trial)) = (b["case"].as_str(), b["trial"].as_u64()) else { return refuse(StatusCode::BAD_REQUEST, "case and trial are required") };
    if text.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "the task has no text");
    }
    let model = crate::api::chosen_model(&k, &b).await?;
    let mut modules = BTreeMap::new();
    for (name, on) in b["modules"].as_object().into_iter().flatten() {
        let Some(on) = on.as_bool().filter(|_| MODULES.iter().any(|m| m.name == name)) else {
            return refuse(StatusCode::BAD_REQUEST, &format!("no removable module {name}, or not true/false"));
        };
        modules.insert(name.clone(), on);
    }
    if !modules.is_empty() && b["via"] == "chat" {
        return refuse(StatusCode::BAD_REQUEST, "module switches apply to task trials");
    }
    // The same trial may run once on each model and module setting, to compare them.
    let tag = ExperimentTag { id, case: case.into(), trial: trial as u32 };
    let on = model.clone().unwrap_or_else(|| k.default_model());
    let (mut want, _) = k.modules_for(&on.0, &on.1);
    want.extend(modules.clone());
    let dup = k.read(|s| {
        s.runs.values().any(|r| {
            r.experiment.as_ref() == Some(&tag)
                && (r.config.adapter.as_str(), r.config.model.as_str()) == (on.0.as_str(), on.1.as_str())
                && MODULES.iter().all(|m| r.config.module(m.name) == want[m.name])
        })
    });
    if dup {
        return refuse(StatusCode::CONFLICT, "this trial already exists");
    }
    if b["via"] == "chat" {
        return Ok(match k.chat_say(&p.id, text, b["thread"].as_str().map(String::from), model, Some(tag), vec![], None).await {
            Ok(Some((thread, task))) => (StatusCode::CREATED, Json(json!({ "thread": thread, "task_id": task }))).into_response(),
            Ok(None) => err(StatusCode::NOT_FOUND, "no such thread"),
            Err(e) => internal(e),
        });
    }
    let attachments = b["attachments"].as_array().cloned().unwrap_or_default();
    Ok(match k.create_tagged(&format!("person:{}", p.id), &p.id, text, attachments, Some(tag), model, modules).await {
        Ok((task, run)) => (StatusCode::CREATED, Json(json!({ "task_id": task, "run_id": run }))).into_response(),
        Err(e) => internal(e),
    })
}

fn choose(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (k - i) as f64)
}

/// pass@k: the chance that at least one of k trials passes (unbiased, from n
/// trials with c passes). pass^k: the chance that all k pass.
fn pass_at(n: u64, c: u64, k: u64) -> (f64, f64) {
    if n == 0 || k == 0 || k > n {
        return (f64::NAN, f64::NAN);
    }
    let at = 1.0 - choose(n - c, k) / choose(n, k);
    let hat = choose(c, k) / choose(n, k);
    (at, hat)
}

fn mean_se(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (m, f64::NAN);
    }
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

pub(crate) fn num(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 10000.0).round() / 10000.0)
    } else {
        Value::Null
    }
}

pub(super) async fn experiments(AxState(k): K, Developer(p): Developer) -> ApiResult {
    let lens = k.lens(&p);
    let list: Vec<Value> = k.read(|s| {
        let mut by: BTreeMap<String, Vec<&Run>> = BTreeMap::new();
        for r in s.runs.values().filter(|r| lens.sight(&r.person) != Sight::Hidden) {
            if let Some(t) = &r.experiment {
                by.entry(t.id.clone()).or_default().push(r);
            }
        }
        by.into_iter()
            .map(|(id, rs)| {
                let cases: BTreeSet<&str> = rs.iter().filter_map(|r| r.experiment.as_ref().map(|x| x.case.as_str())).collect();
                let configs: BTreeSet<String> = rs.iter().map(|r| config_key(r)).collect();
                let first = rs.iter().map(|r| r.created_at).min();
                let settled = rs.iter().filter(|r| r.status.terminal()).count();
                json!({ "id": id, "trials": rs.len(), "settled": settled, "cases": cases.len(), "configs": configs, "started": first })
            })
            .collect()
    });
    Ok(Json(json!({ "experiments": list })).into_response())
}

#[derive(serde::Deserialize, Default)]
pub(super) struct ExpQuery {
    k: Option<u64>,
    /// The earlier run of the same requests to compare with.
    base: Option<String>,
}

/// One experiment: each configuration across its repeated trials, case by
/// case, with pass@k and pass^k and their spread, a paired comparison, and
/// failures tagged by where they went wrong (V2). Every number lists its runs.
pub(super) async fn experiment(AxState(k): K, Developer(p): Developer, Path(id): Path<String>, Query(q): Query<ExpQuery>) -> ApiResult {
    let lens = k.lens(&p);
    let stall = k.cfg.observability.stall_minutes;
    let w = k.all_words().await?;
    let v = k.read(|s| {
        let rs: Vec<&Run> = s.runs.values().filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == id) && lens.sight(&r.person) != Sight::Hidden).collect();
        if rs.is_empty() {
            return None;
        }
        // config -> case -> runs
        let mut grid: BTreeMap<String, BTreeMap<String, Vec<&Run>>> = BTreeMap::new();
        for r in &rs {
            if let Some(exp) = &r.experiment {
                grid.entry(config_key(r)).or_default().entry(exp.case.clone()).or_default().push(r);
            }
        }
        let cases: BTreeSet<String> = rs.iter().filter_map(|r| r.experiment.as_ref().map(|x| x.case.clone())).collect();
        let settled = |r: &&&Run| r.status.terminal() || r.status == Status::Suspended;
        let kk = q.k.unwrap_or_else(|| {
            grid.values().flat_map(|c| c.values()).map(|v| v.iter().filter(settled).count() as u64).filter(|n| *n > 0).min().unwrap_or(1)
        });
        let mut configs = vec![];
        let mut rates: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        for (key, by_case) in &grid {
            let mut rows = vec![];
            let (mut at, mut hat, mut p1) = (vec![], vec![], vec![]);
            let mut tags: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            let mut basis = BTreeSet::new();
            for (case, trials) in by_case {
                let done: Vec<&&Run> = trials.iter().filter(settled).collect();
                let n = done.len() as u64;
                let passed: Vec<String> = done.iter().filter(|r| verdict(r).0).map(|r| r.id.clone()).collect();
                let c = passed.len() as u64;
                for r in &done {
                    basis.insert(verdict(r).1);
                    if !verdict(r).0 {
                        tags.entry(failure(s, r, stall).unwrap_or("grader")).or_default().push(r.id.clone());
                    }
                }
                let (a, hh) = pass_at(n, c, kk);
                if n > 0 {
                    at.push(a);
                    hat.push(hh);
                    p1.push(c as f64 / n as f64);
                    rates.entry(key.clone()).or_default().insert(case.clone(), c as f64 / n as f64);
                }
                rows.push(json!({
                    "case": case, "trials": trials.len(), "settled": n, "passed": c,
                    "pass_at_k": num(a), "pass_hat_k": num(hh),
                    "runs": trials.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), "passed_runs": passed,
                }));
            }
            let (m1, se1) = mean_se(&p1);
            let (ma, sea) = mean_se(&at);
            let (mh, seh) = mean_se(&hat);
            let Some(sample) = by_case.values().next().and_then(|v| v.first()) else { continue };
            configs.push(json!({
                "key": key,
                "config": { "adapter": sample.config.adapter, "model": sample.config.model, "harness": sample.config.harness, "instructions": sample.config.instructions, "playbook": sample.config.playbook, "modules": modules_of(sample) },
                "pass_rate": { "mean": num(m1), "se": num(se1) },
                "pass_at_k": { "mean": num(ma), "se": num(sea) },
                "pass_hat_k": { "mean": num(mh), "se": num(seh) },
                "verdict_basis": basis,
                "cases": rows,
                "failures": tags.into_iter().map(|(t, r)| json!({ "where": t, "count": r.len(), "runs": r })).collect::<Vec<_>>(),
            }));
        }
        // Paired, case by case, every configuration against the first.
        let keys: Vec<&String> = rates.keys().collect();
        let mut paired = vec![];
        if let Some(base) = keys.first() {
            for other in keys.iter().skip(1) {
                let (a, b) = (&rates[*base], &rates[*other]);
                let common: Vec<&String> = a.keys().filter(|c| b.contains_key(*c)).collect();
                let diffs: Vec<f64> = common.iter().map(|c| b[*c] - a[*c]).collect();
                let (m, se) = mean_se(&diffs);
                paired.push(json!({
                    "base": base, "other": other, "cases": common.len(),
                    "mean_difference": num(m), "se": num(se),
                    "better": diffs.iter().filter(|d| **d > 0.0).count(),
                    "worse": diffs.iter().filter(|d| **d < 0.0).count(),
                    "same": diffs.iter().filter(|d| **d == 0.0).count(),
                    "by_case": common.iter().zip(&diffs).map(|(c, d)| json!({ "case": c, "difference": num(*d) })).collect::<Vec<_>>(),
                }));
            }
        }
        let requests = request_rows(s, &id, &w, &lens);
        let compared = if requests.is_empty() { Value::Null } else { compare(s, &id, &requests, q.base.as_deref(), &w, &lens) };
        Some(json!({ "id": id, "k": kk, "cases": cases, "configs": configs, "paired": paired, "requests": requests, "compare": compared }))
    });
    Ok(match v {
        Some(v) => Json(v).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such experiment"),
    })
}

/// A chat trial and what it led to: what the model chose (answer, a
/// question back, tasks, a schedule), the tasks its message started, every
/// policy stop (ask or deny) on the way, and the cost and time of it all,
/// until each run first ended or stopped (for the person, or to wait), and
/// the person's verdict on the outcome.
fn request_trial(s: &State, r: &Run, w: &crate::recall::Words) -> Value {
    let kids: Vec<&Run> =
        s.tasks.values().filter(|t| t.chat.as_ref().is_some_and(|c| c.from_run.as_deref() == Some(r.id.as_str()))).filter_map(|t| s.runs.get(&t.run_id)).collect();
    let card = r.results.values().any(|res| res.data.as_ref().is_some_and(|d| d["card"].is_string()));
    let answer = r.summary.as_deref().unwrap_or_default().trim();
    let choice = if card {
        "schedule"
    } else if !kids.is_empty() {
        "task"
    } else if r.status != Status::Completed {
        status_str(r.status)
    } else if answer.ends_with('?') {
        "asked"
    } else {
        "answer"
    };
    let all: Vec<&Run> = std::iter::once(r).chain(kids.iter().copied()).collect();
    let stops: Vec<Value> = all
        .iter()
        .flat_map(|x| {
            x.decisions.iter().filter(|d| d.outcome != "allow").map(move |d| json!({ "run": x.id, "tool": d.tool, "outcome": d.outcome, "stage": d.stage, "rule": d.rule }))
        })
        .collect();
    let settled = all.iter().all(|x| x.status.terminal() || x.status == Status::Suspended);
    let end = all.iter().map(|x| x.first_stop.unwrap_or(x.last_at)).max().unwrap_or(r.created_at);
    let (case, trial) = r.experiment.as_ref().map(|t| (t.case.as_str(), t.trial)).unwrap_or_default();
    let (verdict, note, verdict_from) = request_verdict(s, r, &kids);
    json!({
        "case": case, "trial": trial, "config": config_key(r), "run": r.id, "task": r.task,
        "thread": s.tasks.get(&r.task).and_then(|t| t.chat.as_ref()).map(|c| c.thread.clone()),
        "text": s.tasks.get(&r.task).map(|t| w.text(t.seq).to_string()),
        "choice": choice, "answer": (!answer.is_empty()).then_some(answer),
        "tasks": kids.iter().map(|k| json!({
            "task": k.task, "title": s.tasks.get(&k.task).map(|t| crate::api::title_of(t, Some(k), w)),
            "status": status_str(k.status), "reason": k.reason, "stopped_for": k.suspensions,
        })).collect::<Vec<_>>(),
        "stops": stops,
        "usd": num(all.iter().map(|x| x.usd).sum()),
        "model_calls": all.iter().map(|x| x.model_calls).sum::<u32>(),
        "seconds": (end - r.created_at).num_milliseconds() as f64 / 1000.0,
        "settled": settled,
        "verdict": verdict, "note": note, "verdict_from": verdict_from,
        "model": format!("{}/{}", r.config.adapter, r.config.model),
        "setup": { "harness": r.config.harness, "instructions": r.config.instructions, "playbook": r.config.playbook },
    })
}

/// A request's verdict: the person's on the chat answer; where the message
/// started tasks and the answer has none, theirs once every task has one
/// (right only if all are right, wrong if any is wrong, else partly).
fn request_verdict(s: &State, r: &Run, kids: &[&Run]) -> (Option<String>, Option<String>, &'static str) {
    if let Some(v) = s.tasks.get(&r.task).and_then(|t| t.verdict.as_ref()) {
        return (Some(v.verdict.clone()), v.note.clone(), "answer");
    }
    let vs: Vec<&crate::state::Verdict> = kids.iter().filter_map(|k| s.tasks.get(&k.task).and_then(|t| t.verdict.as_ref())).collect();
    if kids.is_empty() || vs.len() < kids.len() {
        return (None, None, "none");
    }
    let worst = vs.iter().map(|v| rank(&v.verdict)).max().unwrap_or(0);
    let combined = if vs.iter().all(|v| v.verdict == "right") {
        "right"
    } else if worst == rank("wrong") {
        "wrong"
    } else {
        "partly"
    };
    let notes: Vec<String> = vs.iter().filter_map(|v| v.note.clone()).collect();
    (Some(combined.into()), (!notes.is_empty()).then(|| notes.join(" · ")), "tasks")
}

/// Right first: a lower rank is a better verdict.
fn rank(v: &str) -> usize {
    crate::state::VERDICTS.iter().position(|x| *x == v).unwrap_or(crate::state::VERDICTS.len())
}

/// The request trials of one experiment, in case, model and trial order.
fn request_rows(s: &State, id: &str, w: &crate::recall::Words, lens: &Lens) -> Vec<Value> {
    let mut rows: Vec<Value> = s
        .runs
        .values()
        .filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == id) && s.tasks.get(&r.task).is_some_and(|t| t.is_chat_turn()))
        .filter_map(|r| lens.show(lens.sight(&r.person), request_trial(s, r, w)))
        .collect();
    rows.sort_by_key(|v| (v["case"].as_str().unwrap_or_default().to_string(), v["config"].as_str().unwrap_or_default().to_string(), v["trial"].as_u64()));
    rows
}

/// This run of a request set against an earlier one (`base`, or else the
/// latest earlier experiment sharing a request): each request on each model,
/// side by side, what changed between them, and once every trial here has
/// settled, how many improved, stayed the same or regressed by verdict.
fn compare(s: &State, id: &str, rows: &[Value], base: Option<&str>, w: &crate::recall::Words, lens: &Lens) -> Value {
    let started = |e: &str| s.runs.values().filter(|r| r.experiment.as_ref().is_some_and(|t| t.id == e)).map(|r| r.created_at).min();
    let cases: BTreeSet<&str> = rows.iter().filter_map(|r| r["case"].as_str()).collect();
    let base_id = base.map(String::from).or_else(|| {
        let mine = started(id)?;
        let mut others: BTreeMap<String, chrono::DateTime<chrono::Utc>> = BTreeMap::new();
        for r in s.runs.values() {
            let Some(t) = r.experiment.as_ref() else { continue };
            if t.id != id
                && lens.sight(&r.person) != Sight::Hidden
                && cases.contains(t.case.as_str())
                && s.tasks.get(&r.task).is_some_and(|t| t.is_chat_turn())
                && r.created_at < mine
            {
                let e = others.entry(t.id.clone()).or_insert(r.created_at);
                *e = (*e).min(r.created_at);
            }
        }
        others.into_iter().max_by_key(|(_, at)| *at).map(|(e, _)| e)
    });
    let Some(base_id) = base_id else { return Value::Null };
    let before = request_rows(s, &base_id, w, lens);
    let key = |r: &Value| (r["case"].as_str().unwrap_or_default().to_string(), r["model"].as_str().unwrap_or_default().to_string(), r["trial"].as_u64());
    let earlier: BTreeMap<_, &Value> = before.iter().map(|r| (key(r), r)).collect();
    let (mut improved, mut unchanged, mut regressed, mut unjudged) = (0, 0, 0, 0);
    let pairs: Vec<Value> = rows
        .iter()
        .filter_map(|now| {
            let then = *earlier.get(&key(now))?;
            let mut changed = vec![];
            for f in ["verdict", "choice"] {
                if then[f] != now[f] {
                    changed.push(f);
                }
            }
            let stops = |r: &Value| -> Vec<String> {
                let mut v: Vec<String> = r["stops"].as_array().into_iter().flatten().map(|x| format!("{} {} {}", x["tool"], x["outcome"], x["rule"])).collect();
                v.sort();
                v
            };
            if stops(then) != stops(now) {
                changed.push("stops");
            }
            let tasks = |r: &Value| -> Vec<String> {
                let mut v: Vec<String> = r["tasks"].as_array().into_iter().flatten().map(|x| x["status"].as_str().unwrap_or_default().to_string()).collect();
                v.sort();
                v
            };
            if tasks(then) != tasks(now) {
                changed.push("tasks");
            }
            if then["setup"] != now["setup"] {
                changed.push("setup");
            }
            let outcome = match (then["verdict"].as_str(), now["verdict"].as_str()) {
                (Some(a), Some(b)) if rank(b) < rank(a) => {
                    improved += 1;
                    "improved"
                }
                (Some(a), Some(b)) if rank(b) > rank(a) => {
                    regressed += 1;
                    "regressed"
                }
                (Some(_), Some(_)) => {
                    unchanged += 1;
                    "unchanged"
                }
                _ => {
                    unjudged += 1;
                    "unjudged"
                }
            };
            let delta = |f: &str| match (then[f].as_f64(), now[f].as_f64()) {
                (Some(a), Some(b)) => num(b - a),
                _ => Value::Null,
            };
            Some(json!({
                "case": now["case"], "model": now["model"], "trial": now["trial"], "text": now["text"],
                "before": then, "after": now, "changed": changed, "outcome": outcome,
                "usd_change": delta("usd"), "seconds_change": delta("seconds"),
            }))
        })
        .collect();
    let settled = rows.iter().filter(|r| r["settled"] == true).count();
    json!({
        "base": base_id, "pairs": pairs, "settled": settled, "trials": rows.len(),
        "complete": settled == rows.len() && !rows.is_empty(),
        "summary": { "improved": improved, "unchanged": unchanged, "regressed": regressed, "unjudged": unjudged },
    })
}
