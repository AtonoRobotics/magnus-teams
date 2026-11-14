//! 6. A scheduled run uses sub-agents (owner, 2026-11-10; spec rev 140: "a
//! schedule fires with nobody present, uses sub-agents and posts its result
//! in its conversation"). Part of the autonomy suite, kept in its own file
//! for size: `cargo test -p magnus-scratch --test autonomy`.

use super::*;

/// Only an org policy: notes are changed only where a standing authority
/// (here, the confirmed schedule) covers it; everything else goes ahead.
const NOTES_ASK: &str = r#"
permit(principal, action == Action::"execute", resource)
  when { resource.risk != "reversible_write" || !(resource in App::"notes") };
permit(principal, action == Action::"request_approval", resource);
"#;

/// The reason, as the developer view shows it, of every allowed
/// `notes__save` in the person's tasks.
async fn saves_allowed(app: &Appliance, person: &Api) -> Vec<String> {
    let dev = app.api(magnus_scratch::DEVELOPER);
    let mut reasons = vec![];
    for t in visible_tasks(person).await {
        let id = t["task_id"].as_str().unwrap_or_default();
        let Some(run) = person.task(id).await["run_id"].as_str().map(String::from) else { continue };
        let (st, ins) = dev.get(&format!("/v1/dev/runs/{run}")).await;
        assert!(st.is_success(), "the run inspector refused: {st} {ins}");
        let actions = ins["actions"].as_array().cloned().unwrap_or_default();
        let decisions = actions.iter().filter(|a| a["tool"] == "notes__save").flat_map(|a| a["decisions"].as_array().cloned().unwrap_or_default());
        let allowed = decisions.filter(|d| d["outcome"] == "allow" && d["stage"] == "approval");
        reasons.extend(allowed.filter_map(|d| d["reason"].as_str().map(String::from)));
    }
    reasons
}

