use crate::log::Event;
use crate::state::State;
use serde_json::{json, Value};

const ZED: &str = "zed-gone";
const AMY: &str = "amy-stays";
const AT: &str = "2026-11-13T09:00:00Z";

fn ev(seq: i64, actor: &str, ev: Value) -> Event {
    Event { seq, at: chrono::Utc::now(), actor: actor.into(), ev: serde_json::from_value(ev).unwrap() }
}

/// One record of every kind the state keeps per person, for `p`: their
/// task and run, and what each ledger records of them.
fn seed(s: &mut State, base: i64, p: &str) {
    let config = json!({ "adapter": "a", "model": "m", "max_retries": 0, "max_tokens": 1, "budget": { "max_model_calls": 1, "max_actions": 1, "max_usd": 1.0 }, "verifier": false, "instructions": "", "harness": "" });
    let actor = format!("person:{p}");
    let (task, run, item) = (format!("t-{p}"), format!("r-{p}"), format!("i-{p}"));
    let evs: Vec<(String, Value)> = vec![
        (actor.clone(), json!({ "kind": "task_created", "task": task, "run": run, "person": p, "text": "Read this.", "config": config })),
        (
            format!("outcome:{task}"),
            json!({ "kind": "task_created", "task": format!("{task}-turn"), "run": format!("{run}-turn"), "person": p, "text": "It ended.", "config": config }),
        ),
        ("kernel".into(), json!({ "kind": "item", "id": item, "item_kind": "notice", "task": task, "run": run, "person": p, "summary": "Look", "details": {} })),
        (actor.clone(), json!({ "kind": "signed_in", "person": p, "session": format!("s-{p}"), "client": "app" })),
        (actor.clone(), json!({ "kind": "moved", "item": task, "project": null })),
        (actor.clone(), json!({ "kind": "secret_set", "name": format!("connector_mail__{p}") })),
        (
            actor.clone(),
            json!({ "kind": "rule", "change": "proposed", "id": format!("rule-{p}"), "person": p, "compiled": null, "words": format!("{p}'s own words: never email my landlord") }),
        ),
        (
            actor.clone(),
            json!({ "kind": "payment", "change": "requested", "purchase": { "id": format!("buy-{p}"), "person": p, "run": run, "merchant_name": "Shop", "merchant_host": "shop.example", "amount_minor": 500, "currency": "usd", "rail": "link", "status": "waiting", "at": AT } }),
        ),
        (
            "kernel".into(),
            json!({ "kind": "evidence", "change": "read_back", "run": run, "call_id": "c1", "tool": "mail__send", "found": { "to": format!("{p}'s landlord") }, "note": "" }),
        ),
        ("kernel".into(), json!({ "kind": "sub_agent", "child": format!("{run}-turn"), "parent": run, "tools": [] })),
        (
            actor.clone(),
            json!({ "kind": "local_computer", "id": format!("pc-{p}"), "person": p, "name": format!("{p}'s laptop"), "folders": [format!("/home/{p}")], "mode": "ask" }),
        ),
        (actor.clone(), json!({ "kind": "local_file", "change": "written", "computer": format!("pc-{p}"), "person": p, "path": format!("/home/{p}/notes.txt"), "at": AT })),
        (
            actor.clone(),
            json!({ "kind": "local_file", "change": "deleted", "computer": format!("pc-{p}"), "person": p, "path": format!("/home/{p}/old.txt"), "at": AT, "by_person": true }),
        ),
        (actor.clone(), json!({ "kind": "desktop", "change": "device", "id": format!("dev-{p}"), "person": p, "name": format!("{p}'s mac") })),
        (actor.clone(), json!({ "kind": "team", "change": "member", "workspace": "ws", "person": p, "role": "member" })),
        (actor.clone(), json!({ "kind": "team", "change": "team_admin", "person": p, "on": true })),
        (actor.clone(), json!({ "kind": "team", "change": "chat", "workspace": "ws", "chat": { "what": "read", "channel": "general", "person": p, "at": AT } })),
        (
            actor.clone(),
            json!({ "kind": "team", "change": "chat", "workspace": "ws", "chat": { "what": "reacted", "channel": "general", "id": "m1", "emoji": format!("+{p}"), "person": p, "on": true } }),
        ),
        (
            actor.clone(),
            json!({ "kind": "team", "change": "whisper", "workspace": "ws", "channel": "general", "thread": null, "task": task, "owner": p, "history": format!("{p} whispered") }),
        ),
        (
            actor.clone(),
            json!({ "kind": "team", "change": "message", "workspace": "ws", "channel": "general", "message": { "id": format!("msg-{base}"), "sender": p, "text": format!("{p}'s door code"), "at": AT } }),
        ),
        (
            format!("run:{run}"),
            json!({ "kind": "team", "change": "message", "workspace": "ws", "channel": "general", "message": { "id": format!("post-{base}"), "sender": format!("agent:{p}"), "text": format!("posted for {p}"), "at": AT } }),
        ),
        (
            actor.clone(),
            json!({ "kind": "team", "change": "chat", "workspace": "ws", "chat": { "what": "edited", "channel": "general", "id": format!("msg-{base}"), "text": format!("{p}'s new door code"), "at": AT } }),
        ),
        (actor.clone(), json!({ "kind": "legal", "what": "terms_accepted", "person": p, "terms": "1", "privacy": "1" })),
        (actor.clone(), json!({ "kind": "legal", "what": "usage_chosen", "person": p, "share": true, "version": "1" })),
        ("kernel".into(), json!({ "kind": "push", "change": "received", "person": p, "app": "mail", "at": AT })),
        ("admin:root".into(), json!({ "kind": "halt", "change": "engaged", "scope": { "level": "person", "id": p }, "name": "Root" })),
        ("admin:root".into(), json!({ "kind": "halt", "change": "paused", "agent": format!("person:{p}"), "name": "Root" })),
        (actor.clone(), json!({ "kind": "tools", "change": "consented", "app": "mail", "person": p, "terms": "1" })),
        ("admin:root".into(), json!({ "kind": "spend", "change": "limit_set", "scope": { "person": p }, "usd": 5.0 })),
        (
            actor.clone(),
            json!({ "kind": "spend", "change": "increase_asked", "id": format!("more-{p}"), "person": p, "month": "2026-11", "usd": 5.0, "reason": format!("{p} needs it") }),
        ),
        ("admin:root".into(), json!({ "kind": "spend", "change": "models_allowed", "person": p, "models": ["a/m"] })),
        ("kernel".into(), json!({ "kind": "catalog", "change": "told", "id": "mail", "person": p, "item": item })),
        // Their arrangements, library, inbox marks, identity, conversations, watches and trash.
        (actor.clone(), json!({ "kind": "schedule_set", "id": format!("sc-{p}"), "person": p, "text": "Sum up.", "every_minutes": 60 })),
        (
            actor.clone(),
            json!({ "kind": "grant_set", "id": format!("g-{p}"), "person": p, "apps": ["mail"], "period": "day", "max_usd": 2.0, "card": format!("c-{p}"), "cedar": "permit(principal, action, resource);" }),
        ),
        ("kernel".into(), json!({ "kind": "commitment_recorded", "id": format!("cm-{p}"), "person": p, "who": format!("{p}'s landlord"), "by": "" })),
        (actor.clone(), json!({ "kind": "card_declined", "card": format!("cd-{p}"), "person": p, "reason": "not now" })),
        (actor.clone(), json!({ "kind": "file_uploaded", "id": format!("f-{p}"), "person": p, "name": format!("{p}.txt"), "media_type": "text/plain", "data_b64": "aGk=" })),
        (actor.clone(), json!({ "kind": "project_set", "id": format!("pr-{p}"), "person": p, "name": format!("{p}'s project") })),
        (actor.clone(), json!({ "kind": "project_set", "id": format!("pt-{p}"), "person": p, "name": format!("{p}'s trashed project") })),
        (actor.clone(), json!({ "kind": "trashed", "item": format!("pt-{p}"), "person": p })),
        (
            "model".into(),
            json!({ "kind": "memory", "id": format!("m-{p}"), "person": p, "run": run, "fact": format!("{p} likes tea"), "label": { "untrusted": false, "private": true }, "sources": [] }),
        ),
        (
            actor.clone(),
            json!({ "kind": "bundle_written", "path": format!("/users/{}/me.md", crate::bundle::user_segment(p)), "version": 1, "doc_kind": "relationship", "scope": "user", "owner_user": p, "actor": "magnus/1", "label": { "untrusted": false, "private": true }, "sources": [], "writer": p }),
        ),
        (actor.clone(), json!({ "kind": "item_opened", "id": item })),
        (actor.clone(), json!({ "kind": "approval", "id": format!("a-{p}"), "item": item, "person": p, "task": task, "tool": "mail__send", "scope": "task" })),
        ("kernel".into(), json!({ "kind": "review_woken", "run": format!("rv-{p}"), "task": format!("tv-{p}"), "person": p, "reason": "overnight" })),
        ("admin:root".into(), json!({ "kind": "subject_bound", "person": p, "subject": format!("sub-{p}") })),
        ("kernel".into(), json!({ "kind": "rights_from_provider", "person": p, "groups": ["staff"], "roles": ["member"] })),
        ("admin:root".into(), json!({ "kind": "roles_set", "person": p, "roles": ["member"] })),
        (actor.clone(), json!({ "kind": "preferences_set", "person": p, "prefs": { "timezone": "Europe/Paris" } })),
        (actor.clone(), json!({ "kind": "home_opened", "person": p })),
        (
            "kernel".into(),
            json!({ "kind": "task_created", "task": format!("t-{p}-open"), "run": format!("r-{p}-open"), "person": p, "text": "Hello.", "config": config, "chat": { "thread": format!("thread_{p}"), "turn": true, "opener": {} }, "attachments": [{ "name": format!("{p}.pdf") }] }),
        ),
        (
            actor.clone(),
            json!({ "kind": "task_created", "task": format!("t-{p}-gone"), "run": format!("r-{p}-gone"), "person": p, "text": "Gone.", "config": config, "chat": { "thread": format!("thread_{p}_gone"), "turn": true } }),
        ),
        (actor.clone(), json!({ "kind": "trashed", "item": format!("thread_{p}_gone"), "person": p })),
        ("kernel".into(), json!({ "kind": "removed", "item": format!("thread_{p}_gone"), "seqs": [] })),
        (actor.clone(), json!({ "kind": "chat_hidden", "person": p, "thread": format!("thread_{p}"), "entries": ["e1"] })),
        (actor.clone(), json!({ "kind": "reacted", "person": p, "thread": format!("thread_{p}"), "entry": "e1", "reaction": "great" })),
        ("kernel".into(), json!({ "kind": "disagreement_recorded", "id": format!("d-{p}"), "person": p, "run": run, "position": "wait", "decision": "send now" })),
        (actor.clone(), json!({ "kind": "thread_branched", "person": p, "thread": format!("thread_{p}_b"), "from": format!("thread_{p}"), "upto": "e1", "title": "B" })),
        ("kernel".into(), json!({ "kind": "watched_seen", "person": p, "app": "mail", "tool": "list", "ids": ["x1"], "full": true })),
        ("kernel".into(), json!({ "kind": "watch_polled", "person": p, "app": "mail", "tool": "list" })),
        (
            "kernel".into(),
            json!({ "kind": "draft_scoped", "run": run, "person": p, "draft": format!("dr-{p}"), "threads": [format!("thread_{p}")], "participants": [], "usage": { "input_tokens": 1, "output_tokens": 1, "cost_usd": 0.0 } }),
        ),
        // Last, so the run it stops is still going.
        (actor.clone(), json!({ "kind": "cancelled", "run": run })),
    ];
    for (i, (actor, e)) in evs.into_iter().enumerate() {
        s.apply(&ev(base + i as i64, &actor, e));
    }
}

