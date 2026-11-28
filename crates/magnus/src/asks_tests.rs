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

/// A payment asks every time: no Allow covers another. Private data to a
/// new destination after untrusted input (case 3) is covered for the rest of
/// the task only by an Allow given on a case-3 card, for the same
/// destinations (spec, How an ask looks: "Allow covers the rest of the
/// task"; owner, 2026-11-13: there was no Allow for the task).
#[test]
fn a_payment_asks_every_time_and_case_three_is_allowed_for_the_task() {
    let bob = to(&["bob@corp.example"]);
    let eve = to(&["eve@evil.example"]);
    assert!(!Reason::Payment.task_wide() && !Reason::Payment.always(), "a payment offered more than one action");
    let allow = approval("task", "t1", Some(OUTSIDE_REQUEST), None, Some(&["bob@corp.example"]));
    for r in [Reason::Payment, Reason::NewDestination] {
        assert!(!covers(&allow, &query(&bob, Some(r), "h")), "an Allow given for another reason covered {r:?}");
    }
    let given_on_payment = approval("task", "t1", Some(crate::payments::MONEY_RULE), None, Some(&["bob@corp.example"]));
    assert!(!covers(&given_on_payment, &query(&bob, None, "h")), "an approval given on a payment card covered the task");
    assert!(!covers(&given_on_payment, &query(&bob, Some(Reason::Payment), "h2")), "one payment's Allow covered another");
    assert!(Reason::NewDestination.task_wide() && Reason::NewDestination.always(), "case 3 offered no Allow for the task or Allow always");
    let task = approval("task", "t1", Some(NEW_DESTINATION), None, Some(&["bob@corp.example"]));
    assert!(covers(&task, &query(&bob, Some(Reason::NewDestination), "h2")), "Allow on a case-3 card did not cover the rest of the task");
    assert!(!covers(&task, &query(&eve, Some(Reason::NewDestination), "h2")), "Allow reached another destination");
    let once = approval("once", "t1", Some(NEW_DESTINATION), Some("h1"), Some(&["bob@corp.example"]));
    assert!(covers(&once, &query(&bob, Some(Reason::NewDestination), "h1")));
    assert!(!covers(&once, &query(&bob, Some(Reason::NewDestination), "h2")), "one approval covered a different send");
    let mut spent = once.clone();
    spent.used = true;
    assert!(!covers(&spent, &query(&bob, Some(Reason::NewDestination), "h1")), "a spent approval covered the action again");
}

/// Allow always saves a narrow rule: this person, this app's tool, these
/// destinations. It is offered wherever a rule can answer: an action Auto
/// Review asked about, and case 3.
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
    assert_eq!(Reason::ALL.iter().filter(|r| r.always()).collect::<Vec<_>>(), vec![&Reason::OutsideRequest, &Reason::NewDestination]);
}