/// 6. A scheduled run uses sub-agents. The person sets up a schedule in a
/// conversation and leaves. Its run starts three sub-agents, for the
/// implementation, independent tests and a review, with no grant input:
/// each works under the run's authority, the scope the person confirmed
/// for the schedule, with no new approval and nothing of its own. The run
/// narrows the tests and the review to reading; the review's write is
/// refused, and so is a sub-agent given a tool the run lacks. The results
/// come back to the run, and its result and the agent's report post in the
/// conversation the schedule was set up in.
#[tokio::test(flavor = "multi_thread")]
async fn a_scheduled_run_uses_sub_agents() {
    const ASK: &str = "Every morning, update the release notes, with the implementation, independent tests and a review each done by a sub-agent.";
    const RUN: &str = "Update the release notes in my notes app, using sub-agents.";

    const IMPLEMENT: &str = "Implementation: write the release notes as a note titled Release notes.";
    const TESTS: &str = "Independent tests: check on your own that the release notes note is there and complete.";
    const REVIEW: &str = "Review: review the release notes note.";
    const OUTSIDE: &str = "Deploy: mark the release done in the CRM.";
    const SUMMARY: &str = "Release notes written, tested and reviewed by three sub-agents.";
    const REPORT: &str = "This morning's run updated the release notes: one sub-agent wrote them, one tested them and one reviewed them.";
    let note = || json!({ "title": "Release notes", "text": "1.4: faster sync." });
    let at = due_soon();
    let m = FakeModel::start(script(move |c| {
        let at = at.clone();
        async move {
            let checked = |goal: &str| plan(goal);
            let sub = |goal: &str, tools: &[&str]| tool("start_sub_agent", json!({ "goal": goal, "tools": tools }));
            let o = opening(&c);
            match run_of(&c) {
                Run::Chat => match c.turn() {
                    0 => tool("create_schedule", json!({ "title": "Release notes", "goal": RUN, "daily_at": at })),
                    _ => say("It is set up for every morning."),
                },
                Run::Task if o.contains(IMPLEMENT) => match c.turn() {
                    0 => checked("the release notes are written"),
                    1 => tool("notes__save", note()),
                    _ => complete("Wrote the release notes."),
                },
                Run::Task if o.contains(TESTS) => match c.turn() {
                    0 => checked("the release notes check out"),
                    1 => tool("notes__list", json!({})),
                    _ => complete("Tested: the release notes are there and complete."),
                },
                Run::Task if o.contains(REVIEW) => match c.turn() {
                    0 => checked("the release notes are reviewed"),
                    1 => tool("notes__save", json!({ "title": "Release notes", "text": "Reviewer's rewrite." })),
                    2 => tool("notes__list", json!({})),
                    _ => complete("Reviewed: the release notes read well."),
                },
                Run::Task if o.contains(RUN) => match c.turn() {
                    0 => checked("the release notes are updated"),
                    1 => sub(IMPLEMENT, &[]),
                    2 => sub(TESTS, &["notes__list"]),
                    3 => sub(REVIEW, &["notes__list"]),
                    4 => sub(OUTSIDE, &["crm__update_record"]),
                    _ => complete(SUMMARY),
                },
                Run::Outcome => say(REPORT),
                // Auto Review refuses every change: one the schedule covers
                // never reaches it, and one it does not cover stops here.
                Run::AutoReview => tool("review", json!({ "decision": "deny", "reason": "Nothing the person set up covers changing notes." })),
                _ => background(&c),
            }
        }
    }))
    .await;
    let notes = notes().start().await;
    let spec = proactive_on().app("notes", &notes, "private", "private", &[("save", "reversible_write"), ("list", "read")]);
    let app = Appliance::start(magnus_scratch::with_developer(spec), &m).await;
    let alice = app.api("alice");
    assert!(alice.put_policy("org", NOTES_ASK).await.is_success());
    let b = before(&alice).await;
    let origin = schedule_in_chat(&alice, ASK).await;
    until_outcome(&m).await;
    let e = in_origin(&m, &alice, &origin, REPORT).await;

    // The run's result and the agent's report are where the schedule was set up.
    let result = e.iter().find(|e| e["kind"] == "result").unwrap_or_else(|| panic!("no result where the schedule was set up: {e:?}"));
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("completed"), Some(SUMMARY)), "{result}\n{}", story(&m));
    assert!(!e.iter().any(|e| e.to_string().contains(OUTCOME)), "the server's turn text is shown to the person: {e:?}");

    // The run started its sub-agents with no grant: the tool takes none.
    let runs = m.requests();
    // The run itself: a sub-agent's opening carries the schedule's goal too, as its scope.
    let is_run = |c: &Conversation| run_of(c) == Run::Task && opening(c).contains(RUN) && ![IMPLEMENT, TESTS, REVIEW, OUTSIDE].iter().any(|g| opening(c).contains(g));
    let parent = |turn: usize| runs.iter().find(|c| is_run(c) && c.turn() == turn).expect("the run went no further").last_results();
    let schema = runs.iter().find(|c| is_run(c)).map(|c| c.body["tools"].clone()).unwrap_or_default();
    let spec = schema.as_array().into_iter().flatten().find(|t| t["name"] == "start_sub_agent").cloned().expect("start_sub_agent was not offered to the scheduled run");
    assert!(spec.to_string().contains("\"goal\"") && !spec.to_string().contains("\"grant\""), "start_sub_agent takes a grant: {spec}");
    assert!(m.requested_calls().iter().filter(|(n, _)| n == "start_sub_agent").all(|(_, i)| i.get("grant").is_none()));
    // Each sub-agent's result came back to the run.
    for (turn, said) in [(2, "Wrote the release notes."), (3, "Tested: the release notes are there"), (4, "Reviewed: the release notes read well.")] {
        let r = parent(turn);
        assert!(r.len() == 1 && !r[0].is_error && r[0].text.contains(said), "sub-agent result {turn} did not come back: {r:?}\n{}", story(&m));
    }
    // A sub-agent given a tool the run lacks is refused, and never starts.
    let r = parent(5);
    assert!(r.len() == 1 && r[0].is_error && r[0].text.contains("is not a tool you hold"), "a sub-agent was given a tool its parent lacks: {r:?}");
    assert!(!runs.iter().any(|c| opening(c).contains(OUTSIDE) && run_of(c) == Run::Task), "the refused sub-agent ran");

    // Each worked under what the person set the schedule up to do.
    for goal in [IMPLEMENT, TESTS, REVIEW] {
        let first = runs.iter().find(|c| run_of(c) == Run::Task && opening(c).contains(goal)).unwrap_or_else(|| panic!("no sub-agent for {goal}"));
        assert!(opening(first).contains(&format!("The person set up this schedule to do this; keep to it: {RUN}")), "a sub-agent lost the schedule's scope: {}", opening(first));
    }
    // The implementation changed the note under the schedule's confirmation:
    // no approval, and no grant of its own. A schedule answers for the person
    // on a change that sends nothing out, without Auto Review
    // (decisions/2026-11-10-permissions-asks.md, case 1), and Auto Review
    // here refuses everything, so the change went ahead on the schedule alone.
    assert_eq!(notes.effects("save"), 1, "the implementation's change did not go ahead, or the review's did");
    assert_eq!(notes.world()["notes"][0], note());
    assert!(!runs.iter().any(|c| run_of(c) == Run::AutoReview), "Auto Review judged a change the schedule covers:\n{}", story(&m));
    let saves = saves_allowed(&app, &alice).await;
    assert!(
        saves.len() == 1 && saves[0].starts_with("covered by the confirmed schedule sched_"),
        "the implementation's change was not allowed as covered by the schedule: {saves:?}\n{}",
        story(&m)
    );
    // The run narrowed the review to reading: its write was refused.
    let review = runs.iter().find(|c| run_of(c) == Run::Task && opening(c).contains(REVIEW) && c.turn() == 2).expect("the review went no further").last_results();
    assert!(review.len() == 1 && review[0].is_error && review[0].text.contains("notes__save"), "the review wrote past its limit: {review:?}");

    // The person sees the run and its three sub-agents under the schedule,
    // the conversation, and nothing else: no approval, notice or thread.
    let tasks = visible_tasks(&alice).await;
    assert_eq!(tasks.len(), 4, "the run and its three sub-agents: {tasks:?}");
    assert!(tasks.iter().all(|t| t["group"]["kind"] == "schedule"), "a sub-agent is not grouped under its schedule: {tasks:?}");
    let ids: Vec<String> = tasks.iter().map(|t| t["task_id"].as_str().unwrap().to_string()).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    only(&alice, &b, &refs, &[], &[&origin]).await;
}

