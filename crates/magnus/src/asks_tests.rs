use super::*;
use crate::boundary::Facts;
use crate::tools::{Audience, Label};

fn approval(scope: &str, task: &str, reason: Option<&str>, hash: Option<&str>, dests: Option<&[&str]>) -> Approval {
    Approval {
        id: "ap".into(),
        person: "alice".into(),
        task: task.into(),
        tool: "mail__send".into(),
        input_hash: hash.map(String::from),
        scope: scope.into(),
        until: None,
        used: false,
        by: "person:alice".into(),
        reason: reason.map(String::from),
        risk: Some("irreversible".into()),
        destinations: dests.map(|d| d.iter().map(|s| s.to_string()).collect()),
    }
}

fn query<'a>(dests: &'a [String], reason: Option<Reason>, hash: &'a str) -> Query<'a> {
    let tasks: &'static [String] = Box::leak(vec!["t1".to_string()].into_boxed_slice());
    Query { person: "alice", tasks, tool: "mail__send", risk: "irreversible", hash, reason, destinations: dests, now: chrono::Utc::now() }
}

fn to(d: &[&str]) -> Vec<String> {
    d.iter().map(|s| s.to_string()).collect()
}

/// Allow covers the rest of the task: later actions of the same kind to the
/// same destinations, and nothing else (spec rev 140, How an ask looks).
#[test]
fn allow_covers_the_task_by_kind_and_destinations() {
    let allow = approval("task", "t1", Some(OUTSIDE_REQUEST), Some("h1"), Some(&["bob@corp.example"]));
    let bob = to(&["bob@corp.example"]);
    assert!(covers(&allow, &query(&bob, Some(Reason::OutsideRequest), "h2")), "Allow did not cover a later send to the same person");
    assert!(covers(&allow, &query(&bob, None, "h3")), "Allow did not cover an unasked send of the same kind");
    assert!(!covers(&allow, &query(&to(&["eve@evil.example"]), Some(Reason::OutsideRequest), "h4")), "Allow covered another destination");
    assert!(!covers(&allow, &query(&to(&["bob@corp.example", "eve@evil.example"]), None, "h5")), "Allow covered an added destination");
    assert!(!covers(&allow, &query(&[], None, "h6")), "Allow for a destination covered an action with none");
    let other_task = approval("task", "t2", Some(OUTSIDE_REQUEST), None, Some(&["bob@corp.example"]));
    assert!(!covers(&other_task, &query(&bob, None, "h7")), "Allow reached another task");
    let mut other_tool = allow.clone();
    other_tool.tool = "mail__delete".into();
    assert!(!covers(&other_tool, &query(&bob, None, "h8")), "Allow reached another kind of action");
    let mut riskier = allow.clone();
    riskier.risk = Some("reversible_write".into());
    assert!(!covers(&riskier, &query(&bob, None, "h9")), "Allow covered the action once it was classed riskier");
}

/// A payment and private data to a new destination after untrusted input
/// ask every time: neither a task-wide Allow nor any other approval covers
/// them, only the approval of that very action, once.
#[test]
fn payments_and_the_taint_case_ask_every_time_after_allow() {
    let bob = to(&["bob@corp.example"]);
    let allow = approval("task", "t1", Some(OUTSIDE_REQUEST), None, Some(&["bob@corp.example"]));
    for r in [Reason::Payment, Reason::NewDestination] {
        assert!(!covers(&allow, &query(&bob, Some(r), "h")), "a task-wide Allow covered {r:?}");
        assert!(!r.task_wide() && !r.always(), "{r:?} offered more than one action");
    }
    let given_on_payment = approval("task", "t1", Some(crate::payments::MONEY_RULE), None, Some(&["bob@corp.example"]));
    assert!(!covers(&given_on_payment, &query(&bob, None, "h")), "an approval given on a payment card covered the task");
    let once = approval("once", "t1", Some(NEW_DESTINATION), Some("h1"), Some(&["bob@corp.example"]));
    assert!(covers(&once, &query(&bob, Some(Reason::NewDestination), "h1")));
    assert!(!covers(&once, &query(&bob, Some(Reason::NewDestination), "h2")), "one approval covered a different send");
    let mut spent = once.clone();
    spent.used = true;
    assert!(!covers(&spent, &query(&bob, Some(Reason::NewDestination), "h1")), "a spent approval covered the action again");
}

