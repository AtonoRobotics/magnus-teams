//! End to end in the kernel: two of Mo's computers report Blender through
//! the app's own session, each with a fake local MCP server standing in for
//! the program; a call goes to the computer where Mo allowed it, and only
//! the app's session takes or answers calls.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A program's MCP server on loopback: answers `tools/call` with its tag,
/// the tool and the arguments, and counts the calls.
async fn fake_program(tag: &'static str) -> (String, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |axum::Json(b): axum::Json<Value>| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                let text = format!("{tag}: {} {}", b["params"]["name"].as_str().unwrap_or_default(), b["params"]["arguments"]);
                axum::Json(json!({ "jsonrpc": "2.0", "id": b["id"], "result": { "content": [{ "type": "text", "text": text }] } }))
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, n)
}

/// The app's poll loop, cut to its routing: take a call, forward it to the
/// program's loopback server, answer.
fn bridge(base: String, token: String, device: &'static str, program_url: String) {
    tokio::spawn(async move {
        let http = reqwest::Client::new();
        loop {
            let Ok(r) = http.get(format!("{base}/v1/desktop/devices/{device}/next")).bearer_auth(&token).send().await else { return };
            if r.status() != reqwest::StatusCode::OK {
                continue;
            }
            let job: Value = r.json().await.unwrap();
            let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": job["tool"], "arguments": job["arguments"] } });
            let answer: Value = http.post(&program_url).json(&call).send().await.unwrap().json().await.unwrap();
            let text = answer["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
            http.post(format!("{base}/v1/desktop/devices/{device}/result")).bearer_auth(&token).json(&json!({ "job": job["id"], "ok": true, "text": text })).send().await.unwrap();
        }
    });
}

async fn call(http: &reqwest::Client, base: &str, token: &str, method: reqwest::Method, path: &str, body: Value) -> (u16, Value) {
    let r = http.request(method, format!("{base}{path}")).bearer_auth(token).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

fn blender(running: bool) -> Value {
    json!([{ "program": "blender", "version": "4.5.1", "path": "/usr/bin/blender", "running": running,
             "tools": [{ "name": "run_python", "description": "Run Python in Blender", "input_schema": { "type": "object", "properties": { "code": { "type": "string" } } } }] }])
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_reaches_the_computer_it_was_allowed_on_through_the_apps_own_session() {
    let s = crate::admin_app::tests::server_with("", |_| axum::Router::new()).await;
    let (k, base, http) = (s.k.clone(), s.base.clone(), reqwest::Client::new());
    let app = k.accounts.open_session("mo", "app", None).await.unwrap().1;
    let browser = k.accounts.open_session("mo", "browser", None).await.unwrap().1;
    let sams = k.accounts.open_session("sam", "app", None).await.unwrap().1;
    let post = reqwest::Method::POST;
    let put = reqwest::Method::PUT;

    // Both of Mo's computers report Blender, running; only the app's session may.
    for (id, name) in [("laptop-0001", "Mo's laptop"), ("desk-0001", "Mo's desk")] {
        let (st, v) = call(&http, &base, &app, post.clone(), "/v1/desktop/devices", json!({ "id": id, "name": name, "programs": blender(true) })).await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(v["programs"][0]["state"], "not_enabled", "{v}");
    }
    let (st, _) = call(&http, &base, &browser, post.clone(), "/v1/desktop/devices", json!({ "id": "laptop-0002", "name": "x", "programs": [] })).await;
    assert_eq!(st, 403, "a browser session reported a computer");
    let (st, _) = call(&http, &base, &sams, post.clone(), "/v1/desktop/devices", json!({ "id": "desk-0001", "name": "Sam's", "programs": [] })).await;
    assert_eq!(st, 409, "another person took Mo's computer id");

    // Mo allows Blender on the desk only, from the app; a browser cannot allow it.
    let (st, _) = call(&http, &base, &browser, put.clone(), "/v1/desktop/devices/desk-0001/programs/blender", json!({ "allowed": true })).await;
    assert_eq!(st, 403, "a browser session allowed a program");
    let (st, v) = call(&http, &base, &app, put.clone(), "/v1/desktop/devices/desk-0001/programs/blender", json!({ "allowed": true })).await;
    assert_eq!((st, &v["programs"][0]["state"]), (200, &json!("ready")), "{v}");
    let (st, _) = call(&http, &base, &app, put.clone(), "/v1/desktop/devices/desk-0001/programs/photoshop", json!({ "allowed": true })).await;
    assert_eq!(st, 404);

    // The agent sees Blender's tool on the desk, and the tool is classed as running code.
    let specs = k.desktop_specs("mo");
    let tool = specs.iter().find(|t| t.name == "desktop_blender__run_python").expect("the ready program's tool is offered");
    assert_eq!(tool.input_schema["properties"]["computer"]["enum"], json!(["Mo's desk"]));
    assert!(k.desktop_specs("sam").is_empty(), "Sam sees Mo's computers");
    assert_eq!(crate::boundary::facts(&k, "desktop_blender__run_python").unwrap().risk, Risk::Irreversible);

    // Only the app's session takes calls, and only for its own person's computer.
    let r = http.get(format!("{base}/v1/desktop/devices/desk-0001/next")).bearer_auth(&browser).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403, "a browser session took a call");
    let r = http.get(format!("{base}/v1/desktop/devices/desk-0001/next")).bearer_auth(&sams).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404, "Sam took a call meant for Mo's desk");

    let (laptop_url, laptop_calls) = fake_program("laptop").await;
    let (desk_url, desk_calls) = fake_program("desk").await;
    bridge(base.clone(), app.clone(), "laptop-0001", laptop_url);
    bridge(base.clone(), app.clone(), "desk-0001", desk_url);

    let p = program("blender").unwrap();
    let (text, failed, _) = k.desktop_call("mo", p, "run_python", &json!({ "code": "print(1)" })).await;
    assert!(!failed, "{text}");
    assert_eq!(text, r#"desk: run_python {"code":"print(1)"}"#, "the call went elsewhere, or carried `computer`");
    assert_eq!((desk_calls.load(Ordering::SeqCst), laptop_calls.load(Ordering::SeqCst)), (1, 0));
    let (text, failed, _) = k.desktop_call("mo", p, "run_python", &json!({ "code": "x", "computer": "Mo's laptop" })).await;
    assert!(failed && text.contains("not enabled on Mo's laptop"), "{text}");
    assert_eq!(laptop_calls.load(Ordering::SeqCst), 0, "a call reached a program Mo did not allow");

    // Blender closed on the desk: nothing to call, and the agent is told so.
    let (st, v) = call(&http, &base, &app, post.clone(), "/v1/desktop/devices", json!({ "id": "desk-0001", "name": "Mo's desk", "programs": blender(false) })).await;
    assert_eq!((st, &v["programs"][0]["state"]), (200, &json!("closed")), "{v}");
    assert!(programs_text(&k.desktop_views("mo")).contains("Mo's desk: Blender: closed"));
    assert!(k.desktop_specs("mo").iter().all(|t| t.name == "desktop_programs"), "a closed program's tools were offered");

    // Taken back from a browser: allowed, and it holds.
    call(&http, &base, &app, post.clone(), "/v1/desktop/devices", json!({ "id": "desk-0001", "name": "Mo's desk", "programs": blender(true) })).await;
    let (st, v) = call(&http, &base, &browser, put.clone(), "/v1/desktop/devices/desk-0001/programs/blender", json!({ "allowed": false })).await;
    assert_eq!((st, &v["programs"][0]["state"]), (200, &json!("not_enabled")), "{v}");
    let (text, failed, _) = k.desktop_call("mo", p, "run_python", &json!({ "code": "x" })).await;
    assert!(failed && text.contains("not ready"), "{text}");
    assert_eq!(desk_calls.load(Ordering::SeqCst), 1, "a call reached a program after its consent was taken back");

    // An answer for a call nobody made is told to no one; and the list names each computer.
    let (st, v) = call(&http, &base, &app, post, "/v1/desktop/devices/desk-0001/result", json!({ "job": "nobody", "ok": true, "text": "x" })).await;
    assert_eq!((st, &v["told"]), (200, &json!(false)));
    let (st, v) = call(&http, &base, &browser, reqwest::Method::GET, "/v1/desktop/devices", Value::Null).await;
    let names: Vec<&str> = v.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!((st, names.len()), (200, 2), "{v}");
    assert!(names.contains(&"Mo's desk") && names.contains(&"Mo's laptop"));
    s.done().await;
}
