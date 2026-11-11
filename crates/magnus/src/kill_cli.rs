//! `magnus kill --config <file>` and `magnus kill --config <file> --release`:
//! the server-level kill switch from the server itself (spec rev 141,
//! Administration: "magnus kill on the server itself still stops everything
//! when the app or sign-in is broken").
//!
//! It speaks to the running server over its control socket
//! (`<state_dir>/control.sock`, readable and writable by the server's own
//! user alone), so it needs neither the app, the network nor anyone's
//! sign-in. Pressing moves the same switch the app's top bar does
//! (`Kernel::kill_switch`): new work and running work stop at once, the vault
//! seals and sign-ins to connected apps are revoked. Releasing it from here
//! is allowed because only someone already in control of the server reaches
//! the socket, which is a stronger check than the app's step-up.
//!
//! Each is recorded twice, as the app's are: the switch's own event, with
//! the actor `server:<user>` naming the server's local operator, and an
//! admin audit entry (`kill_switch_engaged` / `kill_switch_released`, source
//! `server`). Pressing moves the switch first and records after, because
//! stopping must not wait on anything; releasing records first.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long the server has to answer on its control socket before
/// `magnus kill` says it did not. A press stops every microVM and revokes
/// each sign-in at its provider before it answers (each provider call may
/// take 30 seconds), and one switch waits for another: giving up sooner
/// would leave a press that happened unaudited. The operator can still stop
/// waiting with Ctrl-C.
pub const KILL_ANSWER_TIMEOUT_SECONDS: u64 = 300;

/// The most of the server's answer read back: far above any it gives.
pub const KILL_ANSWER_MAX_BYTES: u64 = 1024 * 1024;

/// The prefix of the log actor naming the server's local operator.
pub const OPERATOR_ACTOR_PREFIX: &str = "server:";

/// Who runs this on the server: the local user's name, from the process's
/// own uid (`/proc/self`) and `/etc/passwd`, else `uid <n>`.
pub fn operator() -> String {
    use std::os::unix::fs::MetadataExt;
    let Ok(uid) = std::fs::metadata("/proc/self").map(|m| m.uid()) else { return "unknown".into() };
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    user_of(&passwd, uid).unwrap_or_else(|| format!("uid {uid}"))
}

/// The user name `/etc/passwd`'s text gives `uid`.
fn user_of(passwd: &str, uid: u32) -> Option<String> {
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() > 2 && f[2].parse::<u32>().ok() == Some(uid)).then(|| f[0].to_string())
    })
}

/// The log actor for `operator`.
pub fn actor(operator: &str) -> String {
    format!("{OPERATOR_ACTOR_PREFIX}{operator}")
}

/// How the app names the operator behind `actor`, when it is one.
pub fn operator_name(actor: &str) -> Option<String> {
    actor.strip_prefix(OPERATOR_ACTOR_PREFIX).map(|u| format!("{u} on the server"))
}

/// One JSON request to the control socket in `state_dir`: the answer's
/// status and body.
pub async fn call(state_dir: &Path, method: &str, path: &str, body: &Value) -> Result<(u16, Value)> {
    let sock = crate::control::socket_path(state_dir);
    let wait = std::time::Duration::from_secs(KILL_ANSWER_TIMEOUT_SECONDS);
    let exchange = async {
        let mut s = tokio::net::UnixStream::connect(&sock).await.with_context(|| format!("reach the server at {}", sock.display()))?;
        let b = body.to_string();
        let req = format!("{method} {path} HTTP/1.1\r\nHost: magnus\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
        s.write_all(req.as_bytes()).await.context("send to the server")?;
        let mut raw = vec![];
        s.take(KILL_ANSWER_MAX_BYTES).read_to_end(&mut raw).await.context("read the server's answer")?;
        anyhow::Ok(raw)
    };
    let raw = tokio::time::timeout(wait, exchange).await.map_err(|_| anyhow::anyhow!("the server did not answer within {KILL_ANSWER_TIMEOUT_SECONDS} seconds"))??;
    parse(&raw)
}

/// An HTTP/1.1 answer's status and JSON body (null when it has none).
fn parse(raw: &[u8]) -> Result<(u16, Value)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").context("the server's answer has no end of headers")?;
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).context("the server's answer has no status")?;
    Ok((status, serde_json::from_str(body).unwrap_or(Value::Null)))
}

/// The audit entry for a move of the switch by `operator`.
fn audit(operator: &str, engaged: bool, after: Value) -> Value {
    let action = if engaged { "kill_switch_engaged" } else { "kill_switch_released" };
    json!({ "admin": actor(operator), "name": operator_name(&actor(operator)), "action": action, "target": "server", "source": "server", "after": after })
}

/// Press (or with `release`, release) the server's switch as `operator`,
/// through the control socket in `state_dir`. What to tell the operator.
pub async fn kill(state_dir: &Path, operator: &str, release: bool) -> Result<String> {
    let who = actor(operator);
    if release {
        let (st, v) = call(state_dir, "POST", "/v1/audit", &audit(operator, false, json!({ "engaged": false }))).await?;
        if !(200..300).contains(&st) {
            bail!("the release was not made: its audit entry could not be written ({})", v["error"].as_str().unwrap_or("no answer"));
        }
        let (st, v) = call(state_dir, "POST", "/v1/kill-switch", &json!({ "engaged": false, "actor": who })).await?;
        if !(200..300).contains(&st) {
            bail!("the server refused the release: {}", v["error"].as_str().unwrap_or("no answer"));
        }
        let told = v["told"].as_u64().unwrap_or(0);
        return Ok(format!("The kill switch is off. Nothing resumes by itself: {told} stopped task(s) wait for their people to continue them."));
    }
    let (st, v) = call(state_dir, "POST", "/v1/kill-switch", &json!({ "engaged": true, "actor": who })).await?;
    if !(200..300).contains(&st) {
        bail!("the server refused: {}", v["error"].as_str().unwrap_or("no answer"));
    }
    let (st, a) = call(state_dir, "POST", "/v1/audit", &audit(operator, true, v.clone())).await?;
    let note = if (200..300).contains(&st) { String::new() } else { format!(" Its audit entry could not be written ({}).", a["error"].as_str().unwrap_or("no answer")) };
    Ok(format!("The kill switch is on: everything on the server is stopped. Release it from the app's top bar or with `magnus kill --release`.{note}"))
}

#[cfg(test)]
mod tests {
    use super::{operator_name, parse, user_of};

    #[test]
    fn the_operator_is_named_from_passwd() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nmagnus:x:998:998::/var/lib/magnus:/usr/sbin/nologin\n";
        assert_eq!(user_of(passwd, 0).as_deref(), Some("root"));
        assert_eq!(user_of(passwd, 998).as_deref(), Some("magnus"));
        assert_eq!(user_of(passwd, 5), None);
        assert_eq!(operator_name("server:root").as_deref(), Some("root on the server"));
        assert_eq!(operator_name("admin:ana"), None);
    }

    #[test]
    fn an_answer_is_read() {
        let (st, v) = parse(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 17\r\n\r\n{\"engaged\":true}").unwrap();
        assert_eq!((st, v["engaged"].as_bool()), (200, Some(true)));
        assert!(parse(b"garbage").is_err());
    }
}