/// Allow always on an action that reaches no one saves a rule for that tool
/// alone, and it covers only actions that reach no one: never a recipient,
/// so never case 3 (2026-11-28-gates-beyond-the-invariants).
#[test]
fn a_rule_with_no_destination_never_reaches_a_recipient() {
    let bare = always_rule("alice", "crm", "crm__update_record", &[]).unwrap();
    let named = always_rule("alice", "crm", "crm__update_record", &to(&["bob@corp.example"])).unwrap();
    let alice: Person = serde_json::from_value(json!({ "id": "alice", "groups": [] })).unwrap();
    let carol: Person = serde_json::from_value(json!({ "id": "carol", "groups": [] })).unwrap();
    let facts = |name| crate::policy::ToolFacts { name, app: "crm", risk: Risk::Irreversible, audience: Audience::Internal };
    let covers = |c: &crate::rules::Compiled, p: &Person, tool, d: &[&str]| crate::rules::allow_covers(c, p, &facts(tool), &to(d));
    assert!(covers(&bare, &alice, "crm__update_record", &[]), "the tool's own rule did not cover it");
    assert!(!covers(&bare, &alice, "crm__update_record", &["eve@evil.example"]), "a rule with no destination reached a recipient");
    assert!(!covers(&bare, &alice, "crm__delete_record", &[]), "the rule reached another tool");
    assert!(!covers(&bare, &carol, "crm__update_record", &[]), "the rule reached another person");
    assert!(!covers(&named, &alice, "crm__update_record", &[]), "a rule for named destinations covered an action with none");
    assert!(covers(&named, &alice, "crm__update_record", &["bob@corp.example"]));
    assert!(always_rule("alice", "mail", "mail__send", &to(&["   "])).is_err(), "Allow always saved a rule whose destinations were blank");
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
    assert!(is_boundary(&a(crate::computer::INSTALL_TOOL), &facts(crate::computer::INSTALL_TOOL, "computer", Risk::ReversibleWrite, false)), "an install was not reviewed");
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
    // Case 3: Allow for the rest of the task and Allow always, as on every card (spec, How an ask looks).
    assert_eq!((taint["offers"].clone(), taint["allow_scope"].clone()), (json!(["allow", "allow_always", "dont_allow"]), json!("task")));
    let mut bare = item(OUTSIDE_REQUEST, "irreversible", "alice");
    bare.details["ask_destinations"] = json!([]);
    bare.details["input"] = json!({ "record": "42", "stage": "closed" });
    assert_eq!(card(&s, &alice, &bare)["offers"], json!(["allow", "allow_always", "dont_allow"]), "an action with no destination offered no Allow always");
    let program = card(&s, &alice, &item(crate::desktop::ASK_RULE, "irreversible", "alice"));
    assert_eq!(program["offers"], json!(["allow", "allow_always", "dont_allow"]), "a program's code on the person's computer offered no Allow always");
    for own in [LOCAL_ASK_FIRST, &format!("{}r1", crate::rule_effects::PERSON_RULE)] {
        assert_eq!(card(&s, &alice, &item(own, "irreversible", "alice"))["offers"], json!(["allow", "dont_allow"]), "Allow always beat the person's own {own}");
    }
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

/// A chat app with no destinations set up goes to the channel its call
/// names, so a person's rule on that channel takes it in (2026-11-23).
#[test]
fn a_chat_apps_channel_is_where_it_goes() {
    let a = action("chat__post", json!({ "channel": "C0ABC123", "text": "hi" }));
    assert_eq!(destinations(&a, &facts("chat__post", "chat", Risk::ReversibleWrite, true), None), to(&["c0abc123"]));
}

/// Whether a rule naming `named` takes in an action going to `dests`, as
/// `Kernel::stopping_rule` matches: the sites above each site too.
fn blocks(person: &Person, f: &Facts, named: &str, dests: &[String]) -> bool {
    let rule = crate::rules::compile(&person.id, &f.app, "", &[named.to_string()], false, crate::rules::Effect::Block).unwrap();
    let facts = crate::policy::ToolFacts::from(f);
    with_parent_sites(dests).iter().any(|d| crate::policy::rule_covers(&rule.cedar, person, &facts, d).unwrap())
}

/// For every built-in tool and an app's tool, the destinations a card
/// names are the ones the person's rules match: a block naming any of
/// them, as the person would type it, stops the action, and an allow
/// naming them all covers it. Before, rules read only email-style fields,
/// so a block on a site never fired for a web fetch (2026-11-23).
#[test]
fn cards_and_rules_see_the_same_destinations() {
    let alice: Person = serde_json::from_value(json!({ "id": "alice", "groups": [] })).unwrap();
    let input = json!({ "url": "https://www.Site.example/p?q=1", "to": "Bob <Bob@Corp.example>", "channel": "#general", "host": "Data.example", "text": "hi" });
    let mut seen = 0;
    for b in crate::families::builtins() {
        let f = Facts { name: b.name.into(), app: b.app.into(), risk: b.risk, label: b.label, audience: b.audience, connector: None, remote: b.name.into(), reaches_self: false };
        let a = action(b.name, input.clone());
        let dests = destinations(&a, &f, None);
        for d in &dests {
            // As the person types it in Settings > Rules: a site's address, or the address itself.
            let typed = if matches!(b.app, "web" | "browser") { format!("https://{d}/") } else { d.to_uppercase() };
            assert!(blocks(&alice, &f, &typed, &dests), "{}: a block on {typed} did not stop what its card names ({dests:?})", b.name);
        }
        if !dests.is_empty() {
            seen += 1;
            let allow = crate::rules::compile("alice", &f.app, b.name, &dests, false, crate::rules::Effect::Allow).unwrap();
            let facts = crate::policy::ToolFacts::from(&f);
            assert!(dests.iter().all(|d| crate::policy::rule_covers(&allow.cedar, &alice, &facts, d).unwrap()), "{}: an allow on its card's {dests:?} did not cover it", b.name);
        }
    }
    assert!(seen > 10, "too few tools named where they go: {seen}");
    // A web fetch goes to its site, and a block on the site above it stops it.
    let web = facts("web_fetch", "web", Risk::Read, false);
    let fetch = destinations(&action("web_fetch", input.clone()), &web, None);
    assert_eq!(fetch, to(&["www.site.example"]));
    assert!(blocks(&alice, &web, "site.example", &fetch), "a block on site.example missed www.site.example");
    assert!(!blocks(&alice, &web, "other.example", &fetch));
    // An app's tool with no destinations set up: its addresses and its channel.
    let app = facts("chat__post", "chat", Risk::ReversibleWrite, true);
    let post = destinations(&action("chat__post", input), &app, None);
    assert_eq!(post, to(&["#general", "bob@corp.example"]));
    assert!(blocks(&alice, &app, "#General", &post), "a block on the channel did not stop the post");
}
