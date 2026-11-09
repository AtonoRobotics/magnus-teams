//! The walls between people in the content store (owner's ruling 9 and
//! "recall only", 2026-11-01; `docs/proposals/recall-only.md`, step 1):
//! every row of content records whose it is, and the per-person reader
//! role sees only that person's rows, whatever query it runs.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;
use serde_json::json;
use tokio_postgres::Client;

/// Content rows of runs whose person is not the one their row records,
/// and how many rows each person has.
async fn owners(admin: &Client) -> (i64, i64, i64) {
    let wrong = admin
        .query_one(
            "SELECT count(*) FROM public.content c JOIN public.events e USING (seq)
             WHERE e.run_id IS NOT NULL AND c.person IS DISTINCT FROM
                 (SELECT t.body->>'person' FROM public.events t WHERE t.kind = 'task_created' AND t.run_id = e.run_id LIMIT 1)",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let count = |who: &'static str| async move { admin.query_one("SELECT count(*) FROM public.content WHERE person = $1", &[&who]).await.unwrap().get::<_, i64>(0) };
    (wrong, count("alice").await, count("carol").await)
}

/// What `{role}_person` returns to a query, reading for `person`.
async fn as_reader(kernel: &mut Client, role: &str, person: Option<&str>, sql: &str) -> Result<Vec<String>, tokio_postgres::Error> {
    let tx = kernel.transaction().await?;
    tx.batch_execute(&format!("SET LOCAL ROLE {role}_person")).await?;
    if let Some(p) = person {
        tx.execute("SELECT set_config('magnus.person', $1, true)", &[&p]).await?;
    }
    let rows = tx.query(sql, &[]).await?;
    tx.commit().await?;
    Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
}

#[tokio::test(flavor = "multi_thread")]
async fn each_persons_content_is_walled_from_everyone_else_in_the_database() {
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let mut app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (alices, carols) = (canary("alice-words"), canary("carol-words"));
    let a = alice.submit(&format!("Draft the board letter. {alices}")).await;
    let c = carol.submit(&format!("Plan the offsite. {carols}")).await;
    alice.wait_settled(&a).await;
    carol.wait_settled(&c).await;
    let (st, v) = carol.post("/v1/schedules", json!({ "text": format!("Water the plants. {carols}"), "daily_at": "03:00" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");

    // Each row records its person: the task's text, and its run's replies.
    let admin = app.db.admin().await;
    let (wrong, a_rows, c_rows) = owners(&admin).await;
    assert_eq!(wrong, 0, "a run's content is recorded as someone else's, or no one's");
    assert!(a_rows >= 2 && c_rows >= 3, "alice {a_rows}, carol {c_rows}");

    // Reading for carol, the wall returns carol's content and none of alice's.
    let role = app.db.kernel_role.clone();
    let (mut kernel, conn) = tokio_postgres::connect(&app.db.kernel_url(), tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(conn);
    let seen = as_reader(&mut kernel, &role, Some("carol"), "SELECT body::text FROM public.content").await.unwrap();
    assert!(seen.iter().any(|b| b.contains(&carols)), "carol's own content is walled from her");
    assert!(!seen.iter().any(|b| b.contains(&alices)), "alice's content reached a reader for carol");
    let asked = as_reader(&mut kernel, &role, Some("carol"), "SELECT body::text FROM public.content WHERE person = 'alice' OR person IS NULL").await.unwrap();
    assert!(asked.is_empty(), "a query asking for alice's rows got them: {asked:?}");
    // The log itself is not readable through the wall at all.
    let joined = as_reader(&mut kernel, &role, Some("carol"), "SELECT c.body::text FROM public.content c JOIN public.events e USING (seq)").await;
    assert!(joined.is_err(), "the reader role reads the event log: {joined:?}");
    // Reading for no one returns nothing.
    assert!(as_reader(&mut kernel, &role, None, "SELECT body::text FROM public.content").await.unwrap().is_empty());
    // The kernel's own role still reads every row, as replay needs.
    let all: i64 = kernel.query_one("SELECT count(*) FROM public.content", &[]).await.unwrap().get(0);
    assert!(all >= a_rows + c_rows);

    // Rows written before the column existed are filled by `migrate`.
    admin.batch_execute("UPDATE public.content SET person = NULL").await.unwrap();
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["migrate", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--admin-url", &app.db.admin_url()])
        .output()
        .unwrap();
    assert!(out.status.success(), "migrate: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(owners(&admin).await, (0, a_rows, c_rows), "migrate did not fill whose each row is");

    // And so are rows restored from a backup, which does not say.
    let (st, b) = alice.post("/v1/backups", json!({})).await;
    assert_eq!(st, 201, "{b}");
    let file = app.state_dir.join("backups").join(b["file"].as_str().unwrap());
    app.kill().await;
    drop(kernel);
    admin.batch_execute("DROP TABLE public.content, public.events").await.unwrap();
    std::fs::remove_dir_all(app.state_dir.join("vault")).unwrap();
    let out = std::process::Command::new(bin("magnus-appliance"))
        .args(["restore", "--config"])
        .arg(app.dir.path().join("magnus.toml"))
        .args(["--admin-url", &app.db.admin_url(), "--from"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success(), "restore: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(owners(&admin).await, (0, a_rows, c_rows), "restored rows do not say whose they are");
    app.restart().await;
    assert_eq!(alice.get("/v1/me").await.0, 200);
}

/// The owner's read of a person's memory is recalled from the content
/// store as that person, through the wall (step 2): content the database
/// does not give that person is not shown, whatever the kernel holds.
#[tokio::test(flavor = "multi_thread")]
async fn the_owner_reads_a_persons_memory_recalled_through_their_wall() {
    let fact = canary("carol-fact");
    let said = fact.clone();
    let m = FakeModel::start(script(move |c| {
        let said = said.clone();
        async move {
            let pass = c.offered_tools() == ["memory_write".to_string()];
            match c.turn() {
                0 if !pass && c.text().contains("Remember that") => tool("memory_write", json!({ "fact": format!("Carol's code name is {said}.") })),
                _ => Reply::Text("Noted.".into()),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let (alice, carol) = (app.api("alice"), app.api("carol"));
    let (st, v) = carol.post("/v1/chat", json!({ "text": "Remember that my code name matters." })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    let read = || async { alice.get("/v1/people/carol/data/memory").await.1["memory"].as_array().cloned().unwrap_or_default() };
    let facts = read().await;
    assert!(facts.iter().any(|f| f["fact"].as_str().is_some_and(|t| t.contains(&fact))), "{facts:?}");

    // The database now says the row is someone else's: a read for carol
    // that took it from the kernel's memory would still show it.
    let admin = app.db.admin().await;
    let moved = admin.execute("UPDATE public.content SET person = 'bob' WHERE body->>'fact' LIKE '%' || $1 || '%'", &[&fact]).await.unwrap();
    assert_eq!(moved, 1);
    let facts = read().await;
    assert!(!facts.iter().any(|f| f.to_string().contains(&fact)), "carol's memory read past her wall: {facts:?}");
    // Her own view is recalled the same way (step 4).
    assert!(!carol.get("/v1/memory").await.1.to_string().contains(&fact));
}

/// A person's memory is recalled, not kept (step 4): their memory view,
/// memory search and the trash show what the content store says, for
/// them, whatever the kernel read before.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_memory_is_recalled_from_the_content_store_for_them() {
    let (fact, edited) = (canary("carol-fact"), canary("edited"));
    let said = fact.clone();
    let m = FakeModel::start(script(move |c| {
        let said = said.clone();
        async move {
            let pass = c.offered_tools() == ["memory_write".to_string()];
            let first = c.messages().first().map(|m| m.to_string()).unwrap_or_default();
            match c.turn() {
                0 if !pass && first.contains("Remember that") => tool("memory_write", json!({ "fact": format!("Carol's code name is {said}.") })),
                0 if !pass && first.contains("code name") => tool("memory_search", json!({ "query": "code name" })),
                // What the search returned, not the whole request: the request also
                // holds Magnus's earlier messages to her, which name the fact as it was.
                _ if !pass && first.contains("code name") => Reply::Text(format!("Found: {}", c.last_results().iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n"))),
                _ => Reply::Text("Noted.".into()),
            }
        }
    }))
    .await;
    let app = Appliance::start(Spec::default(), &m).await;
    let carol = app.api("carol");
    let chat = |text: &'static str| {
        let carol = carol.clone();
        async move {
            let (st, v) = carol.post("/v1/chat", json!({ "text": text })).await;
            assert_eq!(st.as_u16(), 201, "{v}");
            carol.wait_settled(v["task_id"].as_str().unwrap()).await
        }
    };
    chat("Remember that my code name matters.").await;
    let kept = carol.get("/v1/memory").await.1;
    assert!(kept.to_string().contains(&fact), "{kept}");
    let id = kept["memory"][0]["id"].as_str().unwrap().to_string();

    // What the content store says is what she is shown.
    let admin = app.db.admin().await;
    let changed = admin
        .execute(
            "UPDATE public.content SET body = jsonb_set(body, '{fact}', to_jsonb($2::text)) WHERE body->>'fact' LIKE '%' || $1 || '%'",
            &[&fact, &format!("Carol's code name is {edited}.")],
        )
        .await
        .unwrap();
    assert_eq!(changed, 1);
    let shown = carol.get("/v1/memory").await.1.to_string();
    assert!(shown.contains(&edited) && !shown.contains(&fact), "her memory view is not recalled: {shown}");
    let found = chat("What is my code name?").await;
    let answer = found.to_string();
    assert!(answer.contains(&edited) && !answer.contains(&fact), "memory search is not recalled: {answer}");

    // In the trash, its title is recalled too.
    assert!(carol.post("/v1/trash", json!({ "item": id })).await.0.is_success());
    let trash = carol.get("/v1/trash").await.1.to_string();
    assert!(trash.contains(&edited), "{trash}");
    assert!(carol.post(&format!("/v1/trash/{id}/restore"), json!({})).await.0.is_success());

    // A row the database says is not hers is not shown to her, nor found.
    admin.execute("UPDATE public.content SET person = 'bob' WHERE body->>'fact' LIKE '%' || $1 || '%'", &[&edited]).await.unwrap();
    let shown = carol.get("/v1/memory").await.1.to_string();
    assert!(!shown.contains(&edited), "her memory view read past her wall: {shown}");
    let answer = chat("What is my code name?").await.to_string();
    assert!(!answer.contains(&edited), "memory search read past her wall: {answer}");
}

/// A person's library files are recalled, not kept (step 5): a download,
/// a search of their text, and the file a task is given come from the
/// content store, for them.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_files_are_recalled_from_the_content_store_for_them() {
    use base64::Engine;
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    let m = FakeModel::start(steps(vec![fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let carol = app.api("carol");
    let (old, new) = (canary("old-words").to_lowercase(), canary("new-words").to_lowercase());
    let (st, v) = carol.post("/v1/files", json!({ "name": "notes.txt", "media_type": "text/plain", "data_b64": b64(&format!("plan: {old}")) })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (st, v) = carol.post("/v1/files", json!({ "name": "chart.png", "media_type": "image/png", "data_b64": b64(&old) })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let image = v["id"].as_str().unwrap().to_string();
    let found = |q: String| {
        let carol = carol.clone();
        async move { carol.get(&format!("/v1/files?q={q}")).await.1["files"].as_array().cloned().unwrap_or_default().len() }
    };
    assert_eq!(found(old.clone()).await, 1);

    // The content store now says something else: that is what she gets.
    let admin = app.db.admin().await;
    for (from, to) in [(format!("plan: {old}"), format!("plan: {new}")), (old.clone(), new.clone())] {
        let changed = admin
            .execute("UPDATE public.content SET body = jsonb_set(body, '{data_b64}', to_jsonb($2::text)) WHERE body->>'data_b64' = $1", &[&b64(&from), &b64(&to)])
            .await
            .unwrap();
        assert_eq!(changed, 1);
    }
    assert_eq!(carol.get(&format!("/v1/files/{id}")).await.1, json!(format!("plan: {new}")), "the download is not recalled");
    assert_eq!((found(old.clone()).await, found(new.clone()).await), (0, 1), "the search is not recalled");
    let (st, v) = carol.post("/v1/tasks", json!({ "text": "Describe the chart.", "files": [image] })).await;
    assert!(st.is_success(), "{v}");
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    // Every model call that carries the image: the run's own.
    let sent: Vec<String> = m.requests().iter().map(|c| c.body.to_string()).filter(|b| b.contains("\"type\":\"image\"")).collect();
    assert!(!sent.is_empty() && sent.iter().all(|b| b.contains(&b64(&new)) && !b.contains(&b64(&old))), "the task was not given the recalled file: {} requests", sent.len());

    // A row the database says is not hers is neither downloaded nor found.
    admin.execute("UPDATE public.content SET person = 'bob' WHERE body->>'data_b64' = $1", &[&b64(&format!("plan: {new}"))]).await.unwrap();
    assert_eq!(carol.get(&format!("/v1/files/{id}")).await.0.as_u16(), 404, "the download read past her wall");
    assert_eq!(found(new.clone()).await, 0, "the search read past her wall");
}

/// What a person asked is recalled, not kept (step 3a): their tasks, their
/// schedules and their chat messages show what the content store says,
/// for them, whatever the kernel read before.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_requests_are_recalled_from_the_content_store_for_them() {
    let m = FakeModel::start(steps(vec![fail("nothing to do"), fail("nothing to do")])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let carol = app.api("carol");
    let (said, edited) = (canary("said"), canary("edited"));
    let task = carol.submit(&format!("Plan the offsite. {said}")).await;
    carol.wait_settled(&task).await;
    let (st, v) = carol.post("/v1/chat", json!({ "text": format!("Hello there. {said}") })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let thread = v["thread"].as_str().unwrap().to_string();
    carol.wait_settled(v["task_id"].as_str().unwrap()).await;
    let (st, v) = carol.post("/v1/schedules", json!({ "text": format!("Water the plants. {said}"), "daily_at": "03:00" })).await;
    assert_eq!(st.as_u16(), 201, "{v}");
    let views = || async {
        let tasks = carol.get("/v1/tasks").await.1.to_string();
        let one = carol.get(&format!("/v1/tasks/{task}")).await.1.to_string();
        let schedules = carol.get("/v1/schedules").await.1.to_string();
        let chat = carol.get(&format!("/v1/chat/{thread}")).await.1.to_string();
        let threads = carol.get("/v1/chat").await.1.to_string();
        [tasks, one, schedules, chat, threads]
    };
    for v in views().await {
        assert!(v.contains(&said), "{v}");
    }

    // The content store now says something else: that is what she is shown.
    let admin = app.db.admin().await;
    let changed = admin
        .execute(
            "UPDATE public.content SET body = jsonb_set(body, '{text}', to_jsonb(replace(body->>'text', $1, $2))) WHERE body->>'text' LIKE '%' || $1 || '%'",
            &[&said, &edited],
        )
        .await
        .unwrap();
    assert!(changed >= 3, "{changed}");
    for (name, v) in ["tasks", "task", "schedules", "chat", "threads"].iter().zip(views().await) {
        assert!(v.contains(&edited) && !v.contains(&said), "her {name} view is not recalled: {v}");
    }

    // Rows the database says are not hers are not shown to her.
    admin.execute("UPDATE public.content SET person = 'bob' WHERE body->>'text' LIKE '%' || $1 || '%'", &[&edited]).await.unwrap();
    for (name, v) in ["tasks", "task", "schedules", "chat", "threads"].iter().zip(views().await) {
        assert!(!v.contains(&edited), "her {name} view read past her wall: {v}");
    }
}
