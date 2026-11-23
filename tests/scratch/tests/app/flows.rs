//! TLS, plan/view, developer view, experiments, playbook, versions and memory tests
//! for the desktop-app kernel API, kept in a submodule for file size.

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn plain_http_on_the_https_port_is_sent_to_https() {
    let mut f = magnus_scratch::Flow::start().await;
    let (url, _) = f.with_tls().await;
    let addr = url.trim_start_matches("https://").to_string();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(&addr).await.unwrap();
    s.write_all(format!("GET /#/inbox HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).await.unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 308"), "{reply:?}");
    assert!(reply.contains(&format!("Location: https://{addr}/#/inbox\r\n")), "{reply:?}");
    // HTTPS on the same port still works.
    let me = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap().get(format!("{url}/v1/sign-in")).send().await.unwrap();
    assert_eq!(me.status(), 200);
}

/// A browser offers HTTP/2 first; whatever the appliance agrees to, it answers.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_prefers_http2_is_answered_over_https() {
    let mut f = magnus_scratch::Flow::start().await;
    let (url, _) = f.with_tls().await;
    let browser = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let r = browser.get(format!("{url}/v1/health")).send().await.expect("an answer over HTTPS");
    assert_eq!(r.status(), 200);
    // The appliance offers only HTTP/1.1. A build of the whole workspace
    // can serve h2 (the scratch client's features unify into the server's
    // hyper), so an answer alone would not catch h2 offered again; the
    // protocol it agreed to does.
    assert_eq!(r.version(), reqwest::Version::HTTP_11, "the appliance agreed to HTTP/2");
}

/// The whole operator flow: plan, no review card, and an outcome with its
/// composition and evidence. (It had a policy-asked approval before spec
/// rev 140; asks are covered in asks.rs.)
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_a_review_and_the_outcome_carries_its_view() {
    let model = FakeModel::start(script(|c| async move {
        let last = c.last_results();
        match c.turn() {
            0 => plan("note the figure"),
            1 => tool(
                "task_plan",
                json!({ "units": [
                { "id": "u1", "goal": "remember the figure", "done_when": "memory has it", "status": "active" },
                { "id": "u2", "goal": "report", "done_when": "summary sent" }
            ] }),
            ),
            2 => tool("memory_write", json!({ "fact": "Q3 revenue is 41.7M", "topic": "finance" })),
            3 => tool("task_complete", json!({ "summary": "Remembered.", "deliverable": [{ "type": "nonsense" }] })),
            4 => {
                assert!(last.iter().any(|r| r.is_error && r.text.contains("unknown type")), "a bad view was not refused: {:?}", last.iter().map(|r| &r.text).collect::<Vec<_>>());
                tool(
                    "task_complete",
                    json!({
                        "summary": "Remembered the Q3 figure.",
                        "deliverable": [
                            { "type": "stat", "label": "Q3 revenue", "value": "41.7M" },
                            { "type": "bar_chart", "labels": ["Q2", "Q3"], "values": [38.2, 41.7], "unit": "M" }
                        ],
                        "spoken": "Q3 revenue was 41.7 million, up from 38.2."
                    }),
                )
            }
            _ => fail("unexpected turn"),
        }
    }))
    .await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let id = alice.submit("Remember that Q3 revenue was 41.7M.").await;
    // Writing its own memory is inside Magnus: nothing asks (spec rev 140;
    // a policy's ask is no longer an ask), and checking is internal: the
    // run completes with nothing put to the person.
    let done = alice.wait_status(&id, &["completed"]).await;
    assert_eq!(done["outcome_view"][0]["type"], "stat");
    assert_eq!(done["spoken"], "Q3 revenue was 41.7 million, up from 38.2.");

    let (_, tl) = alice.get(&format!("/v1/tasks/{id}/timeline")).await;
    let calls: Vec<&Value> = tl["steps"].as_array().unwrap().iter().flat_map(|s| s["calls"].as_array().unwrap()).collect();
    let mw = calls.iter().find(|c| c["tool"] == "memory_write").expect("memory_write in the timeline");
    assert_eq!(mw["issuer"], "model");
    assert!(mw["decisions"].as_array().unwrap().iter().all(|d| d["outcome"] == "allow"), "{mw}");
    assert!(mw["decisions"].as_array().unwrap().iter().all(|d| d["rule"].is_null() && d["reason"].is_null()), "the task view shows a decision's reasoning: {mw}");
}