/// Allow always saves a narrow rule: this person, this app's tool, these
/// destinations. It is offered only for an action Auto Review asked about.
#[test]
fn allow_always_makes_a_narrow_rule() {
    let rule = always_rule("alice", "mail", "mail__send", &to(&["Bob <bob@corp.example>"])).unwrap();
    assert_eq!((rule.effect, rule.once, rule.destinations.clone()), (crate::rules::Effect::Allow, false, to(&["bob@corp.example"])));
    let person = |id: &str| -> Person { serde_json::from_value(json!({ "id": id, "groups": [] })).unwrap() };
    let facts = |name| crate::policy::ToolFacts { name, app: "mail", risk: Risk::Irreversible, audience: Audience::Public };
    let covers = |p: &str, tool, d: &str| crate::policy::rule_covers(&rule.cedar, &person(p), &facts(tool), d).unwrap();
    assert!(covers("alice", "mail__send", "bob@corp.example"));
    assert!(!covers("alice", "mail__send", "eve@evil.example"), "the rule reached another destination");
    assert!(!covers("alice", "mail__delete", "bob@corp.example"), "the rule reached another tool");
    assert!(!covers("carol", "mail__send", "bob@corp.example"), "the rule reached another person");
    assert!(always_rule("alice", "mail", "mail__send", &[]).is_err(), "Allow always saved a rule with no destination");
    assert_eq!(Reason::ALL.iter().filter(|r| r.always()).collect::<Vec<_>>(), vec![&Reason::OutsideRequest]);
}

/// A rule answers case 1 and case 3 as its person's approval; never a
/// payment, and never over the person's own ask-first rule.
#[test]
fn a_rule_never_answers_a_payment_or_an_ask_first() {
    assert!(Reason::OutsideRequest.rule_answers());
    assert!(Reason::NewDestination.rule_answers(), "case 3 asks unless a rule covers it");
    assert!(!Reason::Payment.rule_answers(), "a rule answered a payment");
    assert!(!Reason::AskFirst.rule_answers(), "an allow rule beat ask first");
}

/// Don't allow stops that action and tells the agent to carry on.
#[test]
fn dont_allow_tells_the_agent_to_carry_on_without_it() {
    let said = declined(Some("not this week"));
    assert!(said.contains("did not allow") && said.contains("not this week") && said.contains("Carry on"), "{said}");
}

/// Exactly four asks exist. Every rule the kernel raises maps to one of
/// them, every retired ask maps to none, and no source file but this one
/// makes an approval card.
#[test]
fn no_ask_outside_the_four_can_be_produced() {
    let four = [
        (OUTSIDE_REQUEST, Reason::OutsideRequest),
        (crate::boundary::REVIEWER_FLAG, Reason::OutsideRequest),
        (crate::payments::MONEY_RULE, Reason::Payment),
        (crate::payments::RULE, Reason::Payment),
        (NEW_DESTINATION, Reason::NewDestination),
        (LOCAL_ASK_FIRST, Reason::AskFirst),
        ("person_rule:rule_1", Reason::AskFirst),
    ];
    for (rule, reason) in four {
        assert_eq!(Reason::of(Some(rule)), Some(reason), "{rule}");
    }
    for retired in ["kernel:audience", "kernel:web_lookups", "kernel:local_change", "new_source", "unknown_outcome", "org:ask", "default:ask"] {
        assert_eq!(Reason::of(Some(retired)), None, "{retired} still asks");
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name == "asks.rs" || name.ends_with("_tests.rs") || name == "measured.rs" || name == "state.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("item_kind: \"approval\""), "{} makes an approval card outside asks.rs", path.display());
            // These are never made again, though an old item is still read;
            // nor is a task's criteria put to the person (checking is internal).
            for gone in [
                "item_kind: \"contract_change\"",
                "\"contract_change\".into()",
                "suspend_on(rid, \"review\"",
                "\"kernel:audience\"",
                "\"kernel:web_lookups\"",
                "\"kernel:local_change\"",
                "\"unknown_outcome\"",
            ] {
                assert!(!text.contains(gone), "{} still raises {gone}", path.display());
            }
        }
    }
}

fn facts(tool: &str, app: &str, risk: Risk, connector: bool) -> Facts {
    let c: Option<crate::config::Connector> =
        connector.then(|| serde_json::from_value(json!({ "name": app, "transport": "http", "url": "http://x", "audience": "public", "label": "private" })).unwrap());
    Facts { name: tool.into(), app: app.into(), risk, label: Label::PRINCIPAL, audience: Audience::Public, connector: c, remote: tool.into(), reaches_self: false }
}

