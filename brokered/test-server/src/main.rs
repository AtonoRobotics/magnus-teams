//! The brokered runtime's test server. `fetch` asks its provider (a test
//! host named in the call) for a path with the call's token, over the
//! kit's way out (TLS through the proxy), as a real connector does; `probe` tries every way out a wrong connector might
//! (a host not in its manifest through the proxy, a direct connection,
//! another port on the host); `leak` answers with the token it was given,
//! as a broken one might. Each answer names the microVM it ran in, which
//! the tests use to see one per person, reused, and stopped. `send` posts
//! to its provider and answers with what the provider answered, checked
//! against its output schema by the kit; `send_unchecked` does the same,
//! its answer unchecked, as a broken server's would be. `list` answers
//! with the items the provider lists at a path, as a watched read does.

use magnus_brokered_kit::{answer, exchange, https, Failure, Tool, MCP_PORT, PROXY_PORT, TOKEN_META};
use serde_json::{json, Value};
use std::sync::OnceLock;

/// This microVM's name, drawn when it boots.
fn instance() -> &'static str {
    static I: OnceLock<String> = OnceLock::new();
    I.get_or_init(|| {
        let mut b = [0u8; 8];
        unsafe { libc::getrandom(b.as_mut_ptr() as *mut libc::c_void, b.len(), 0) };
        b.iter().map(|x| format!("{x:02x}")).collect()
    })
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

fn fetch(a: &Value, token: &str) -> Result<Value, Failure> {
    let host = s(a, "host");
    let r = exchange(https(&host)?, "GET", &host, &s(a, "path"), token, None)?;
    Ok(json!({ "status": r.status, "body": String::from_utf8_lossy(&r.body), "instance": instance() }))
}

fn list(a: &Value, token: &str) -> Result<Value, Failure> {
    let host = s(a, "host");
    let r = exchange(https(&host)?, "GET", &host, &s(a, "path"), token, None)?;
    let items: Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
    Ok(json!({ "items": items }))
}

fn probe(a: &Value, _: &str) -> Result<Value, Failure> {
    let host = s(a, "host");
    let through_proxy = match https(&host) {
        Ok(_) => "connected".to_string(),
        Err(e) => e,
    };
    let direct = match host
        .parse::<std::net::SocketAddr>()
        .map_err(|e| e.to_string())
        .and_then(|at| std::net::TcpStream::connect_timeout(&at, std::time::Duration::from_secs(3)).map_err(|e| e.to_string()))
    {
        Ok(_) => "connected".to_string(),
        Err(e) => e,
    };
    // Another port on the host: only the proxy's answers.
    let other = unsafe {
        let fd = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0);
        let mut addr: libc::sockaddr_vm = std::mem::zeroed();
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = PROXY_PORT + 1;
        addr.svm_cid = libc::VMADDR_CID_HOST;
        let rc = libc::connect(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_vm>() as u32);
        let e = std::io::Error::last_os_error();
        libc::close(fd);
        if rc == 0 {
            "connected".to_string()
        } else {
            e.to_string()
        }
    };
    Ok(json!({ "through_proxy": through_proxy, "direct": direct, "other_port": other }))
}

fn leak(_: &Value, token: &str) -> Result<Value, Failure> {
    Ok(json!({ "token": token }))
}

/// POST to the provider, which does something each time, and answer with
/// what it answered, as a connector whose provider answers more than it
/// should might.
fn send(a: &Value, token: &str) -> Result<Value, Failure> {
    let host = s(a, "host");
    let r = exchange(https(&host)?, "POST", &host, &s(a, "path"), token, Some(&json!({})))?;
    serde_json::from_slice(&r.body).map_err(|e| Failure::Unknown(format!("the provider's answer is not JSON: {e}")))
}

/// The tool this server answers without the kit's check of its answer.
const UNCHECKED: &str = "send_unchecked";

fn main() {
    let host = json!({ "type": "object", "required": ["host"], "additionalProperties": false, "properties": { "host": { "type": "string" }, "path": { "type": "string" } } });
    let sent = json!({ "type": "object", "required": ["id"], "additionalProperties": false, "properties": { "id": { "type": "string" } } });
    let tools = vec![
        Tool {
            name: "fetch",
            description: "Get a path from the provider with the call's access token.",
            input: host.clone(),
            output: json!({ "type": "object", "required": ["status", "body", "instance"], "properties": { "status": { "type": "integer" }, "body": { "type": "string" }, "instance": { "type": "string" } } }),
            run: fetch,
        },
        Tool {
            name: "probe",
            description: "Try every way out of the microVM.",
            input: host.clone(),
            output: json!({ "type": "object", "required": ["through_proxy", "direct", "other_port"] }),
            run: probe,
        },
        Tool {
            name: "leak",
            description: "Answer with the access token this call came with.",
            input: json!({ "type": "object", "properties": {} }),
            output: json!({ "type": "object", "required": ["token"] }),
            run: leak,
        },
        Tool {
            name: "list",
            description: "List the items the provider has at a path.",
            input: host.clone(),
            output: json!({ "type": "object", "required": ["items"], "properties": { "items": { "type": "array" } } }),
            run: list,
        },
        Tool { name: "send", description: "Post to the provider and answer with what it answered.", input: host.clone(), output: sent.clone(), run: send },
        Tool { name: UNCHECKED, description: "Post to the provider and answer with what it answered, unchecked.", input: host, output: sent, run: send },
    ];
    serve("magnus-brokered-test", tools)
}

/// The kit's `serve`, except that `send_unchecked`'s answer goes back as
/// it came, unchecked against its output schema, as a broken server's
/// might: the appliance's own check is all there is.
fn serve(server: &'static str, tools: Vec<Tool>) -> ! {
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::FromRawFd;
    let tools: &'static [Tool] = Box::leak(tools.into_boxed_slice());
    let l = unsafe {
        let fd = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        let mut addr: libc::sockaddr_vm = std::mem::zeroed();
        addr.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        addr.svm_port = MCP_PORT;
        addr.svm_cid = libc::VMADDR_CID_ANY;
        let a = &addr as *const _ as *const libc::sockaddr;
        assert!(libc::bind(fd, a, std::mem::size_of::<libc::sockaddr_vm>() as u32) == 0 && libc::listen(fd, 64) == 0, "vsock listen: {}", std::io::Error::last_os_error());
        fd
    };
    loop {
        let fd = unsafe { libc::accept4(l, std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC) };
        if fd < 0 {
            continue;
        }
        let s = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
        std::thread::spawn(move || {
            let Ok(mut w) = s.try_clone() else { return };
            for line in BufReader::new(s).lines() {
                let Ok(line) = line else { return };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let p = &msg["params"];
                let a = if msg["method"] == "tools/call" && p["name"] == UNCHECKED {
                    let result = match send(&p["arguments"], p["_meta"][TOKEN_META].as_str().unwrap_or_default()) {
                        Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v, "isError": false }),
                        Err(e) => json!({ "content": [{ "type": "text", "text": format!("{e:?}") }], "isError": true }),
                    };
                    Some(json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result }))
                } else {
                    answer(&msg, server, tools)
                };
                if let Some(a) = a {
                    if writeln!(w, "{a}").and_then(|_| w.flush()).is_err() {
                        return;
                    }
                }
            }
        });
    }
}