/// Each per-person part of the state, as it prints.
fn parts(s: &State) -> Vec<(&'static str, String)> {
    vec![
        ("tasks", format!("{:?}", s.tasks)),
        ("runs", format!("{:?}", s.runs)),
        ("items", format!("{:?}", s.inbox.items())),
        ("identity", format!("{:?}", s.identity)),
        ("moved", format!("{:?}", s.library.moved())),
        ("vault", format!("{:?}", s.vault)),
        ("stopped_by_person", format!("{:?}", s.stopped_by_person)),
        ("rules", format!("{:?}", s.rules)),
        ("payments", format!("{:?}", s.payments)),
        ("evidence", format!("{:?}", s.evidence)),
        ("sub_agents", format!("{:?}", s.sub_agents)),
        ("outcomes", format!("{:?}", s.outcomes)),
        ("local", format!("{:?}", s.local)),
        ("team", format!("{:?}", s.team)),
        ("legal", format!("{:?}", s.legal)),
        ("push", format!("{:?}", s.push)),
        ("halts", format!("{:?}", s.halts)),
        ("tools", format!("{:?}", s.tools)),
        ("spend", format!("{:?}", s.spend)),
        ("catalog", format!("{:?}", s.catalog)),
        ("arrangements", format!("{:?}", s.arrangements)),
        ("library", format!("{:?}", s.library)),
        ("inbox", format!("{:?}", s.inbox)),
        ("thread_extras", format!("{:?}", s.thread_extras)),
        ("watch", format!("{:?}", s.watch)),
        ("trash", format!("{:?}", s.trash)),
        ("removed_threads", format!("{:?}", s.removed_threads)),
        ("openers", format!("{:?}", s.openers)),
        ("draft_sources", format!("{:?}", s.draft_sources)),
        ("attachments", format!("{:?}", s.attachments)),
    ]
}

