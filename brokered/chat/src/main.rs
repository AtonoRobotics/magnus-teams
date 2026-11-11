//! Google Chat, brokered (connectors spec, Brokered API connectors;
//! Google's hosted Chat server refuses Magnus with 403, as Gmail's did). An
//! MCP server over Chat's REST API (`https://chat.googleapis.com/v1`), the
//! `/init` of its own microVM for each person: each call comes with the
//! person's access token, goes to Google through the appliance's egress
//! proxy over TLS checked here, and leaves nothing behind. Its tools are
//! those a person uses in Chat: find a conversation and who is in it, read
//! and search its messages, and send, edit or delete their own. Every tool
//! is classified in the catalog manifest (`catalog/google-chat.toml`); each
//! that others see asks the person first.

use magnus_brokered_google::{enc, opt_num, query, s, Api};
use magnus_brokered_kit::{serve, Failure, Tool};
use serde_json::{json, Value};

/// The scope a refusal asks for when Google names none: sending, editing
/// and deleting messages (`chat.messages`). The manifest asks for that
/// and the two to list spaces and members.
const API: Api = Api { host: "chat.googleapis.com", base: "/v1", name: "Google Chat", scope: "https://www.googleapis.com/auth/chat.messages" };
// Google's published UserInfo endpoint identifies the account behind the
// current access token. A caller-supplied user ID cannot establish ownership.
const IDENTITY: Api = Api { host: "openidconnect.googleapis.com", base: "/v1", name: "Google account", scope: "openid" };

/// The most pages of messages a search reads (100 each).
const PAGES: usize = 5;

/// A resource's name as Chat gives it (`spaces/AAA`, or
/// `spaces/AAA/messages/BBB` for a message), and nothing that would leave
/// it in the path. A bare space id is taken as that space.
fn name(a: &Value, key: &str, message: bool) -> Result<String, String> {
    let v = s(a, key);
    let v = if v.starts_with("spaces/") { v } else { format!("spaces/{v}") };
    let seg = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) && x != "." && x != "..";
    let parts: Vec<&str> = v.split('/').collect();
    let ok = match parts.as_slice() {
        ["spaces", sp] => !message && seg(sp),
        ["spaces", sp, "messages", m] => message && seg(sp) && seg(m),
        _ => false,
    };
    if !ok {
        return Err(format!("{key} is {} as Chat names it", if message { "a message (spaces/…/messages/…)" } else { "a space (spaces/…)" }));
    }
    Ok(v)
}

/// A message as a person reads it.
fn message(m: &Value) -> Value {
    json!({ "message_id": m["name"], "from": m["sender"]["displayName"].as_str().or(m["sender"]["name"].as_str()), "text": m["text"].as_str().unwrap_or_default(), "sent": m["createTime"], "thread_id": m["thread"]["name"], "edited": m["lastUpdateTime"].is_string() })
}

fn space(sp: &Value) -> Value {
    json!({ "space_id": sp["name"], "name": sp["displayName"].as_str().unwrap_or_default(), "type": sp["spaceType"], "external": sp["externalUserAllowed"] == true, "last_active": sp["lastActiveTime"] })
}

