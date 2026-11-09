//! The server kit's own tests (`brokered/kit/src/lib.rs`, its `tests`
//! module), kept with the repository's tests.

use super::*;

fn echo(args: &Value, token: &str) -> Result<Value, Failure> {
    match args["say"].as_str() {
        Some("wrong") => Ok(json!({ "said": 1 })),
        Some("lost") => Err(Failure::Unknown("lost on the way".into())),
        Some("more") => Err(Failure::NeedsScope("wants more".into(), vec!["s2".into()])),
        Some(s) => Ok(json!({ "said": format!("{s} with {}", token.len()) })),
        None => Err("nothing to say".to_string().into()),
    }
}

fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "echo",
        description: "Say it back",
        input: json!({ "type": "object", "properties": { "say": { "type": "string" } }, "additionalProperties": false }),
        output: json!({ "type": "object", "properties": { "said": { "type": "string" } }, "required": ["said"], "additionalProperties": false }),
        run: echo,
    }]
}

fn talk(lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out = vec![];
    session(input.as_bytes(), &mut out, "test", &tools());
    String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn call(id: u32, args: Value, token: Option<&str>) -> Value {
    let mut p = json!({ "name": "echo", "arguments": args });
    if let Some(t) = token {
        p["_meta"] = json!({ TOKEN_META: t });
    }
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": p })
}

/// A session answers MCP line by line: it lists each tool with both its
/// schemas, runs a call only with the call's token and an input its
/// schema admits, gives back only an answer its output schema admits,
/// and answers no notification.
#[test]
fn a_session_runs_a_call_only_with_its_token_and_schemas() {
    let a = talk(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        call(3, json!({ "say": "hi" }), Some("tok")),
        call(4, json!({ "say": "hi" }), None),
        call(5, json!({ "say": 7 }), Some("tok")),
        call(6, json!({ "say": "wrong" }), Some("tok")),
        call(7, json!({}), Some("tok")),
        json!({ "jsonrpc": "2.0", "id": 8, "method": "resources/list" }),
        call(9, json!({ "say": "lost" }), Some("tok")),
        call(10, json!({ "say": "more" }), Some("tok")),
    ]);
    assert_eq!(a.iter().map(|m| m["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert_eq!(a[0]["result"]["protocolVersion"], PROTOCOL);
    let t = &a[1]["result"]["tools"][0];
    assert_eq!((t["name"].as_str(), t["inputSchema"]["type"].as_str(), t["outputSchema"]["required"][0].as_str()), (Some("echo"), Some("object"), Some("said")));
    assert_eq!(a[2]["result"]["structuredContent"], json!({ "said": "hi with 3" }));
    assert_eq!(a[2]["result"]["isError"], false);
    for (m, says) in [(&a[3], "no access token"), (&a[4], "input does not match"), (&a[5], "answer does not match"), (&a[6], "nothing to say")] {
        assert_eq!(m["result"]["isError"], true, "{m}");
        assert!(m["result"]["content"][0]["text"].as_str().unwrap().contains(says), "{m}");
        assert!(m["result"].get("structuredContent").is_none(), "{m}");
    }
    assert_eq!(a[7]["error"]["code"], -32601);
    // What happened at the provider goes with a failure: nothing, unless
    // it says otherwise.
    assert!(a[6]["result"].get("_meta").is_none());
    assert_eq!(a[8]["result"]["_meta"], json!({ OUTCOME_META: "unknown" }));
    assert_eq!(a[9]["result"]["_meta"], json!({ OUTCOME_META: "needs_scope", SCOPES_META: ["s2"] }));
    assert_eq!((a[8]["result"]["isError"].as_bool(), a[9]["result"]["isError"].as_bool()), (Some(true), Some(true)));
}

/// A call the tool made whose answer does not match its output schema says
/// so in its result's outcome, not only in its words: the provider may have
/// done it, and the appliance decides what that means by the tool's class
/// (#193). One refused before it ran says nothing happened.
#[test]
fn an_answer_that_breaks_its_schema_says_the_call_was_made() {
    let a = talk(&[call(1, json!({ "say": "wrong" }), Some("tok")), call(2, json!({ "say": 7 }), Some("tok")), call(3, json!({ "say": "hi" }), None)]);
    assert_eq!(a[0]["result"]["isError"], true, "{}", a[0]);
    assert_eq!(a[0]["result"]["_meta"], json!({ OUTCOME_META: BAD_ANSWER }), "{}", a[0]);
    for m in &a[1..] {
        assert_eq!(m["result"]["isError"], true, "{m}");
        assert!(m["result"].get("_meta").is_none(), "refused before it ran, yet it says it was made: {m}");
    }
}

/// A provider's side of one exchange: what it was sent, and its answer,
/// after which its reads end as `end` says (`None`: a clean end).
struct Peer(Vec<u8>, std::io::Cursor<Vec<u8>>, Option<std::io::ErrorKind>);
impl Read for Peer {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match (self.1.read(b)?, self.2) {
            (0, Some(k)) => Err(k.into()),
            (n, _) => Ok(n),
        }
    }
}
impl Write for Peer {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn answered(answer: &str, end: Option<std::io::ErrorKind>) -> Result<Response, String> {
    exchange_over(Peer(vec![], std::io::Cursor::new(answer.as_bytes().to_vec()), end), "GET", "api.example", "/v1/x", "tok", None)
}

/// One request carries the token and the body, and the answer's status,
/// headers and body come back, a chunked body joined.
#[test]
fn an_exchange_sends_the_token_and_reads_a_chunked_answer() {
    let answer = b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n2\r\n1}\r\n0\r\n\r\n".to_vec();
    let mut p = Peer(vec![], std::io::Cursor::new(answer), None);
    let r = exchange_over(&mut p, "POST", "api.example", "/v1/x", "tok", Some(&json!({ "b": 2 }))).unwrap();
    assert_eq!((r.status, r.header("retry-after"), r.body.as_slice()), (429, Some("7"), &b"{\"a\":1}"[..]));
    let sent = String::from_utf8(p.0).unwrap();
    assert!(sent.starts_with("POST /v1/x HTTP/1.1\r\nHost: api.example\r\nAuthorization: Bearer tok\r\n"), "{sent}");
    assert!(sent.ends_with("\r\n\r\n{\"b\":2}"), "{sent}");
}

/// A connection that fails while the answer comes is an error; a TLS
/// peer's end without its closing alert is not, once the answer is
/// whole.
#[test]
fn a_read_that_fails_is_an_error() {
    let whole = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi";
    assert!(answered(whole, Some(std::io::ErrorKind::ConnectionReset)).unwrap_err().contains("reading api.example's answer"));
    assert_eq!(answered(whole, Some(std::io::ErrorKind::UnexpectedEof)).unwrap().body, b"hi");
    assert!(answered("HTTP/1.1 200 OK\r\n\r\nhi", Some(std::io::ErrorKind::UnexpectedEof)).unwrap_err().contains("cut short"));
    assert_eq!(answered("HTTP/1.1 200 OK\r\n\r\nhi", None).unwrap().body, b"hi");
    assert_eq!(answered("HTTP/1.1 204 No Content\r\n\r\n", Some(std::io::ErrorKind::UnexpectedEof)).unwrap().body, b"");
}

/// A body shorter than its Content-Length is an error.
#[test]
fn a_body_shorter_than_its_length_is_an_error() {
    for end in [None, Some(std::io::ErrorKind::UnexpectedEof)] {
        assert!(answered("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n{\"a\":", end).unwrap_err().contains("cut short"));
    }
    assert_eq!(answered("HTTP/1.1 200 OK\r\ncontent-length: 7\r\n\r\n{\"a\":1}", None).unwrap().body, b"{\"a\":1}");
}

/// A chunked body that does not end with its last, empty chunk is an
/// error, as is one whose chunk is short or whose size is not one.
#[test]
fn a_chunked_body_without_its_last_chunk_is_an_error() {
    let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    for body in ["5\r\n{\"a\":\r\n", "5\r\n{\"a\":\r\n2\r\n1", "5\r\n{\"a\"", "zz\r\nhello\r\n0\r\n\r\n", ""] {
        for end in [None, Some(std::io::ErrorKind::UnexpectedEof)] {
            assert!(answered(&format!("{head}{body}"), end).unwrap_err().contains("cut short"), "{body:?}");
        }
    }
    assert_eq!(answered(&format!("{head}5\r\n{{\"a\":\r\n2\r\n1}}\r\n0\r\n\r\n"), None).unwrap().body, b"{\"a\":1}");
}

/// A CA of the test's own, and a certificate from it for `name`.
fn ca() -> (rcgen::Certificate, rcgen::KeyPair) {
    let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    p.distinguished_name.push(rcgen::DnType::CommonName, "A test's own CA");
    let k = rcgen::KeyPair::generate().unwrap();
    (p.self_signed(&k).unwrap(), k)
}

/// A local host that takes one connection and keeps what it read: over
/// TLS with a certificate for `name` from `ca`, or in the clear. It
/// answers a request with a greeting.
fn provider(tls: Option<(&(rcgen::Certificate, rcgen::KeyPair), &str)>) -> (std::net::TcpStream, std::sync::mpsc::Receiver<Vec<u8>>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let server = tls.map(|((ca, ca_key), name)| {
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap().signed_by(&key, ca, ca_key).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let c = rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth();
        Arc::new(c.with_single_cert(vec![cert.der().clone()], rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into())).unwrap())
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (tcp, _) = l.accept().unwrap();
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        // In the clear, it answers whatever it is first sent, as an HTTP
        // server answers what it cannot read.
        fn serve(mut s: impl Read + Write, clear: bool) -> Vec<u8> {
            let mut got = vec![];
            let mut b = [0u8; 4096];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") && !(clear && !got.is_empty()) {
                match s.read(&mut b) {
                    Ok(n) if n > 0 => got.extend_from_slice(&b[..n]),
                    _ => return got,
                }
            }
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
            let _ = s.flush();
            got
        }
        let got = match server {
            Some(c) => {
                let mut s = rustls::StreamOwned::new(rustls::ServerConnection::new(c).unwrap(), tcp);
                let got = serve(&mut s, false);
                s.conn.send_close_notify();
                let _ = s.flush();
                got
            }
            None => serve(tcp, true),
        };
        let _ = tx.send(got);
    });
    (std::net::TcpStream::connect(addr).unwrap(), rx)
}