/// Erasing a person leaves nothing in the state that names them, their
/// runs or their tasks: not a rule's words, an evidence read-back, a spend
/// request's reason, nor any ledger's record of them. Before, only the
/// core fields and push forgot, so the feature ledgers kept the person
/// (their rule's words in memory until the next start). Someone else's
/// records stay.
#[test]
fn erasing_a_person_leaves_nothing_of_theirs_in_any_part_of_the_state() {
    let mut s = State::default();
    s.apply(&ev(1, "person:amy", json!({ "kind": "team", "change": "created", "workspace": "ws", "name": "W", "owner": AMY })));
    s.apply(&ev(2, "person:amy", json!({ "kind": "team", "change": "channel", "workspace": "ws", "channel": "general", "name": "general" })));
    seed(&mut s, 100, ZED);
    seed(&mut s, 200, AMY);
    let before = parts(&s);
    for (name, part) in &before {
        assert!(part.contains(ZED), "the test seeded nothing of {ZED}'s in {name}");
    }
    s.apply(&ev(300, "admin:root", json!({ "kind": "person_erased", "person": ZED })));
    for ((name, part), (_, was)) in parts(&s).iter().zip(&before) {
        assert!(!part.contains(ZED), "{name} kept {ZED} after the erasure: {part}");
        assert!(part.contains(AMY) || !was.contains(AMY), "{name} lost {AMY}'s records with {ZED}'s");
    }
    let all = format!("{s:?}");
    assert!(!all.contains(ZED), "the state still names {ZED}");
}
