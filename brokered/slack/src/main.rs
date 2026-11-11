//! Slack, brokered (connectors spec, Brokered API connectors). An MCP
//! server over Slack's Web API (`https://slack.com/api`), the `/init` of
//! its own microVM for each person: each call comes with the person's
//! user access token, goes to Slack through the appliance's egress proxy
//! over TLS checked here, and leaves nothing behind. It searches messages,
//! reads channels and direct messages the person is in, and posts as that
//! person. Every tool is classified in the catalog manifest
//! (`catalog/slack.toml`); posting asks the person first, since the people
//! in the conversation are told.

use magnus_brokered_kit::{exchange, https, serve, Failure, Response, Tool};
use serde_json::{json, Value};

const HOST: &str = "slack.com";

/// One request to Slack as the person. A request that may have posted and
/// has no clear answer has an unknown outcome.
fn slack(method: &str, path: &str, token: &str, body: Option<&Value>, wrote: bool, scopes: &[&str]) -> Result<Value, Failure> {
    let s = https(&format!("{HOST}:443")).map_err(|e| format!("{e}. Nothing was changed."))?;
    let r =
        exchange(s, method, HOST, path, token, body).map_err(|e| if wrote { Failure::Unknown(format!("{e}: whether Slack did this is unknown")) } else { Failure::NotDone(e) })?;
    interpret(&r, wrote, scopes)
}

/// Slack's answer: JSON with `ok`, or what the refusal means. A write
/// answered with something that is not JSON is an unknown outcome.
fn interpret(r: &Response, wrote: bool, scopes: &[&str]) -> Result<Value, Failure> {
    if r.status >= 500 && wrote {
        return Err(Failure::Unknown(format!("Slack answered HTTP {} and whether it did this is unknown.", r.status)));
    }
    let text = String::from_utf8_lossy(&r.body).to_string();
    if text.trim().is_empty() {
        return Err(if wrote {
            Failure::Unknown("Slack gave no answer and whether it did this is unknown.".into())
        } else {
            Failure::NotDone(format!("Slack gave no answer (HTTP {}). Nothing was changed.", r.status))
        });
    }
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => {
            let what = format!("Slack's answer was not JSON: {}", clip(&text));
            return Err(if wrote { Failure::Unknown(format!("{what}; whether it did this is unknown")) } else { Failure::NotDone(what) });
        }
    };
    let error = v["error"].as_str().unwrap_or("");
    if r.status == 429 || error == "ratelimited" {
        let wait = r.header("retry-after").map(|w| format!(" (in {w} seconds)")).unwrap_or_default();
        return Err(Failure::NotDone(format!("Slack is limiting requests. Try again shortly{wait}. Nothing was changed.")));
    }
    if r.status >= 400 || v["ok"] == false {
        return Err(explain(&v, r.status, wrote, scopes));
    }
    Ok(v)
}

fn explain(v: &Value, status: u16, wrote: bool, scopes: &[&str]) -> Failure {
    let error = v["error"].as_str().unwrap_or("");
    if status == 401 || matches!(error, "invalid_auth" | "token_revoked" | "token_expired" | "not_authed" | "account_inactive") {
        return Failure::NotDone("Slack rejected the sign-in: it has expired or was revoked. Sign in again. Nothing was changed.".into());
    }
    if error == "missing_scope" {
        let wanted = v["needed"]
            .as_str()
            .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>())
            .filter(|w| !w.is_empty())
            .unwrap_or_else(|| scopes.iter().map(|s| (*s).to_string()).collect());
        return Failure::NeedsScope("This sign-in does not include the access Slack needs for this. Sign in again and allow it. Nothing was changed.".into(), wanted);
    }
    if wrote && (status >= 500 || matches!(error, "internal_error" | "fatal_error")) {
        return Failure::Unknown(format!("Slack answered {error} and whether it did this is unknown."));
    }
    let detail = if error.is_empty() { format!("HTTP {status}") } else { error.to_string() };
    Failure::NotDone(format!("Slack refused this ({detail}). Nothing was changed."))
}

fn clip(s: &str) -> String {
    s.chars().take(180).collect()
}

fn s(a: &Value, k: &str) -> String {
    a[k].as_str().unwrap_or_default().to_string()
}

fn enc(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A Slack id or channel name, as it goes in a query. A slash would leave
/// the method's path.
fn slack_id(a: &Value, k: &str) -> Result<String, String> {
    let v = s(a, k);
    // A timestamp has a dot (`1700000000.000100`). A slash would leave the query.
    if v.is_empty() || v.len() > 80 || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err(format!("{k} is not a Slack id"));
    }
    Ok(v)
}

fn limit(a: &Value) -> String {
    let n = a["max_results"].as_u64().unwrap_or(20).clamp(1, 100);
    n.to_string()
}

fn cursor(v: &Value) -> Option<&str> {
    v["response_metadata"]["next_cursor"].as_str().filter(|c| !c.is_empty())
}

