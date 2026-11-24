//! Structured records (JSON) merged by record and field: an object by its
//! keys, a list of records (objects that each carry an `id`) by record, and
//! each record by its fields, so two people changing different line items,
//! or different fields of one, both land. The same field changed two ways
//! conflicts; so does any other value both changed (a plain list, a number).
//! A part is named by its path: `lines[l2].qty`.

use super::merge::{pick, Choices, Conflict, Edit, Keep, Merged};
use serde_json::{Map, Value};
use std::collections::HashMap;

fn parse(s: &str) -> Option<Value> {
    serde_json::from_str(s).ok()
}

/// A list's records by their `id`, when every item is one.
fn records(v: &Value) -> Option<Vec<(String, &Value)>> {
    let list = v.as_array()?;
    let ids: Option<Vec<(String, &Value)>> = list.iter().map(|r| Some((id_of(r)?, r))).collect();
    let ids = ids?;
    let mut seen = std::collections::BTreeSet::new();
    ids.iter().all(|(id, _)| seen.insert(id.clone())).then_some(ids)
}

fn id_of(r: &Value) -> Option<String> {
    match r.as_object()?.get("id")? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A list of records by id, so each is found in one step: a merge of
/// thousands of records stays linear.
fn index<'a>(list: &'a [(String, &'a Value)]) -> HashMap<&'a str, &'a Value> {
    list.iter().map(|(i, r)| (i.as_str(), *r)).collect()
}

fn key(path: &str, k: &str) -> String {
    if path.is_empty() {
        k.to_string()
    } else {
        format!("{path}.{k}")
    }
}

fn shown(v: Option<&Value>) -> Option<String> {
    v.map(|v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// The three-way merge of one part, `None` when it is absent from the result.
fn merge_at(path: &str, b: Option<&Value>, m: Option<&Value>, t: Option<&Value>, choices: &Choices, conflicts: &mut Vec<Conflict>) -> Option<Value> {
    if let Some(v) = pick(b, m, t) {
        return v.cloned();
    }
    let empty = Map::new();
    if let (Some(Value::Object(mo)), Some(Value::Object(to))) = (m, t) {
        let bo = b.and_then(Value::as_object).unwrap_or(&empty);
        let keys = to.keys().chain(mo.keys().filter(|k| !to.contains_key(*k))).chain(bo.keys().filter(|k| !to.contains_key(*k) && !mo.contains_key(*k)));
        let mut out = Map::new();
        for k in keys.cloned().collect::<Vec<_>>() {
            if let Some(v) = merge_at(&key(path, &k), bo.get(&k), mo.get(&k), to.get(&k), choices, conflicts) {
                out.insert(k, v);
            }
        }
        return Some(Value::Object(out));
    }
    if let (Some(mr), Some(tr)) = (m.and_then(records), t.and_then(records)) {
        let br = b.and_then(records).unwrap_or_default();
        let (bi, mi, ti) = (index(&br), index(&mr), index(&tr));
        let ids = tr.iter().map(|(i, _)| i).chain(mr.iter().filter(|(i, _)| !ti.contains_key(i.as_str())).map(|(i, _)| i));
        let ids: Vec<&String> = ids.chain(br.iter().filter(|(i, _)| !ti.contains_key(i.as_str()) && !mi.contains_key(i.as_str())).map(|(i, _)| i)).collect();
        let out: Vec<Value> = ids
            .into_iter()
            .filter_map(|id| merge_at(&format!("{path}[{id}]"), bi.get(id.as_str()).copied(), mi.get(id.as_str()).copied(), ti.get(id.as_str()).copied(), choices, conflicts))
            .collect();
        return Some(Value::Array(out));
    }
    let at = if path.is_empty() { super::merge::WHOLE_FILE.to_string() } else { path.to_string() };
    match choices.get(&at) {
        Some(Keep::Mine) => m.cloned(),
        Some(Keep::Theirs) => t.cloned(),
        None => {
            conflicts.push(Conflict { at, base: shown(b), mine: shown(m), theirs: shown(t) });
            t.cloned()
        }
    }
}

pub(super) fn merge(base: &str, mine: &str, theirs: &str, choices: &Choices) -> Merged {
    let (b, m, t) = (parse(base), parse(mine), parse(theirs));
    let mut conflicts = vec![];
    let merged = merge_at("", b.as_ref(), m.as_ref(), t.as_ref(), choices, &mut conflicts);
    if !conflicts.is_empty() {
        return Merged::Conflicts(conflicts);
    }
    let mut text = merged.map(|v| serde_json::to_string_pretty(&v).unwrap_or_default()).unwrap_or_default();
    text.push('\n');
    Merged::Clean(text.into_bytes())
}

fn diff_at(path: &str, b: Option<&Value>, a: Option<&Value>, out: &mut Vec<Edit>) {
    if b == a {
        return;
    }
    if let (Some(Value::Object(bo)), Some(Value::Object(ao))) = (b, a) {
        for k in ao.keys().chain(bo.keys().filter(|k| !ao.contains_key(*k))) {
            diff_at(&key(path, k), bo.get(k), ao.get(k), out);
        }
        return;
    }
    if let (Some(br), Some(ar)) = (b.and_then(records), a.and_then(records)) {
        let (bi, ai) = (index(&br), index(&ar));
        for (id, r) in &ar {
            diff_at(&format!("{path}[{id}]"), bi.get(id.as_str()).copied(), Some(r), out);
        }
        for (id, r) in br.iter().filter(|(i, _)| !ai.contains_key(i.as_str())) {
            diff_at(&format!("{path}[{id}]"), Some(r), None, out);
        }
        return;
    }
    let at = if path.is_empty() { super::merge::WHOLE_FILE.to_string() } else { path.to_string() };
    out.push(Edit { at, before: shown(b), after: shown(a) });
}

/// The fields `after` changed against `before`.
pub(super) fn diff(before: &str, after: &str) -> Vec<Edit> {
    let mut out = vec![];
    diff_at("", parse(before).as_ref(), parse(after).as_ref(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn quote(lines: Value) -> String {
        json!({ "customer": "Acme", "lines": lines }).to_string()
    }

    fn clean(m: Merged) -> Value {
        match m {
            Merged::Clean(b) => serde_json::from_slice(&b).unwrap(),
            Merged::Conflicts(c) => panic!("conflicted: {c:?}"),
        }
    }

    #[test]
    fn different_records_and_fields_both_land() {
        let base = quote(json!([{ "id": "l1", "item": "Bracket", "qty": 10, "price": 4 }, { "id": "l2", "item": "Bolt", "qty": 100, "price": 0.2 }]));
        let mine = quote(json!([{ "id": "l1", "item": "Bracket", "qty": 12, "price": 4 }, { "id": "l2", "item": "Bolt", "qty": 100, "price": 0.2 }]));
        let theirs = quote(
            json!([{ "id": "l1", "item": "Bracket", "qty": 10, "price": 4.5 }, { "id": "l2", "item": "Bolt", "qty": 100, "price": 0.2 }, { "id": "l3", "item": "Nut", "qty": 100, "price": 0.1 }]),
        );
        let got = clean(merge(&base, &mine, &theirs, &Choices::new()));
        assert_eq!(got["lines"][0], json!({ "id": "l1", "item": "Bracket", "qty": 12, "price": 4.5 }), "two fields of one record");
        assert_eq!(got["lines"][2]["id"], "l3", "a record added by one");
        assert_eq!(diff(&base, &mine), vec![Edit { at: "lines[l1].qty".into(), before: Some("10".into()), after: Some("12".into()) }]);
    }

    #[test]
    fn the_same_field_conflicts() {
        let base = quote(json!([{ "id": "l1", "qty": 10 }]));
        let mine = quote(json!([{ "id": "l1", "qty": 12 }]));
        let theirs = quote(json!([{ "id": "l1", "qty": 8 }]));
        let Merged::Conflicts(c) = merge(&base, &mine, &theirs, &Choices::new()) else { panic!("the same field merged") };
        assert_eq!(c, vec![Conflict { at: "lines[l1].qty".into(), base: Some("10".into()), mine: Some("12".into()), theirs: Some("8".into()) }]);
        let keep = Choices::from([("lines[l1].qty".to_string(), Keep::Theirs)]);
        assert_eq!(clean(merge(&base, &mine, &theirs, &keep))["lines"][0]["qty"], 8);
    }

    #[test]
    fn a_record_removed_by_one_and_changed_by_the_other_conflicts() {
        let base = quote(json!([{ "id": "l1", "qty": 10 }, { "id": "l2", "qty": 1 }]));
        let mine = quote(json!([{ "id": "l1", "qty": 10 }]));
        let theirs = quote(json!([{ "id": "l1", "qty": 10 }, { "id": "l2", "qty": 2 }]));
        assert!(matches!(merge(&base, &mine, &theirs, &Choices::new()), Merged::Conflicts(_)));
        // Removed by one, untouched by the other: it goes.
        let same = quote(json!([{ "id": "l1", "qty": 11 }, { "id": "l2", "qty": 1 }]));
        assert_eq!(clean(merge(&base, &mine, &same, &Choices::new()))["lines"], json!([{ "id": "l1", "qty": 11 }]));
    }

    /// Many records merge by id in linear time, not one search per record.
    #[test]
    fn many_records_merge_in_linear_time() {
        let list = |f: &dyn Fn(usize) -> usize| {
            serde_json::json!({ "lines": (0..50_000).map(|i| serde_json::json!({ "id": format!("l{i}"), "qty": f(i) })).collect::<Vec<_>>() }).to_string()
        };
        let (base, mine, theirs) = (list(&|_| 1), list(&|i| if i == 7 { 2 } else { 1 }), list(&|i| if i == 49_000 { 3 } else { 1 }));
        let started = std::time::Instant::now();
        let Merged::Clean(got) = merge(&base, &mine, &theirs, &Choices::new()) else { panic!("conflicted") };
        let got: Value = serde_json::from_slice(&got).unwrap();
        assert_eq!((got["lines"][7]["qty"].as_u64(), got["lines"][49_000]["qty"].as_u64()), (Some(2), Some(3)));
        assert_eq!(diff(&base, &mine).len(), 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }
}