fn action(tool: &str, input: Value) -> Action {
    Action { run: "r".into(), call_id: "c".into(), step_id: "s".into(), tool: tool.into(), input, issuer: Issuer::Model }
}

/// Reads and everything inside the computer pass without review; writes,
/// sends, payments, installs, sources and the person's own desktop do not.
#[test]
fn only_boundary_actions_are_reviewed() {
    let a = |t: &str| action(t, json!({}));
    assert!(is_boundary(&a("mail__send"), &facts("mail__send", "mail", Risk::Irreversible, true)));
    assert!(!is_boundary(&a("mail__list"), &facts("mail__list", "mail", Risk::Read, true)), "a read was a boundary action");
    assert!(is_boundary(&a("purchase_fill"), &facts("purchase_fill", "payments", Risk::Financial, false)));
    assert!(is_boundary(&a(crate::rule_effects::SOURCE_TOOL), &facts(crate::rule_effects::SOURCE_TOOL, "computer", Risk::ReversibleWrite, false)));
    assert!(is_boundary(&a("local_delete"), &facts("local_delete", "local", Risk::Irreversible, false)));
    assert!(!is_boundary(&a("computer_click"), &facts("computer_click", "sandbox", Risk::Sandbox, false)), "the computer's inside was reviewed");
    assert!(!is_boundary(&a("browser_click"), &facts("browser_click", "browser", Risk::ReversibleWrite, false)), "the computer's browser was reviewed");
    assert!(!is_boundary(&a("create_task"), &facts("create_task", "chat", Risk::ReversibleWrite, false)), "starting a task was reviewed");
    assert!(!is_boundary(&a("start_sub_agent"), &facts("start_sub_agent", "sandbox", Risk::Sandbox, false)), "a sub-agent was reviewed");
    assert!(!is_boundary(&a("memory_write"), &facts("memory_write", "memory", Risk::ReversibleWrite, false)));
}

fn item(rule: &str, risk: &str, person: &str) -> Item {
    Item {
        id: "i1".into(),
        kind: "approval".into(),
        task: None,
        run: None,
        person: person.into(),
        summary: String::new(),
        details: json!({
            "tool": "mail__send", "app": "mail", "app_title": "Mail", "risk": risk, "rule": rule,
            "why": "Auto Review: the person never named eve", "taint": { "untrusted": true, "private": true },
            "input": { "to": "eve@evil.example", "body": "hi" }, "ask_destinations": ["eve@evil.example"],
        }),
        created_at: chrono::Utc::now(),
        resolution: None,
    }
}

/// The card is one sentence from structured fields, three answers for the
/// person whose it is, and none of the reasoning.
#[test]
fn the_card_is_one_sentence_three_answers_and_no_reasoning() {
    let s = State::default();
    let alice: Person = serde_json::from_value(json!({ "id": "alice", "groups": [] })).unwrap();
    let bob: Person = serde_json::from_value(json!({ "id": "bob", "groups": [] })).unwrap();
    let c = card(&s, &alice, &item(OUTSIDE_REQUEST, "irreversible", "alice"));
    assert_eq!(c["sentence"], "Magnus wants to send in Mail, to eve@evil.example.");
    assert_eq!(c["offers"], json!(["allow", "allow_always", "dont_allow"]));
    assert_eq!(c["allow_scope"], "task");
    let text = c.to_string();
    for word in ["Auto Review", "taint", "untrusted", "rule", "policy", "never named"] {
        assert!(!text.contains(word), "the card carries {word}: {text}");
    }
    assert_eq!(card(&s, &bob, &item(OUTSIDE_REQUEST, "irreversible", "alice"))["offers"], json!(["allow", "dont_allow"]), "Allow always was offered for someone else");
    let pay = card(&s, &alice, &item(crate::payments::MONEY_RULE, "financial", "alice"));
    assert_eq!((pay["offers"].clone(), pay["allow_scope"].clone()), (json!(["allow", "dont_allow"]), json!("once")));
    let taint = card(&s, &alice, &item(NEW_DESTINATION, "irreversible", "alice"));
    assert_eq!((taint["offers"].clone(), taint["allow_scope"].clone()), (json!(["allow", "dont_allow"]), json!("once")));
    let shown = without_reasoning(&item(OUTSIDE_REQUEST, "irreversible", "alice").details);
    for k in REASONING_FIELDS {
        assert!(shown.get(k).is_none(), "{k} stayed on the user's item");
    }
    assert_eq!(shown["input"]["to"], "eve@evil.example", "the exact content is not one tap away");
}