fn search_messages(a: &Value, t: &str) -> Result<Value, Failure> {
    let q = s(a, "query");
    if q.is_empty() {
        return Err("Say what to search for.".to_string().into());
    }
    let page = a["page"].as_u64().unwrap_or(1).clamp(1, 100);
    let mut path = format!("/api/search.messages?query={}&count={}&page={page}", enc(&q), limit(a));
    if a["newest_first"] == true {
        path.push_str("&sort=timestamp&sort_dir=desc");
    }
    let r = slack("GET", &path, t, None, false, &["search:read"])?;
    Ok(json!({
        "messages": r["messages"]["matches"].as_array().into_iter().flatten().map(search_row).collect::<Vec<_>>(),
        "total": r["messages"]["total"],
        "page": r["messages"]["pagination"]["page"],
        "pages": r["messages"]["pagination"]["pages"],
    }))
}

fn search_row(m: &Value) -> Value {
    json!({
        "text": m["text"].as_str().unwrap_or_default(),
        "user": m["user"], "username": m["username"], "ts": m["ts"],
        "channel_id": m["channel"]["id"], "channel": m["channel"]["name"],
        "permalink": m["permalink"],
    })
}

fn kind_of(c: &Value) -> &'static str {
    if c["is_im"] == true {
        "direct"
    } else if c["is_mpim"] == true {
        "group"
    } else if c["is_private"] == true {
        "private"
    } else {
        "channel"
    }
}

fn list_conversations(a: &Value, t: &str) -> Result<Value, Failure> {
    let mut path = format!("/api/conversations.list?types=public_channel,private_channel,im,mpim&exclude_archived=true&limit={}", limit(a));
    if let Some(c) = a["cursor"].as_str().filter(|c| !c.is_empty()) {
        path.push_str(&format!("&cursor={}", enc(c)));
    }
    let r = slack("GET", &path, t, None, false, &["channels:read", "groups:read", "im:read", "mpim:read"])?;
    let conversations = r["channels"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            json!({
                "id": c["id"], "name": c["name"].as_str().or(c["user"].as_str()).unwrap_or_default(),
                "kind": kind_of(c), "member": c["is_member"] != false,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "conversations": conversations, "next_cursor": cursor(&r) }))
}

fn messages(r: &Value) -> Vec<Value> {
    r["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            json!({
                "user": m["user"], "text": m["text"].as_str().unwrap_or_default(), "ts": m["ts"], "thread_ts": m["thread_ts"],
            })
        })
        .collect()
}

fn read_conversation(a: &Value, t: &str) -> Result<Value, Failure> {
    let id = slack_id(a, "channel")?;
    let mut path = format!("/api/conversations.history?channel={}&limit={}", enc(&id), limit(a));
    if let Some(c) = a["cursor"].as_str().filter(|c| !c.is_empty()) {
        path.push_str(&format!("&cursor={}", enc(c)));
    }
    let r = slack("GET", &path, t, None, false, &["channels:history", "groups:history", "im:history", "mpim:history"])?;
    Ok(json!({ "messages": messages(&r), "next_cursor": cursor(&r) }))
}

fn read_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    let id = slack_id(a, "channel")?;
    let ts = slack_id(a, "ts")?;
    let path = format!("/api/conversations.replies?channel={}&ts={}&limit={}", enc(&id), enc(&ts), limit(a));
    let r = slack("GET", &path, t, None, false, &["channels:history", "groups:history", "im:history", "mpim:history"])?;
    Ok(json!({ "messages": messages(&r) }))
}

fn post_message(a: &Value, t: &str) -> Result<Value, Failure> {
    let channel = slack_id(a, "channel")?;
    let text = s(a, "text");
    if text.trim().is_empty() {
        return Err("A message needs text. Nothing was sent.".to_string().into());
    }
    let mut body = json!({ "channel": channel, "text": text });
    if let Some(ts) = a["thread_ts"].as_str().filter(|ts| !ts.is_empty()) {
        slack_id(&json!({ "thread_ts": ts }), "thread_ts")?;
        body["thread_ts"] = json!(ts);
    }
    let r = slack("POST", "/api/chat.postMessage", t, Some(&body), true, &["chat:write"])?;
    Ok(json!({ "channel": r["channel"], "ts": r["ts"] }))
}