/// 6, continued. A scheduled run that fails wakes the agent in the
/// conversation the schedule was set up in, through the outcome turn and its
/// retry limit; the person's inbox has the failure's notice, as any
/// scheduled run's failure does.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_scheduled_run_wakes_the_agent_where_it_was_set_up() {
    const ASK: &str = "Every morning, tell me what's new in my inbox.";
    const RUN: &str = "Summarise what is new in my inbox.";
    const REASON: &str = "The mail app answered with an error; the inbox could not be read.";
    const REPORT: &str = "This morning's inbox summary failed: the mail app could not be read. I'll try again tomorrow, or tell me if something changed.";
    let at = due_soon();
    let m = FakeModel::start(script(move |c| {
        let at = at.clone();
        async move {
            match run_of(&c) {
                Run::Chat => match c.turn() {
                    0 => tool("create_schedule", json!({ "title": "Morning inbox", "goal": RUN, "daily_at": at })),
                    _ => say("It is set up."),
                },
                Run::Task => match c.turn() {
                    0 => plan("summarise the inbox"),
                    _ => fail(REASON),
                },
                Run::Outcome => say(REPORT),
                _ => background(&c),
            }
        }
    }))
    .await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(proactive_on().app("mail", &mail, "public", "private", &[("list_inbox", "read")]), &m).await;
    let alice = app.api("alice");
    let b = before(&alice).await;
    let origin = schedule_in_chat(&alice, ASK).await;
    until_outcome(&m).await;
    let e = in_origin(&m, &alice, &origin, REPORT).await;

    let result = e.iter().find(|e| e["kind"] == "result").unwrap_or_else(|| panic!("no result where the schedule was set up: {e:?}"));
    assert_eq!((result["status"].as_str(), result["summary"].as_str()), (Some("failed"), Some(REASON)), "{result}");
    assert_eq!(e.iter().filter(|e| e["kind"] == "message").count(), 1, "the conversation shows a message nobody wrote: {e:?}");
    assert!(!e.iter().any(|e| e.to_string().contains(OUTCOME)), "the server's turn text is shown to the person: {e:?}");
    // The agent woke there, told of the failure and of its retries.
    let woke = m.requests().into_iter().find(|c| run_of(c) == Run::Outcome).expect("the agent was not woken");
    assert!(said(&woke).contains("failed") && said(&woke).contains("retry 1 of 2"), "the outcome turn is not the failure's, within the retry limit: {}", said(&woke));
    assert!(woke.text().contains(ASK), "the agent woke somewhere other than the conversation the schedule was set up in");
    let tasks = visible_tasks(&alice).await;
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    let run = tasks[0]["task_id"].as_str().unwrap().to_string();
    only(&alice, &b, &[&run], &["notice"], &[&origin]).await;
}
