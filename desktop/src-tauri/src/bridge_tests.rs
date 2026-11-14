use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A program's MCP server on loopback, speaking just enough HTTP: it
/// answers `initialize` with a session, lists one tool, and answers
/// `tools/call` as an event stream. It counts the calls it takes.
async fn fake_program() -> (SocketAddr, Arc<AtomicUsize>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let n = n.clone();
            tokio::spawn(async move {
                let mut buf = vec![];
                let mut chunk = [0u8; 4096];
                let body = loop {
                    let k = s.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..k]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            break text[end + 4..end + 4 + len].to_string();
                        }
                    }
                    if k == 0 {
                        return;
                    }
                };
                let req: Value = serde_json::from_str(&body).unwrap();
                let (kind, out) = match req["method"].as_str().unwrap_or_default() {
                    "initialize" => ("application/json", json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": MCP_VERSION } }).to_string()),
                    "tools/list" => ("application/json", json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "tools": [{ "name": "run_python", "description": "Run Python", "inputSchema": { "type": "object" } }] } }).to_string()),
                    "tools/call" => {
                        n.fetch_add(1, Ordering::SeqCst);
                        let text = format!("ran {} with {}", req["params"]["name"].as_str().unwrap_or_default(), req["params"]["arguments"]);
                        ("text/event-stream", format!("event: message\ndata: {}\n\n", json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "content": [{ "type": "text", "text": text }] } })))
                    }
                    _ => ("application/json", String::new()),
                };
                let status = if out.is_empty() { "202 Accepted" } else { "200 OK" };
                let head = format!("HTTP/1.1 {status}\r\ncontent-type: {kind}\r\nmcp-session-id: s1\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", out.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(out.as_bytes()).await;
            });
        }
    });
    (addr, calls)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A port is registered only on loopback, and only for a supported program.
#[test]
fn only_a_loopback_port_for_a_supported_program_is_registered() {
    let mut p = Ports::default();
    assert!(p.register("blender", "192.168.1.20:9876".parse().unwrap()).is_err(), "a port beyond loopback was registered");
    assert!(p.register("blender", "0.0.0.0:9876".parse().unwrap()).is_err());
    assert!(p.register("blender", "127.0.0.1:0".parse().unwrap()).is_err());
    assert!(p.register("photoshop", "127.0.0.1:9876".parse().unwrap()).is_err(), "an unsupported program was registered");
    assert!(p.register("blender", "127.0.0.1:9876".parse().unwrap()).is_ok());
    assert!(p.register("freecad", "[::1]:9877".parse().unwrap()).is_ok());
}

/// A call goes only to an enabled program's registered port.
#[test]
fn a_call_goes_only_to_an_enabled_programs_registered_port() {
    let mut p = Ports::default();
    p.register("blender", "127.0.0.1:9876".parse().unwrap()).unwrap();
    assert_eq!(p.target("blender", &set(&["blender"])).unwrap(), "127.0.0.1:9876".parse::<SocketAddr>().unwrap());
    assert!(p.target("blender", &set(&[])).unwrap_err().contains("not enabled"));
    assert!(p.target("freecad", &set(&["freecad"])).unwrap_err().contains("no server registered"));
    // Phase 1 names no program's port yet: the catalog registers nothing beyond loopback.
    assert!(Ports::from_catalog().0.values().all(|a| a.ip().is_loopback()));
}

#[test]
fn the_servers_view_says_which_programs_are_enabled() {
    let v = json!({ "programs": [{ "program": "blender", "allowed": true }, { "program": "freecad", "allowed": false }] });
    assert_eq!(enabled_of(&v), set(&["blender"]));
    assert!(enabled_of(&Value::Null).is_empty());
}

#[test]
fn an_answer_is_read_from_json_or_an_event_stream() {
    let id = json!(1);
    assert_eq!(answer_of(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#, false, &id).unwrap(), json!({ "ok": true }));
    let sse = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"note\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"n\":2}}\n\n";
    assert_eq!(answer_of(sse, true, &id).unwrap(), json!({ "n": 2 }));
    assert!(answer_of(r#"{"jsonrpc":"2.0","id":1,"error":{"message":"no"}}"#, false, &id).unwrap_err().contains("refused it: no"));
    assert!(answer_of("<html>", false, &id).is_err());
}

/// End to end on this side: the report finds the enabled program's server
/// and its tools; a call from the server reaches it; a call for a program
/// not enabled, or with no registered port, reaches nothing.
#[tokio::test]
async fn a_call_reaches_the_enabled_programs_loopback_server_and_nothing_else() {
    let (addr, calls) = fake_program().await;
    let mut ports = Ports::default();
    ports.register("blender", addr).unwrap();
    let mcp = Mcp::new().unwrap();
    let found = vec![Found { program: "blender", version: Some("4.5".into()), path: None }, Found { program: "freecad", version: None, path: None }];

    let before = report(&found, &set(&[]), &ports, &mcp).await;
    assert_eq!(before[0]["running"], false, "a program not enabled was probed");
    let after = report(&found, &set(&["blender"]), &ports, &mcp).await;
    assert_eq!((after[0]["running"].clone(), after[0]["tools"][0]["name"].clone()), (json!(true), json!("run_python")), "{after:?}");
    assert_eq!(after[1]["running"], false);

    let job = json!({ "id": "j1", "program": "blender", "tool": "run_python", "arguments": { "code": "print(1)" } });
    let (ok, text) = carry_out(&job, &ports, &set(&["blender"]), &mcp).await;
    assert!(ok, "{text}");
    assert_eq!(text, r#"ran run_python with {"code":"print(1)"}"#);
    let (ok, text) = carry_out(&job, &ports, &set(&[]), &mcp).await;
    assert!(!ok && text.contains("not enabled"), "{text}");
    let other = json!({ "id": "j2", "program": "freecad", "tool": "x", "arguments": {} });
    let (ok, _) = carry_out(&other, &ports, &set(&["freecad", "blender"]), &mcp).await;
    assert!(!ok, "a program with no registered port was reached");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a refused call reached the program");
}