fn lookup_user(a: &Value, t: &str) -> Result<Value, Failure> {
    let id = slack_id(a, "user")?;
    let r = slack("GET", &format!("/api/users.info?user={}", enc(&id)), t, None, false, &["users:read"])?;
    let u = &r["user"];
    Ok(json!({
        "user_id": u["id"].as_str().unwrap_or_default(),
        "name": u["name"].as_str().unwrap_or_default(),
        "real_name": u["real_name"].as_str().unwrap_or_default(),
        "display_name": u["profile"]["display_name"].as_str().unwrap_or_default(),
    }))
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let maybe = json!({ "type": ["string", "null"] });
    let msg = json!({ "type": "object" });
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "search_messages",
            "Search the person's Slack messages, best match first, or newest first when newest_first is true.",
            obj(json!({ "query": st, "max_results": n, "page": n, "newest_first": { "type": "boolean" } }), &["query"]),
            obj(
                json!({ "messages": { "type": "array", "items": msg }, "total": { "type": ["integer", "null"] }, "page": { "type": ["integer", "null"] }, "pages": { "type": ["integer", "null"] } }),
                &["messages"],
            ),
            search_messages,
        ),
        tool(
            "list_conversations",
            "List channels, private channels and direct messages the person is in.",
            obj(json!({ "max_results": n, "cursor": st }), &[]),
            obj(json!({ "conversations": { "type": "array" }, "next_cursor": maybe }), &["conversations"]),
            list_conversations,
        ),
        tool(
            "read_conversation",
            "Read recent messages in a channel or direct message the person is in.",
            obj(json!({ "channel": st, "max_results": n, "cursor": st }), &["channel"]),
            obj(json!({ "messages": { "type": "array", "items": msg }, "next_cursor": maybe }), &["messages"]),
            read_conversation,
        ),
        tool(
            "read_thread",
            "Read a thread.",
            obj(json!({ "channel": st, "ts": st, "max_results": n }), &["channel", "ts"]),
            obj(json!({ "messages": { "type": "array", "items": msg } }), &["messages"]),
            read_thread,
        ),
        tool(
            "post_message",
            "Post a message as the person, once they have approved it. It is sent to everyone in that conversation.",
            obj(json!({ "channel": st, "text": st, "thread_ts": st }), &["channel", "text"]),
            obj(json!({ "channel": st, "ts": st }), &["channel", "ts"]),
            post_message,
        ),
        tool(
            "lookup_user",
            "The name of a person in the workspace.",
            obj(json!({ "user": st }), &["user"]),
            obj(json!({ "user_id": st, "name": st, "real_name": st, "display_name": st }), &["user_id"]),
            lookup_user,
        ),
    ]
}

fn main() {
    serve("magnus-slack", tools())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, head: &str, body: Value) -> Response {
        Response { status, head: format!("HTTP/1.1 {status} X\r\n{head}"), body: body.to_string().into_bytes() }
    }

    #[test]
    fn slacks_refusals_are_said_as_what_to_do() {
        let dead = json!({ "ok": false, "error": "invalid_auth" });
        assert!(matches!(interpret(&answer(200, "", dead), false, &[]), Err(Failure::NotDone(t)) if t.contains("Sign in again")));
        let scope = json!({ "ok": false, "error": "missing_scope", "needed": "chat:write" });
        assert!(matches!(interpret(&answer(200, "", scope), true, &["chat:write"]), Err(Failure::NeedsScope(_, s)) if s == ["chat:write"]));
        let busy = json!({ "ok": false, "error": "ratelimited" });
        assert!(matches!(interpret(&answer(429, "Retry-After: 2\r\n", busy), false, &[]), Err(Failure::NotDone(t)) if t.contains("in 2 seconds")));
        let junk = Response { status: 200, head: "HTTP/1.1 200 X\r\n".into(), body: b"<html>ok</html>".to_vec() };
        assert!(matches!(interpret(&junk, true, &[]), Err(Failure::Unknown(t)) if t.contains("unknown")));
        assert!(matches!(interpret(&junk, false, &[]), Err(Failure::NotDone(_))));
        assert!(matches!(interpret(&answer(500, "", json!({})), true, &[]), Err(Failure::Unknown(_))));
        let ok = json!({ "ok": true, "channel": "C1", "ts": "1.2" });
        assert_eq!(interpret(&answer(200, "", ok), true, &[]).unwrap()["ts"], "1.2");
    }

    #[test]
    fn a_message_is_read_as_a_person_reads_it_and_a_post_needs_text() {
        let r = json!({ "ok": true, "messages": { "total": 1, "pagination": { "page": 1, "pages": 1 }, "matches": [{ "text": "Friday", "user": "U1", "username": "ada", "ts": "1", "channel": { "id": "C1", "name": "general" }, "permalink": "https://example.slack.com/archives/C1/p1" }] } });
        let row = search_row(&r["messages"]["matches"][0]);
        assert_eq!((row["text"].as_str(), row["channel"].as_str(), row["username"].as_str()), (Some("Friday"), Some("general"), Some("ada")));
        assert!(slack_id(&json!({ "channel": "C1" }), "channel").is_ok());
        assert!(slack_id(&json!({ "ts": "1700000000.000100" }), "ts").is_ok());
        assert!(slack_id(&json!({ "channel": "../x" }), "channel").is_err());
        assert!(post_message(&json!({ "channel": "C1", "text": "  " }), "t").is_err());
    }

    #[test]
    fn every_tool_is_classed_in_the_manifest() {
        let m: toml::Table = include_str!("../../../catalog/slack.toml").parse().unwrap();
        let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
        let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
        classed.sort();
        listed.sort();
        assert_eq!(classed, listed);
    }

    #[test]
    fn every_tool_names_its_schemas() {
        for t in tools() {
            assert_eq!(t.input["type"], "object", "{}", t.name);
            assert_eq!(t.output["type"], "object", "{}", t.name);
        }
    }
}