/// A saved card's ask says on its card that it is the saved card, what it
/// costs and where: the rule and the reason that said so are the developer's.
#[test]
fn a_saved_cards_card_says_the_charge() {
    let s = State::default();
    let alice: Person = serde_json::from_value(json!({ "id": "alice", "groups": [] })).unwrap();
    let mut i = item(crate::payments::RULE, "financial", "alice");
    i.details["tool"] = json!("purchase_fill");
    i.details["app"] = json!("payments");
    i.details["charge"] = json!({ "saved_card": true, "amount": "USD 35.05", "merchant": "Paper Shop" });
    let c = card(&s, &alice, &i);
    assert_eq!(c["sentence"], "Magnus wants to charge your saved card USD 35.05 at Paper Shop.");
    assert_eq!((c["offers"].clone(), c["allow_scope"].clone()), (json!(["allow", "dont_allow"]), json!("once")), "a saved card was allowed for more than one fill");
    assert_eq!(without_reasoning(&i.details)["charge"]["saved_card"], true, "the item does not say it is the saved card");
}

#[test]
fn a_sentence_names_what_and_where() {
    assert_eq!(sentence("purchase_fill", "payments", None, "financial", &[]), "Magnus wants to make a payment (purchase fill) with your payment method.");
    assert_eq!(sentence(crate::rule_effects::SOURCE_TOOL, "computer", None, "reversible_write", &to(&["data.example"])), "Magnus wants to let its computer reach data.example.");
    assert_eq!(sentence("web_fetch", "web", None, "read", &to(&["collect.example"])), "Magnus wants to look up collect.example on the web.");
    assert_eq!(
        sentence("crm__update_record", "crm", None, "reversible_write", &to(&["a@x", "b@x", "c@x", "d@x"])),
        "Magnus wants to update record in Crm, to a@x, b@x, c@x and 1 more."
    );
}

/// An allowed new destination is remembered for its person.
#[test]
fn an_approved_destination_is_remembered() {
    let given = vec![approval("once", "t1", Some(NEW_DESTINATION), Some("h"), Some(&["bob@corp.example"]))];
    assert!(remembered(&given, "alice", "BOB@corp.example"));
    assert!(!remembered(&given, "alice", "eve@evil.example"));
    assert!(!remembered(&given, "carol", "bob@corp.example"), "one person's approval remembered for another");
    let outside = vec![approval("task", "t1", Some(OUTSIDE_REQUEST), None, Some(&["eve@evil.example"]))];
    assert!(!remembered(&outside, "alice", "eve@evil.example"), "an Allow on another ask released private data");
}

/// A web lookup's destination is its host.
#[test]
fn a_lookup_goes_to_its_host() {
    let a = action("web_fetch", json!({ "url": "https://Collect.Example/x?d=41.7M" }));
    assert_eq!(destinations(&a, &facts("web_fetch", "web", Risk::Read, false), None), to(&["collect.example"]));
}

/// An app with no destinations set up still has the recipients its call
/// names, so its card says where it goes and Allow always can be offered.
#[test]
fn a_send_without_destinations_set_up_goes_to_whom_it_names() {
    let a = action("mail__send", json!({ "to": "Bob <Bob@Corp.example>", "cc": ["carol@corp.example"], "body": "ask eve@x.example" }));
    assert_eq!(destinations(&a, &facts("mail__send", "mail", Risk::ReversibleWrite, true), None), to(&["bob@corp.example", "carol@corp.example"]));
}

/// A sub-agent holds what was approved for the runs above it: its query
/// carries its parents' tasks too (decisions/2026-11-10-sub-agents-carry-parent-authority.md).
#[test]
fn a_sub_agent_holds_its_parents_allow() {
    let dests = to(&["bob@corp.example"]);
    let parent = approval("task", "t1", Some(OUTSIDE_REQUEST), None, Some(&["bob@corp.example"]));
    let lineage = vec!["child".to_string(), "t1".to_string()];
    let q = Query { tasks: &lineage, ..query(&dests, Some(Reason::OutsideRequest), "h9") };
    assert!(covers(&parent, &q), "a sub-agent did not hold its parent's Allow");
    let alone = vec!["child".to_string()];
    assert!(!covers(&parent, &Query { tasks: &alone, ..query(&dests, Some(Reason::OutsideRequest), "h9") }), "an unrelated task held the Allow");
}