/// The inspector compares each run's instructions with the build's own: the
/// chat instructions for a chat turn, the task instructions for a task. It
/// said every chat turn ran on instructions other than the build's.
#[tokio::test(flavor = "multi_thread")]
async fn the_inspector_matches_chat_and_task_runs_to_their_own_instructions() {
    let s = script(|c| async move {
        if c.body["system"].to_string().contains("in a chat") {
            return Reply::Text("Hello.".into());
        }
        match c.turn() {
            0 => plan("x"),
            _ => fail("nothing to do"),
        }
    });
    let model = FakeModel::start(s).await;
    let app = Appliance::start(spec(), &model).await;
    let dana = app.api("dana");
    let task = dana.submit("Look something up.").await;
    let task_run = dana.wait_status(&task, &["failed"]).await["run_id"].as_str().unwrap().to_string();
    let (st, v) = dana.post("/v1/chat", json!({ "text": "Hi" })).await;
    assert_eq!(st, 201, "{v}");
    let chat_run = dana.wait_status(v["task_id"].as_str().unwrap(), &["completed"]).await["run_id"].as_str().unwrap().to_string();
    for (what, run, file) in [("task", task_run, "agent.md@"), ("chat turn", chat_run, "chat.md@")] {
        let (_, ins) = dana.get(&format!("/v1/dev/runs/{run}")).await;
        let ctx = &ins["steps"][0]["context"];
        assert!(ctx["instructions"].as_str().unwrap().starts_with(file), "{what}: {ctx}");
        assert_eq!(ctx["instructions_match_build"], true, "the {what} is said to run on other instructions: {ctx}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_developer_view_needs_the_role_and_drills_down_to_the_log() {
    let model = FakeModel::start(steps(vec![plan("x"), tool("memory_search", json!({ "query": "anything" })), fail("nothing to do")])).await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let dana = app.api("dana");
    let id = alice.submit("Look something up.").await;
    let task = alice.wait_status(&id, &["failed"]).await;
    let run = task["run_id"].as_str().unwrap().to_string();

    for path in ["/v1/dev/runs", "/v1/dev/metrics", "/v1/dev/experiments", "/v1/dev/policy", "/v1/dev/playbook", &format!("/v1/dev/runs/{run}/log")] {
        assert_eq!(alice.get(path).await.0, 403, "{path} is open to a person without the developer role");
        assert_eq!(dana.get(path).await.0, 200, "{path}");
    }

    let (_, runs) = dana.get("/v1/dev/runs").await;
    let row = runs["runs"].as_array().unwrap().iter().find(|r| r["run"] == run.as_str()).expect("the run is listed");
    assert_eq!(row["failure"], "agent");

    let (_, ins) = dana.get(&format!("/v1/dev/runs/{run}")).await;
    let steps = ins["steps"].as_array().unwrap();
    assert!(steps.len() >= 3, "{ins}");
    let sources = steps[0]["context"]["sources"].as_array().unwrap();
    for s in ["tool definitions", "instructions", "task"] {
        assert!(sources.iter().any(|x| x["source"] == s && x["tokens"].as_u64().is_some()), "no {s} in {sources:?}");
    }
    let later = steps[2]["context"]["sources"].as_array().unwrap();
    assert!(later.iter().any(|x| x["source"] == "tool results: memory_search"), "results are not attributed: {later:?}");
    let searches: Vec<&Value> = ins["actions"].as_array().unwrap().iter().filter(|a| a["tool"] == "memory_search").collect();
    assert_eq!(searches.len(), 1, "one call is one action");
    let search = searches[0];
    assert!(search["decisions"].as_array().unwrap().iter().any(|d| d["stage"] == "policy"));
    assert_eq!(search["issuer"], "model");
    assert!(search["result"]["text"].is_string(), "{}", serde_json::to_string_pretty(&ins).unwrap());
    assert!(ins["harness"].as_array().unwrap().iter().any(|h| h["kind"] == "failed"));

    let (_, log) = dana.get(&format!("/v1/dev/runs/{run}/log")).await;
    assert_eq!(log["events"].as_array().unwrap().len() as u64, log["total"].as_u64().unwrap());
    assert!(log["events"].as_array().unwrap().iter().any(|e| e["kind"] == "task_created"));

    // Every number drills down to runs.
    let (_, m) = dana.get("/v1/dev/metrics").await;
    let failures = m["panels"].as_array().unwrap().iter().find(|p| p["id"] == "failures").unwrap();
    assert!(failures["runs"].as_array().unwrap().iter().any(|r| r == run.as_str()));
    let capture = m["panels"].as_array().unwrap().iter().find(|p| p["id"] == "capture").unwrap();
    assert!(capture["value"].as_f64().is_some(), "capture overhead is not measured: {capture}");
}

#[tokio::test(flavor = "multi_thread")]
async fn experiments_report_pass_at_k_from_repeated_trials() {
    // Case "good" completes, case "bad" fails; the grader marks one good trial wrong.
    let model = FakeModel::start(script(|c| async move {
        let good = c.messages()[0].to_string().contains("GOOD");
        match c.turn() {
            0 => plan("x"),
            _ if good => complete("done"),
            _ => fail("cannot"),
        }
    }))
    .await;
    let mail = mcp::mail().start().await;
    let app = Appliance::start(spec().app("mail", &mail, "public", "private", &[("list_sent", "read")]), &model).await;
    let dana = app.api("dana");
    let mut runs = vec![];
    for (case, text) in [("good", "GOOD task"), ("bad", "BAD task")] {
        for trial in 0..3 {
            let (st, v) = dana.post("/v1/dev/experiments/e1/trials", json!({ "case": case, "trial": trial, "text": text })).await;
            assert_eq!(st, 201, "{v}");
            runs.push((case, v["task_id"].as_str().unwrap().to_string(), v["run_id"].as_str().unwrap().to_string()));
        }
    }
    let (st, _) = dana.post("/v1/dev/experiments/e1/trials", json!({ "case": "good", "trial": 0, "text": "GOOD task" })).await;
    assert_eq!(st, 409, "a trial ran twice");
    for (_, task, _) in &runs {
        dana.wait_settled(task).await;
    }
    let good: Vec<&String> = runs.iter().filter(|r| r.0 == "good").map(|r| &r.2).collect();
    for r in &good {
        let passed = *r != good[0];
        let (st, _) = dana.post(&format!("/v1/dev/runs/{r}/grades"), json!({ "grader": "check", "passed": passed })).await;
        assert_eq!(st, 200);
    }
    let (_, e) = dana.get("/v1/dev/experiments/e1?k=1").await;
    let cfg = &e["configs"][0];
    let case = |name: &str| cfg["cases"].as_array().unwrap().iter().find(|c| c["case"] == name).unwrap().clone();
    assert_eq!(case("good")["passed"], 2);
    assert_eq!(case("bad")["passed"], 0);
    // pass@1 = c/n per case; the mean over cases is (2/3 + 0) / 2.
    let p = cfg["pass_at_k"]["mean"].as_f64().unwrap();
    assert!((p - 1.0 / 3.0).abs() < 1e-3, "pass@1 = {p}");
    let (_, e3) = dana.get("/v1/dev/experiments/e1?k=3").await;
    let good3 = e3["configs"][0]["cases"].as_array().unwrap().iter().find(|c| c["case"] == "good").unwrap().clone();
    assert_eq!(good3["pass_at_k"], 1.0);
    assert_eq!(good3["pass_hat_k"], 0.0);
    let tags: Vec<&str> = cfg["failures"].as_array().unwrap().iter().map(|f| f["where"].as_str().unwrap()).collect();
    assert!(tags.contains(&"grader_disagrees") && tags.contains(&"agent"), "{tags:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn playbook_items_reach_later_tasks_and_carry_their_taint() {
    let web = FakeWeb::start(&[("/howto", "<p>Invoices go to finance@example.com.</p>")]).await;
    let howto = web.url("/howto");
    let model = FakeModel::start(script(move |c| {
        let howto = howto.clone();
        async move {
            let first = c.messages()[0].to_string().contains("Learn");
            match (first, c.turn()) {
                (_, 0) => plan("x"),
                (true, 1) => tool("web_fetch", json!({ "url": howto })),
                (true, 2) => tool("playbook_update", json!({ "op": "add", "text": "Invoices go to finance@example.com.", "addresses": "where invoices go" })),
                (true, _) => fail("done"),
                (false, _) => {
                    assert!(c.messages()[0].to_string().contains("Invoices go to finance@example.com."), "the playbook item did not reach the next task");
                    fail("done")
                }
            }
        }
    }))
    .await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let a = alice.submit("Learn where invoices go.").await;
    alice.wait_status(&a, &["failed"]).await;
    let b = alice.submit("Send the invoice.").await;
    let later = alice.wait_status(&b, &["failed"]).await;
    assert_eq!(later["taint"]["untrusted"], true, "an item learned from a web page laundered its taint: {}", later["taint"]);

    let (_, pb) = app.api("dana").get("/v1/dev/playbook").await;
    let item = &pb["items"][0];
    assert_eq!(item["person"], "alice");
    assert_eq!(item["label"]["untrusted"], true, "{item}");
    assert_eq!(item["with_item"]["runs"].as_array().unwrap().len(), 1, "{pb}");
    let (_, v) = alice.get("/v1/versions").await;
    assert!(v["changes"].as_array().unwrap().iter().any(|c| c["what"] == "playbook"), "{v}");
    assert!(v["changes"].as_array().unwrap().iter().any(|c| c["what"] == "harness"), "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_change_is_a_version_change() {
    let model = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(spec(), &model).await;
    let alice = app.api("alice");
    let (_, before) = alice.get("/v1/versions").await;
    let (st, v) = alice.put("/v1/model", json!({ "adapter": "anthropic", "model": "claude-other" })).await;
    assert!(st.is_success(), "{v}");
    let (_, after) = alice.get("/v1/versions").await;
    assert_eq!(after["model"], "anthropic/claude-other");
    assert!(after["latest_seq"].as_i64() > before["latest_seq"].as_i64());
}

/// A person sees their own memories and can have one forgotten; nobody
/// else can, and a forgotten memory is gone after a restart too.
#[tokio::test(flavor = "multi_thread")]
async fn a_person_forgets_a_memory() {
    let model = FakeModel::start(steps(vec![plan("remember"), tool("memory_write", json!({ "fact": "alice prefers PDF reports", "topic": "reports" })), fail("done")])).await;
    let mut app = Appliance::start(Spec::default(), &model).await;
    let (alice, bob) = (app.api("alice"), app.api("bob"));
    let id = alice.submit("Remember that I like PDF reports.").await;
    alice.wait_settled(&id).await;
    let (_, v) = alice.get("/v1/memory").await;
    let mem = v["memory"].as_array().unwrap().clone();
    assert_eq!(mem.len(), 1, "{v}");
    let mid = mem[0]["id"].as_str().unwrap().to_string();
    assert!(bob.get("/v1/memory").await.1["memory"].as_array().unwrap().is_empty(), "bob sees alice's memory");
    let (st, _) = bob.send(reqwest::Method::DELETE, &format!("/v1/memory/{mid}"), None).await;
    assert_eq!(st, 404, "bob forgot alice's memory");
    let (st, _) = alice.send(reqwest::Method::DELETE, &format!("/v1/memory/{mid}"), None).await;
    assert_eq!(st, 204);
    app.restart().await;
    assert!(alice.get("/v1/memory").await.1["memory"].as_array().unwrap().is_empty(), "the memory came back");
}
