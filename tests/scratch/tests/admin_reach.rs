//! Who can reach the admin console (admin console spec, invariant tests,
//! Reach): only the organisation's network and VPN, by its own hostname,
//! over TLS 1.3. It runs as its own process, `magnus-admin`, on the
//! appliance's address on the network; the tunnel reaches services over
//! loopback, where the console never listens or answers.

use magnus_invariants::appliance::bin;
use magnus_invariants::*;

use magnus_scratch::admin::*;
use std::net::Ipv4Addr;

/// The console refuses to start where its reach rules would not hold.
async fn refused(app: &Appliance, listen: &[String], allow: &[&str]) -> String {
    let (path, _) = config(app, admin(listen, allow));
    let out = tokio::process::Command::new(bin("magnus-admin")).arg("--config").arg(&path).output().await.unwrap();
    assert!(!out.status.success(), "magnus-admin started listening on {listen:?}, taking {allow:?}");
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_console_answers_only_its_own_network_by_its_own_name_over_tls_1_3() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let ip = lan_ip();
    let mine = format!("{ip}/32");
    let c = start(&app, &[&mine]).await;
    let url = |p: &str| format!("https://{HOST}:{}{p}", c.addr.port());

    // From the organisation's network, by its name: the console, hardened.
    let r = browser(&c, false).get(url("/")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let csp = r.headers()["content-security-policy"].to_str().unwrap().to_string();
    assert!(csp.contains("default-src 'none'") && csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"), "{csp}");
    let page = r.text().await.unwrap();
    assert!(page.contains("Acme") && page.contains("Admin console"), "{page}");
    assert!(!page.contains("https://") && !page.contains("http://"), "the page loads something from elsewhere: {page}");
    for asset in ["/assets/app.css", "/assets/theme.js", "/assets/admin.css", "/assets/admin.js"] {
        assert_eq!(browser(&c, false).get(url(asset)).send().await.unwrap().status(), 200, "{asset}");
    }
    let status: serde_json::Value = browser(&c, false).get(url("/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status, serde_json::json!({ "ok": true }), "the status before sign-in says more than that it answers");
    // Nothing else answers without signing in.
    for p in ["/api/v1/users", "/api/v1/health", "/api/v1/overview", "/api/v1/kill-switch"] {
        assert_eq!(browser(&c, false).get(url(p)).send().await.unwrap().status(), 401, "{p} answered without a sign-in");
    }
    for (m, p) in [
        ("POST", "/api/v1/users"),
        ("GET", "/api/v1/users/x"),
        ("PATCH", "/api/v1/users/x"),
        ("POST", "/api/v1/users/x/disable"),
        ("POST", "/api/v1/users/x/enable"),
        ("DELETE", "/api/v1/users/x/sessions"),
        ("DELETE", "/api/v1/users/x/sessions/y"),
        ("POST", "/api/v1/users/x/invite"),
        ("DELETE", "/api/v1/users/x/invite"),
    ] {
        let r = browser(&c, false).request(m.parse().unwrap(), url(p)).json(&serde_json::json!({ "role": "admin" })).send().await.unwrap();
        assert_eq!(r.status(), 401, "{m} {p} answered without a sign-in");
    }

    // Any other name is not found, the console not even confirmed.
    let r = browser(&c, false).get(url("/")).header("host", "magnus.pyrallon.ai").send().await.unwrap();
    assert_eq!(r.status(), 404);
    let r = browser(&c, false).get(url("/status")).header("host", format!("{ip}:{}", c.addr.port())).send().await.unwrap();
    assert_eq!(r.status(), 404);

    // TLS 1.3 only.
    assert!(browser(&c, true).get(url("/")).send().await.is_err(), "the console spoke TLS 1.2");

    // Loopback, where the tunnel reaches services, is never the console's.
    // Connecting to 127.0.0.1 on this port is not the check: another test's
    // listener can hold that address while this console listens elsewhere
    // (#143, run 41912463362).
    let pid = c.child.id().expect("the console is running");
    assert!(!process_listens_on_loopback(pid, c.addr.port()), "the console listens on loopback");
    // Nor is the user API, which the tunnel serves.
    for p in ["/assets/admin.js", "/status"] {
        let st = reqwest::get(format!("{}{p}", app.base)).await.unwrap().status();
        assert_eq!(st, 404, "the user API serves the console's {p}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_address_outside_the_allowed_subnets_is_refused() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    // This machine is outside the allowed VPN subnet: its connection is
    // dropped before TLS.
    let c = start(&app, &["198.51.100.0/24"]).await;
    let r = browser(&c, false).get(format!("https://{HOST}:{}/", c.addr.port())).send().await;
    assert!(r.is_err(), "a request from outside the allowed subnets was served: {:?}", r.map(|r| r.status()));
    // Inside them, it is served.
    let mine = format!("{}/32", lan_ip());
    let c = start(&app, &["198.51.100.0/24", &mine]).await;
    assert_eq!(browser(&c, false).get(format!("https://{HOST}:{}/", c.addr.port())).send().await.unwrap().status(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_console_will_not_listen_on_every_address_or_loopback_nor_take_loopback() {
    let m = FakeModel::start(steps(vec![])).await;
    let app = Appliance::start(Spec::default(), &m).await;
    let mine = format!("{}/32", lan_ip());
    for listen in ["0.0.0.0:0", "[::]:0", "127.0.0.1:0", "[::1]:0"] {
        let said = refused(&app, &[listen.to_string()], &[&mine]).await;
        assert!(said.contains("admin.listen"), "{listen}: {said}");
    }
    let here = format!("{}:0", lan_ip());
    for allow in ["127.0.0.0/8", "127.0.0.1", "0.0.0.0/0", "::1"] {
        let said = refused(&app, &[here.clone()], &[allow]).await;
        assert!(said.contains("admin.allow") && said.contains("loopback"), "{allow}: {said}");
    }
}

/// Inodes of TCP listeners on a loopback address at `port` (`/proc/net/tcp`).
fn loopback_listen_inodes(port: u16) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["tcp", "tcp6"] {
        let Ok(text) = std::fs::read_to_string(format!("/proc/net/{name}")) else { continue };
        for line in text.lines().skip(1) {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() < 10 || p[3] != "0A" {
                continue;
            }
            let Some((addr, pr)) = p[1].split_once(':') else { continue };
            let Ok(got) = u16::from_str_radix(pr, 16) else { continue };
            if got == port && is_loopback_proc_addr(addr) {
                out.push(p[9].to_string());
            }
        }
    }
    out
}

/// `/proc/net/tcp` prints an IPv4 address as a little-endian hex word.
/// `::1` in `/proc/net/tcp6` is the fixed word sequence below.
fn is_loopback_proc_addr(hex: &str) -> bool {
    if hex.len() == 8 {
        let Ok(n) = u32::from_str_radix(hex, 16) else { return false };
        return Ipv4Addr::from(n.swap_bytes()).is_loopback();
    }
    hex == "00000000000000000000000001000000"
}

/// Whether `pid` itself is listening on loopback at `port`.
fn process_listens_on_loopback(pid: u32, port: u16) -> bool {
    let inodes = loopback_listen_inodes(port);
    if inodes.is_empty() {
        return false;
    }
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { return false };
    for fd in fds.flatten() {
        let Ok(target) = std::fs::read_link(fd.path()) else { continue };
        let s = target.to_string_lossy();
        let Some(rest) = s.strip_prefix("socket:[") else { continue };
        let Some(inode) = rest.strip_suffix(']') else { continue };
        if inodes.iter().any(|i| i == inode) {
            return true;
        }
    }
    false
}

#[tokio::test]
async fn a_loopback_socket_on_the_consoles_port_is_not_the_console() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // The old check treated this as the console listening on loopback.
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok());
    assert!(process_listens_on_loopback(std::process::id(), port));
    assert!(!process_listens_on_loopback(1, port), "init does not hold this test's socket");
}