fn search_conversations(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = API.call(
        "GET",
        &format!("/spaces{}", query(&[("pageSize", opt_num(a, "max_results").or(Some("100".into()))), ("pageToken", a["page_token"].as_str().map(String::from))])),
        t,
        None,
    )?;
    let needle = s(a, "query").to_lowercase();
    let spaces: Vec<Value> = r["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|sp| needle.is_empty() || sp["displayName"].as_str().unwrap_or_default().to_lowercase().contains(&needle))
        .map(space)
        .collect();
    Ok(json!({ "spaces": spaces, "next_page_token": r["nextPageToken"].as_str() }))
}

fn list_members(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = API.call(
        "GET",
        &format!("/{}/members{}", name(a, "space_id", false)?, query(&[("pageSize", Some("1000".into())), ("pageToken", a["page_token"].as_str().map(String::from))])),
        t,
        None,
    )?;
    let people: Vec<Value> = r["memberships"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| json!({ "member": m["member"]["displayName"].as_str().or(m["member"]["name"].as_str()), "user": m["member"]["name"], "role": m["role"] }))
        .collect();
    Ok(json!({ "members": people, "next_page_token": r["nextPageToken"].as_str() }))
}

/// Chat's filter on when messages were sent.
fn when(a: &Value) -> Option<String> {
    let mut f = vec![];
    if let Some(x) = a["after"].as_str().filter(|x| !x.is_empty()) {
        f.push(format!("createTime > \"{}\"", x.replace('"', "")));
    }
    if let Some(x) = a["before"].as_str().filter(|x| !x.is_empty()) {
        f.push(format!("createTime < \"{}\"", x.replace('"', "")));
    }
    (!f.is_empty()).then(|| f.join(" AND "))
}

fn page(sp: &str, a: &Value, t: &str, size: String, token: Option<String>) -> Result<Value, Failure> {
    API.call(
        "GET",
        &format!("/{sp}/messages{}", query(&[("pageSize", Some(size)), ("pageToken", token), ("filter", when(a)), ("orderBy", Some("createTime desc".into()))])),
        t,
        None,
    )
}

fn list_messages(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = page(&name(a, "space_id", false)?, a, t, opt_num(a, "max_results").unwrap_or("25".into()), a["page_token"].as_str().map(String::from))?;
    Ok(json!({ "messages": r["messages"].as_array().into_iter().flatten().map(message).collect::<Vec<_>>(), "next_page_token": r["nextPageToken"].as_str() }))
}

/// Messages in a space whose text has the words, newest first, from up to
/// `PAGES` pages (Chat's API filters messages by time, not by words).
fn search_messages(a: &Value, t: &str) -> Result<Value, Failure> {
    let sp = name(a, "space_id", false)?;
    let needle = s(a, "query").to_lowercase();
    let (mut found, mut next) = (vec![], None);
    for _ in 0..PAGES {
        let r = page(&sp, a, t, "100".into(), next.take())?;
        found.extend(r["messages"].as_array().into_iter().flatten().filter(|m| m["text"].as_str().unwrap_or_default().to_lowercase().contains(&needle)).map(message));
        next = r["nextPageToken"].as_str().map(String::from);
        if next.is_none() {
            break;
        }
    }
    Ok(json!({ "messages": found, "more": next.is_some() }))
}

fn get_message(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(message(&API.call("GET", &format!("/{}", name(a, "message_id", true)?), t, None)?))
}

/// Where a message goes: a space named, or the direct conversation with a
/// person by email (one there already).
fn to(a: &Value, t: &str) -> Result<String, Failure> {
    if let Some(email) = a["to_email"].as_str().filter(|x| !x.is_empty()) {
        let r = API.call("GET", &format!("/spaces:findDirectMessage?name={}", enc(&format!("users/{email}"))), t, None)?;
        return r["name"].as_str().map(String::from).ok_or_else(|| Failure::NotDone(format!("There is no direct conversation with {email} yet. Nothing was sent.")));
    }
    Ok(name(a, "space_id", false)?)
}

fn send_message(a: &Value, t: &str) -> Result<Value, Failure> {
    let text = s(a, "text");
    if text.is_empty() {
        return Err("text is the message to send".to_string().into());
    }
    let sp = to(a, t)?;
    let mut body = json!({ "text": text });
    let mut path = format!("/{sp}/messages");
    if let Some(th) = a["thread_id"].as_str().filter(|x| !x.is_empty()) {
        body["thread"] = json!({ "name": th });
        path.push_str("?messageReplyOption=REPLY_MESSAGE_FALLBACK_TO_NEW_THREAD");
    }
    Ok(message(&API.call("POST", &path, t, Some(&body))?))
}

fn update_message(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(message(&API.call("PATCH", &format!("/{}?updateMask=text", name(a, "message_id", true)?), t, Some(&json!({ "text": s(a, "text") })))?))
}

fn delete_message(a: &Value, t: &str) -> Result<Value, Failure> {
    let path = format!("/{}", name(a, "message_id", true)?);
    let person = IDENTITY.call("GET", "/userinfo", t, None)?;
    let current = API.call("GET", &path, t, None)?;
    if !owns_message(&person, &current) {
        return Err(Failure::NotDone("Magnus can only delete messages sent by the signed-in person. Ownership could not be verified. Nothing was deleted.".into()));
    }
    API.call("DELETE", &path, t, None)?;
    Ok(json!({ "deleted": true }))
}

fn owns_message(person: &Value, current: &Value) -> bool {
    person["sub"]
        .as_str()
        .filter(|id| !id.is_empty() && !id.contains('/'))
        .is_some_and(|id| current["sender"]["type"] == "HUMAN" && current["sender"]["name"].as_str() == Some(format!("users/{id}").as_str()))
}

/// An object schema: its properties, which are required, and nothing else.
fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let yes = json!({ "type": "boolean" });
    let msg = json!({ "type": "object", "required": ["message_id", "text"] });
    let msgs = obj(json!({ "messages": { "type": "array", "items": msg }, "next_page_token": { "type": ["string", "null"] }, "more": yes }), &["messages"]);
    let one = obj(json!({ "message_id": st }), &["message_id"]);
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "search_conversations",
            "List the person's spaces, group conversations and direct messages, each with when it last had a message, those whose name has the words if given.",
            obj(json!({ "query": st, "max_results": n, "page_token": st }), &[]),
            obj(json!({ "spaces": { "type": "array" }, "next_page_token": { "type": ["string", "null"] } }), &["spaces"]),
            search_conversations,
        ),
        tool(
            "list_members",
            "List who is in a space. Use next_page_token as page_token to read more members.",
            obj(json!({ "space_id": st, "page_token": st }), &["space_id"]),
            obj(json!({ "members": { "type": "array" }, "next_page_token": { "type": ["string", "null"] } }), &["members"]),
            list_members,
        ),
        tool(
            "list_messages",
            "List a space's messages, newest first, sent after or before a time (RFC 3339) if given.",
            obj(json!({ "space_id": st, "after": st, "before": st, "max_results": n, "page_token": st }), &["space_id"]),
            msgs.clone(),
            list_messages,
        ),
        tool(
            "search_messages",
            "Find messages in a space whose text has the words, newest first, from its last 500.",
            obj(json!({ "space_id": st, "query": st, "after": st, "before": st }), &["space_id", "query"]),
            msgs,
            search_messages,
        ),
        tool("get_message", "Read one message.", one.clone(), msg.clone(), get_message),
        tool(
            "send_message",
            "Send a message as the person to a space, or to a person by email (a conversation there already), in a thread if given. Everyone in it sees it.",
            obj(json!({ "space_id": st, "to_email": st, "text": st, "thread_id": st }), &["text"]),
            msg.clone(),
            send_message,
        ),
        tool(
            "update_message",
            "Change the text of a message the person sent. Everyone in the space sees it was edited.",
            obj(json!({ "message_id": st, "text": st }), &["message_id", "text"]),
            msg,
            update_message,
        ),
        tool("delete_message", "Delete a message the person sent, for everyone.", one, obj(json!({ "deleted": yes }), &["deleted"]), delete_message),
    ]
}

fn main() {
    serve("magnus-chat", tools())
}

#[cfg(test)]
#[path = "../../../tests/brokered-chat/chat.rs"]
mod tests;