/// The way out sends nothing until the provider has shown a certificate
/// for its name from a trusted root: not to a host in the clear, nor to
/// one with another name's certificate or an unknown CA's. Then the
/// request reaches it, token and all, and its answer comes back.
#[test]
fn nothing_is_sent_until_the_provider_proves_its_name() {
    let (ours, theirs) = (ca(), ca());
    let trusted = client(vec![ours.0.der().to_vec()]);
    let way_out = |sock, name: &str| secure(sock, name, trusted.clone()).and_then(|s| exchange_over(s, "GET", name, "/v1/x", "tok-secret", None));
    let (s, seen) = provider(None);
    let e = way_out(s, "api.example").unwrap_err();
    assert!(e.contains("could not reach api.example securely"), "{e}");
    let sent = seen.recv().unwrap();
    assert!(!sent.windows(10).any(|w| w == b"tok-secret") && !sent.starts_with(b"GET"), "sent in the clear: {}", String::from_utf8_lossy(&sent));
    for (ca, name) in [(&theirs, "api.example"), (&ours, "other.example")] {
        let (s, seen) = provider(Some((ca, name)));
        let e = way_out(s, "api.example").unwrap_err();
        assert!(e.contains("could not reach api.example securely") && e.to_lowercase().contains("certificate"), "{e}");
        assert!(seen.recv().unwrap().is_empty(), "the provider was sent a request");
    }
    let (s, seen) = provider(Some((&ours, "api.example")));
    let r = way_out(s, "api.example").unwrap();
    assert_eq!((r.status, r.body.as_slice()), (200, &b"hello"[..]));
    let sent = String::from_utf8(seen.recv().unwrap()).unwrap();
    assert!(sent.starts_with("GET /v1/x HTTP/1.1\r\nHost: api.example\r\nAuthorization: Bearer tok-secret\r\n"), "{sent}");
}
