//! Gmail, brokered (connectors spec, Brokered API connectors; owner,
//! 2026-11-03: Google's hosted Gmail server cannot send email). An MCP
//! server over Gmail's REST API (`https://gmail.googleapis.com/gmail/v1`),
//! the `/init` of its own microVM for each person: each call comes with the
//! person's access token, goes to Gmail through the appliance's egress
//! proxy over TLS checked here, and leaves nothing behind. Its tools are
//! those a person uses in Gmail: find and read mail, write drafts (new,
//! replies and forwards), send a draft they approved, and label, file,
//! trash or report it. Every tool is classified in the catalog manifest
//! (`catalog/google-gmail.toml`); sending a draft and deleting one for good
//! ask the person first.

use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use magnus_brokered_kit::{exchange, https, serve, Failure, Response, Tool};
use serde_json::{json, Value};

const HOST: &str = "gmail.googleapis.com";

/// The scope every tool here works with (Gmail API, each method's
/// Authorization scopes: `gmail.modify` is among those of every one).
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// One request to Gmail as the person: its JSON answer, or what Gmail's
/// refusal means, in words a person can act on. A request that may have
/// changed something and has no clear answer has an unknown outcome.
fn gmail(method: &str, path: &str, token: &str, body: Option<&Value>) -> Result<Value, Failure> {
    let wrote = method != "GET";
    // Gmail is known to be Gmail before anything is sent (the kit's way out).
    let s = https(&format!("{HOST}:443")).map_err(|e| format!("{e}. Nothing was changed."))?;
    let r = exchange(s, method, HOST, &format!("/gmail/v1/users/me{path}"), token, body).map_err(|e| {
        if wrote {
            Failure::Unknown(format!("{e}: whether Gmail did this is unknown"))
        } else {
            Failure::NotDone(e)
        }
    })?;
    if r.status >= 400 {
        return Err(explain(&r, wrote));
    }
    read_body(&r.body, wrote)
}

/// A success answer as JSON (`{}` for none). One that is not JSON says
/// nothing of what was done, so after a write it is an unknown outcome:
/// tried again, it could send, label or trash twice. The same rule as
/// `brokered/google` (review of #126); Gmail kept its own copy of that
/// code and still said a write's unreadable success as not done.
fn read_body(body: &[u8], wrote: bool) -> Result<Value, Failure> {
    let text = String::from_utf8_lossy(body).to_string();
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).map_err(|_| {
        let what = format!("Gmail's answer was not JSON: {}", clip(&text));
        if wrote {
            Failure::Unknown(format!("{what}; whether it did this is unknown"))
        } else {
            Failure::NotDone(what)
        }
    })
}

/// What Gmail's refusal means (from AtonoRobotics/magnus-claude#117's
/// `rest::explain`, for Google's REST APIs).
fn explain(r: &Response, wrote: bool) -> Failure {
    let body = String::from_utf8_lossy(&r.body).to_string();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let reason =
        v["error"]["errors"][0]["reason"].as_str().or_else(|| v["error"]["details"].as_array().and_then(|d| d.iter().find_map(|x| x["reason"].as_str()))).unwrap_or_default();
    let message = v["error"]["message"].as_str().unwrap_or_default().trim();
    let blob = format!("{reason} {message}").to_ascii_lowercase();
    let wait = r.header("retry-after").map(|w| format!(" (in {w} seconds)")).unwrap_or_default();
    if r.status == 401 || reason == "authError" {
        return Failure::NotDone("Gmail rejected the sign-in: it has expired or was revoked. Sign in again. Nothing was changed.".into());
    }
    if r.status == 403
        && (blob.contains("accessnotconfigured") || blob.contains("service_disabled") || blob.contains("has not been used in project") || blob.contains("it is disabled"))
    {
        let why = if reason.is_empty() { "accessNotConfigured" } else { reason };
        return Failure::NotDone(format!(
            "The Gmail API is not turned on for this Google Cloud project ({why}). An admin enables it in the project's API library. Nothing was changed."
        ));
    }
    let challenge = r.header("www-authenticate").filter(|h| h.contains("insufficient_scope"));
    if r.status == 403
        && (challenge.is_some()
            || blob.contains("insufficientpermissions")
            || blob.contains("access_token_scope_insufficient")
            || blob.contains("insufficient authentication scopes"))
    {
        let wanted = challenge
            .and_then(|h| h.split("scope=\"").nth(1))
            .and_then(|t| t.split('"').next())
            .map(|t| t.split_whitespace().map(String::from).collect::<Vec<_>>())
            .filter(|w| !w.is_empty())
            .unwrap_or_else(|| vec![SCOPE.to_string()]);
        return Failure::NeedsScope("This sign-in does not include the access Gmail needs for this. Sign in again and allow it. Nothing was changed.".into(), wanted);
    }
    if r.status == 429 || blob.contains("ratelimitexceeded") || blob.contains("rate limit") {
        return Failure::NotDone(format!("Gmail is limiting requests. Try again shortly{wait}. Nothing was changed."));
    }
    if r.status == 404 {
        return Failure::NotDone("Gmail could not find that. Nothing was changed.".into());
    }
    if r.status >= 500 && wrote {
        return Failure::Unknown(format!("Gmail answered HTTP {} and whether it did this is unknown.", r.status));
    }
    let detail = if reason.is_empty() { clip(message) } else { format!("{reason}: {}", clip(message)) };
    Failure::NotDone(format!("Gmail refused this (HTTP {}{}). Nothing was changed.", r.status, if detail.is_empty() { String::new() } else { format!(", {detail}") }))
}

fn clip(s: &str) -> String {
    s.chars().take(180).collect()
}

fn s(a: &Value, k: &str) -> String {
    a[k].as_str().unwrap_or_default().to_string()
}

/// A path segment or query value, percent-encoded.
fn enc(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// An id Gmail gave, as it goes in a path.
fn id(a: &Value, k: &str) -> Result<String, String> {
    let v = s(a, k);
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("{k} is not a Gmail id"));
    }
    Ok(v)
}

fn query(pairs: &[(&str, Option<String>)]) -> String {
    let q: Vec<String> = pairs.iter().filter_map(|(k, v)| v.as_ref().filter(|v| !v.is_empty()).map(|v| format!("{k}={}", enc(v)))).collect();
    if q.is_empty() {
        String::new()
    } else {
        format!("?{}", q.join("&"))
    }
}

fn opt_num(a: &Value, k: &str) -> Option<String> {
    a[k].as_u64().map(|n| n.to_string())
}

fn header(m: &Value, name: &str) -> String {
    m["payload"]["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .and_then(|h| h["value"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn decode(data: &str) -> String {
    let d = URL_SAFE.decode(data).or_else(|_| URL_SAFE_NO_PAD.decode(data)).unwrap_or_default();
    String::from_utf8_lossy(&d).to_string()
}

/// A message's text: its first text/plain part, else its first text/html.
fn text_of(part: &Value, mime: &str) -> Option<String> {
    if part["mimeType"] == mime {
        if let Some(d) = part["body"]["data"].as_str() {
            return Some(decode(d));
        }
    }
    part["parts"].as_array().into_iter().flatten().find_map(|p| text_of(p, mime))
}

fn attachments(part: &Value, out: &mut Vec<Value>) {
    if let Some(f) = part["filename"].as_str().filter(|f| !f.is_empty()) {
        out.push(json!({ "filename": f, "mime_type": part["mimeType"], "size": part["body"]["size"] }));
    }
    for p in part["parts"].as_array().into_iter().flatten() {
        attachments(p, out);
    }
}

/// A message as a person reads it.
fn message(m: &Value) -> Value {
    let p = &m["payload"];
    let mut files = vec![];
    attachments(p, &mut files);
    json!({
        "id": m["id"], "thread_id": m["threadId"], "labels": m["labelIds"].as_array().cloned().unwrap_or_default(),
        "from": header(m, "From"), "to": header(m, "To"), "cc": header(m, "Cc"),
        // A draft's every recipient, shown before the person approves
        // sending it (a message received has none).
        "bcc": header(m, "Bcc"), "subject": header(m, "Subject"), "date": header(m, "Date"),
        "snippet": m["snippet"].as_str().unwrap_or_default(),
        "body": text_of(p, "text/plain").or_else(|| text_of(p, "text/html")).unwrap_or_default(),
        "attachments": files,
    })
}

fn list(a: &Value, k: &str) -> Vec<String> {
    a[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
}

/// A header's value, refused if it could start another header.
fn line(v: &str, what: &str) -> Result<String, String> {
    if v.contains('\r') || v.contains('\n') {
        return Err(format!("{what} cannot have a line break"));
    }
    Ok(v.to_string())
}

/// A subject in RFC 2047's encoded form when it is not plain ASCII.
fn subject(v: &str) -> Result<String, String> {
    let v = line(v, "the subject")?;
    Ok(if v.is_ascii() { v } else { format!("=?UTF-8?B?{}?=", STANDARD.encode(v.as_bytes())) })
}

/// An email (RFC 5322), as Gmail takes it: base64url.
#[allow(clippy::too_many_arguments)]
fn raw(to: &[String], cc: &[String], bcc: &[String], subj: &str, body: &str, reply: Option<(&str, &str)>) -> Result<String, String> {
    let mut h = vec![];
    for (name, who) in [("To", to), ("Cc", cc), ("Bcc", bcc)] {
        if !who.is_empty() {
            h.push(format!("{name}: {}", line(&who.join(", "), "an address")?));
        }
    }
    h.push(format!("Subject: {}", subject(subj)?));
    if let Some((in_reply_to, references)) = reply {
        h.push(format!("In-Reply-To: {}", line(in_reply_to, "In-Reply-To")?));
        h.push(format!("References: {}", line(references, "References")?));
    }
    h.extend(["MIME-Version: 1.0".to_string(), "Content-Type: text/plain; charset=UTF-8".into(), "Content-Transfer-Encoding: base64".into()]);
    let b = STANDARD.encode(body.as_bytes());
    let wrapped: Vec<&str> = b.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap()).collect();
    Ok(URL_SAFE.encode(format!("{}\r\n\r\n{}\r\n", h.join("\r\n"), wrapped.join("\r\n"))))
}

fn draft(d: &Value) -> Value {
    json!({ "draft_id": d["id"], "message_id": d["message"]["id"], "thread_id": d["message"]["threadId"] })
}

fn new_draft(token: &str, raw: String, thread: Option<String>) -> Result<Value, Failure> {
    let mut m = json!({ "raw": raw });
    if let Some(t) = thread {
        m["threadId"] = json!(t);
    }
    Ok(draft(&gmail("POST", "/drafts", token, Some(&json!({ "message": m })))?))
}

fn search_threads(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail(
        "GET",
        &format!(
            "/threads{}",
            query(&[("q", a["query"].as_str().map(String::from)), ("maxResults", opt_num(a, "max_results")), ("pageToken", a["page_token"].as_str().map(String::from))])
        ),
        t,
        None,
    )?;
    let threads: Vec<Value> =
        r["threads"].as_array().into_iter().flatten().map(|x| json!({ "thread_id": x["id"], "snippet": x["snippet"].as_str().unwrap_or_default() })).collect();
    Ok(json!({ "threads": threads, "next_page_token": r["nextPageToken"].as_str() }))
}

fn get_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail("GET", &format!("/threads/{}?format=full", id(a, "thread_id")?), t, None)?;
    Ok(json!({ "thread_id": r["id"], "messages": r["messages"].as_array().into_iter().flatten().map(message).collect::<Vec<_>>() }))
}

fn get_message(a: &Value, t: &str) -> Result<Value, Failure> {
    let raw = gmail("GET", &format!("/messages/{}?format=full", id(a, "message_id")?), t, None)?;
    let mut shown = message(&raw);
    shown["digest"] = json!(digest(&raw, t)?);
    Ok(shown)
}

fn list_drafts(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail(
        "GET",
        &format!(
            "/drafts{}",
            query(&[("q", a["query"].as_str().map(String::from)), ("maxResults", opt_num(a, "max_results")), ("pageToken", a["page_token"].as_str().map(String::from))])
        ),
        t,
        None,
    )?;
    Ok(json!({ "drafts": r["drafts"].as_array().into_iter().flatten().map(draft).collect::<Vec<_>>(), "next_page_token": r["nextPageToken"].as_str() }))
}

fn get_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail("GET", &format!("/drafts/{}?format=full", id(a, "draft_id")?), t, None)?;
    let mut m = message(&r["message"]);
    m["digest"] = json!(digest(&r["message"], t)?);
    Ok(json!({ "draft_id": r["id"], "message": m }))
}

/// A digest of every part of a message as it would be sent: each part's
/// type, file name and bytes, in order, an attachment's bytes fetched by
/// its id (its id is not stable; its bytes are). The approval holds for it,
/// so a change to a part the card does not show, such as the HTML part or
/// an attachment's contents, asks again (#177).
fn digest(m: &Value, t: &str) -> Result<String, Failure> {
    use sha2::Digest;
    fn walk(part: &Value, mid: &str, t: &str, h: &mut sha2::Sha256) -> Result<(), Failure> {
        let bytes = match (part["body"]["data"].as_str(), part["body"]["attachmentId"].as_str()) {
            (Some(d), _) => URL_SAFE.decode(d).or_else(|_| URL_SAFE_NO_PAD.decode(d)).unwrap_or_default(),
            (None, Some(aid)) => {
                let a = gmail("GET", &format!("/messages/{mid}/attachments/{aid}"), t, None)?;
                let d = a["data"].as_str().unwrap_or_default();
                URL_SAFE.decode(d).or_else(|_| URL_SAFE_NO_PAD.decode(d)).unwrap_or_default()
            }
            _ => vec![],
        };
        for field in [part["mimeType"].as_str().unwrap_or_default().as_bytes(), part["filename"].as_str().unwrap_or_default().as_bytes(), &bytes] {
            h.update((field.len() as u64).to_be_bytes());
            h.update(field);
        }
        for p in part["parts"].as_array().into_iter().flatten() {
            walk(p, mid, t, h)?;
        }
        Ok(())
    }
    let mut h = sha2::Sha256::new();
    walk(&m["payload"], m["id"].as_str().unwrap_or_default(), t, &mut h)?;
    Ok(STANDARD.encode(h.finalize()))
}

fn create_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    new_draft(t, raw(&list(a, "to"), &list(a, "cc"), &list(a, "bcc"), &s(a, "subject"), &s(a, "body"), None)?, None)
}

/// A draft rewritten with the edit's recipients, subject and text, in the
/// thread and with the reply headers it had: an edited reply stays one.
fn rewritten(existing: &Value, a: &Value) -> Result<Value, String> {
    let m = &existing["message"];
    let in_reply_to = header(m, "In-Reply-To");
    let refs = header(m, "References");
    let refs = if refs.is_empty() { in_reply_to.clone() } else { refs };
    let reply = (!in_reply_to.is_empty()).then_some((in_reply_to.as_str(), refs.as_str()));
    let mut msg = json!({ "raw": raw(&list(a, "to"), &list(a, "cc"), &list(a, "bcc"), &s(a, "subject"), &s(a, "body"), reply)? });
    if let Some(t) = m["threadId"].as_str() {
        msg["threadId"] = json!(t);
    }
    Ok(msg)
}

fn update_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let d = id(a, "draft_id")?;
    let existing = gmail("GET", &format!("/drafts/{d}?format=metadata"), t, None)?;
    let body = json!({ "id": d, "message": rewritten(&existing, a)? });
    Ok(draft(&gmail("PUT", &format!("/drafts/{d}"), t, Some(&body))?))
}

fn delete_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    gmail("DELETE", &format!("/drafts/{}", id(a, "draft_id")?), t, None)?;
    Ok(json!({ "deleted": true }))
}

fn send_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail("POST", "/drafts/send", t, Some(&json!({ "id": id(a, "draft_id")? })))?;
    Ok(json!({ "message_id": r["id"], "thread_id": r["threadId"], "labels": r["labelIds"].as_array().cloned().unwrap_or_default() }))
}

/// The message answered or forwarded, with the headers that place a reply
/// in its thread.
fn original(a: &Value, t: &str) -> Result<Value, Failure> {
    gmail("GET", &format!("/messages/{}?format=full", id(a, "message_id")?), t, None)
}

fn re(prefix: &str, subj: &str) -> String {
    if subj.to_ascii_lowercase().starts_with(&format!("{}:", prefix.to_ascii_lowercase())) {
        subj.to_string()
    } else {
        format!("{prefix}: {subj}")
    }
}

fn reply_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let m = original(a, t)?;
    let from = header(&m, "Reply-To");
    let from = if from.is_empty() { header(&m, "From") } else { from };
    let mut cc = vec![];
    if a["reply_all"] == true {
        cc = [header(&m, "To"), header(&m, "Cc")].iter().flat_map(|h| h.split(',').map(|x| x.trim().to_string())).filter(|x| !x.is_empty()).collect();
    }
    let mid = header(&m, "Message-ID");
    let refs = format!("{} {mid}", header(&m, "References")).trim().to_string();
    let r = raw(&[from], &cc, &[], &re("Re", &header(&m, "Subject")), &s(a, "body"), Some((&mid, &refs)))?;
    new_draft(t, r, m["threadId"].as_str().map(String::from))
}

fn forward_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let m = message(&original(a, t)?);
    let quoted = format!(
        "{}\n\n---------- Forwarded message ---------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{}",
        s(a, "body"),
        m["from"].as_str().unwrap_or_default(),
        m["date"].as_str().unwrap_or_default(),
        m["subject"].as_str().unwrap_or_default(),
        m["to"].as_str().unwrap_or_default(),
        m["body"].as_str().unwrap_or_default()
    );
    new_draft(t, raw(&list(a, "to"), &list(a, "cc"), &[], &re("Fwd", m["subject"].as_str().unwrap_or_default()), &quoted, None)?, None)
}

fn list_labels(_: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail("GET", "/labels", t, None)?;
    Ok(json!({ "labels": r["labels"].as_array().into_iter().flatten().map(|l| json!({ "label_id": l["id"], "name": l["name"], "type": l["type"] })).collect::<Vec<_>>() }))
}

fn create_label(a: &Value, t: &str) -> Result<Value, Failure> {
    let r = gmail("POST", "/labels", t, Some(&json!({ "name": s(a, "name"), "labelListVisibility": "labelShow", "messageListVisibility": "show" })))?;
    Ok(json!({ "label_id": r["id"], "name": r["name"] }))
}

/// Labels on or off a message or a thread.
fn modify(kind: &str, key: &str, a: &Value, t: &str, add: &[String], remove: &[String]) -> Result<Value, Failure> {
    let r = gmail("POST", &format!("/{kind}/{}/modify", id(a, key)?), t, Some(&json!({ "addLabelIds": add, "removeLabelIds": remove })))?;
    Ok(json!({ "id": r["id"], "labels": r["labelIds"].as_array().cloned().or_else(|| r["messages"][0]["labelIds"].as_array().cloned()).unwrap_or_default() }))
}

fn move_to(kind: &str, key: &str, a: &Value, t: &str, action: &str) -> Result<Value, Failure> {
    let r = gmail("POST", &format!("/{kind}/{}/{action}", id(a, key)?), t, None)?;
    Ok(json!({ "id": r["id"], "labels": r["labelIds"].as_array().cloned().unwrap_or_default() }))
}

fn update_message_labels(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("messages", "message_id", a, t, &list(a, "add_label_ids"), &list(a, "remove_label_ids"))
}
fn label_message(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("messages", "message_id", a, t, &list(a, "label_ids"), &[])
}
fn unlabel_message(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("messages", "message_id", a, t, &[], &list(a, "label_ids"))
}
fn label_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("threads", "thread_id", a, t, &list(a, "label_ids"), &[])
}
fn unlabel_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("threads", "thread_id", a, t, &[], &list(a, "label_ids"))
}
fn trash_message(a: &Value, t: &str) -> Result<Value, Failure> {
    move_to("messages", "message_id", a, t, "trash")
}
fn untrash_message(a: &Value, t: &str) -> Result<Value, Failure> {
    move_to("messages", "message_id", a, t, "untrash")
}
fn trash_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    move_to("threads", "thread_id", a, t, "trash")
}
fn untrash_thread(a: &Value, t: &str) -> Result<Value, Failure> {
    move_to("threads", "thread_id", a, t, "untrash")
}
fn spam() -> Vec<String> {
    vec!["SPAM".into()]
}
fn inbox() -> Vec<String> {
    vec!["INBOX".into()]
}
fn mark_message_spam(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("messages", "message_id", a, t, &spam(), &inbox())
}
fn unmark_message_spam(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("messages", "message_id", a, t, &inbox(), &spam())
}
fn mark_thread_spam(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("threads", "thread_id", a, t, &spam(), &inbox())
}
fn unmark_thread_spam(a: &Value, t: &str) -> Result<Value, Failure> {
    modify("threads", "thread_id", a, t, &inbox(), &spam())
}

/// An object schema: its properties, which are required, and nothing else.
fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let addrs = json!({ "type": "array", "items": { "type": "string" } });
    let page = json!({ "query": st, "max_results": n, "page_token": st });
    let one = |k: &str| obj(json!({ k: st }), &[k]);
    let labels = |k: &str| obj(json!({ k: st, "label_ids": addrs }), &[k, "label_ids"]);
    let changed = obj(json!({ "id": st, "labels": addrs }), &["id", "labels"]);
    let maybe = json!({ "type": ["string", "null"] });
    let drafted = obj(json!({ "draft_id": st, "message_id": maybe, "thread_id": maybe }), &["draft_id"]);
    let msg = json!({ "type": "object", "required": ["id", "thread_id", "from", "subject", "body"] });
    let compose = obj(json!({ "to": addrs, "cc": addrs, "bcc": addrs, "subject": st, "body": st }), &["to", "subject", "body"]);
    let mut update = compose.clone();
    update["properties"]["draft_id"] = st.clone();
    update["required"] = json!(["draft_id", "to", "subject", "body"]);
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "search_threads",
            "Find threads in the person's mail with Gmail's search (from:, subject:, is:unread, after:, ...).",
            obj(page.clone(), &[]),
            obj(json!({ "threads": { "type": "array" }, "next_page_token": { "type": ["string", "null"] } }), &["threads"]),
            search_threads,
        ),
        tool(
            "get_thread",
            "Read a whole thread: each message's sender, recipients, subject, date and text.",
            one("thread_id"),
            obj(json!({ "thread_id": st, "messages": { "type": "array", "items": msg } }), &["thread_id", "messages"]),
            get_thread,
        ),
        tool("get_message", "Read one message: its sender, recipients, subject, date, text and the names of its attachments.", one("message_id"), msg.clone(), get_message),
        tool(
            "list_drafts",
            "List the person's drafts.",
            obj(page, &[]),
            obj(json!({ "drafts": { "type": "array", "items": drafted }, "next_page_token": { "type": ["string", "null"] } }), &["drafts"]),
            list_drafts,
        ),
        tool("get_draft", "Read a draft.", one("draft_id"), obj(json!({ "draft_id": st, "message": msg }), &["draft_id", "message"]), get_draft),
        tool("create_draft", "Write a new email as a draft in the person's mailbox. Nothing is sent.", compose, drafted.clone(), create_draft),
        tool("update_draft", "Rewrite a draft. Nothing is sent.", update, drafted.clone(), update_draft),
        tool(
            "delete_draft",
            "Delete a draft for good (it does not go to the trash).",
            one("draft_id"),
            obj(json!({ "deleted": { "type": "boolean" } }), &["deleted"]),
            delete_draft,
        ),
        tool(
            "send_draft",
            "Send a draft to its recipients, once the person has approved it.",
            one("draft_id"),
            obj(json!({ "message_id": st, "thread_id": st, "labels": addrs }), &["message_id"]),
            send_draft,
        ),
        tool(
            "reply_draft",
            "Write a reply to a message as a draft in its thread, to its sender (or everyone on it). Nothing is sent.",
            obj(json!({ "message_id": st, "body": st, "reply_all": { "type": "boolean" } }), &["message_id", "body"]),
            drafted.clone(),
            reply_draft,
        ),
        tool(
            "forward_draft",
            "Write a forward of a message, with its text, as a draft. Its attachments are not included. Nothing is sent.",
            obj(json!({ "message_id": st, "to": addrs, "cc": addrs, "body": st }), &["message_id", "to"]),
            drafted,
            forward_draft,
        ),
        tool("list_labels", "List the person's labels, Gmail's own and theirs.", obj(json!({}), &[]), obj(json!({ "labels": { "type": "array" } }), &["labels"]), list_labels),
        tool("create_label", "Make a new label.", one("name"), obj(json!({ "label_id": st, "name": st }), &["label_id"]), create_label),
        tool(
            "update_message_labels",
            "Put some labels on a message and take others off, at once.",
            obj(json!({ "message_id": st, "add_label_ids": addrs, "remove_label_ids": addrs }), &["message_id"]),
            changed.clone(),
            update_message_labels,
        ),
        tool("label_message", "Put labels on a message.", labels("message_id"), changed.clone(), label_message),
        tool("unlabel_message", "Take labels off a message.", labels("message_id"), changed.clone(), unlabel_message),
        tool("label_thread", "Put labels on every message of a thread.", labels("thread_id"), changed.clone(), label_thread),
        tool("unlabel_thread", "Take labels off every message of a thread.", labels("thread_id"), changed.clone(), unlabel_thread),
        tool("trash_message", "Move a message to the trash.", one("message_id"), changed.clone(), trash_message),
        tool("untrash_message", "Take a message out of the trash.", one("message_id"), changed.clone(), untrash_message),
        tool("trash_thread", "Move a thread to the trash.", one("thread_id"), changed.clone(), trash_thread),
        tool("untrash_thread", "Take a thread out of the trash.", one("thread_id"), changed.clone(), untrash_thread),
        tool("mark_message_spam", "Report a message as spam.", one("message_id"), changed.clone(), mark_message_spam),
        tool("unmark_message_spam", "Move a message out of spam, to the inbox.", one("message_id"), changed.clone(), unmark_message_spam),
        tool("mark_thread_spam", "Report a thread as spam.", one("thread_id"), changed.clone(), mark_thread_spam),
        tool("unmark_thread_spam", "Move a thread out of spam, to the inbox.", one("thread_id"), changed, unmark_thread_spam),
    ]
}

fn main() {
    serve("magnus-gmail", tools())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_email_is_written_as_gmail_takes_it_and_no_header_can_be_added() {
        let r = raw(&["a@example.com".into()], &[], &[], "Grüße", "Hello", Some(("<1@x>", "<0@x> <1@x>"))).unwrap();
        let text = String::from_utf8(URL_SAFE.decode(r).unwrap()).unwrap();
        assert!(text.starts_with("To: a@example.com\r\nSubject: =?UTF-8?B?"), "{text}");
        assert!(text.contains("In-Reply-To: <1@x>\r\nReferences: <0@x> <1@x>\r\n"));
        assert!(text.ends_with(&format!("\r\n\r\n{}\r\n", STANDARD.encode("Hello"))));
        assert!(raw(&["a@example.com\r\nBcc: b@example.com".into()], &[], &[], "s", "b", None).is_err());
        assert!(raw(&["a@example.com".into()], &[], &[], "s\nBcc: b@example.com", "b", None).is_err());
    }

    #[test]
    fn a_message_is_read_as_a_person_reads_it() {
        let m = json!({ "id": "m1", "threadId": "t1", "labelIds": ["INBOX"], "snippet": "hi", "payload": { "mimeType": "multipart/mixed", "headers": [{ "name": "From", "value": "Bo <bo@example.com>" }, { "name": "subject", "value": "Plans" }],
            "parts": [{ "mimeType": "multipart/alternative", "parts": [{ "mimeType": "text/html", "body": { "data": URL_SAFE.encode("<p>x</p>") } }, { "mimeType": "text/plain", "body": { "data": URL_SAFE.encode("Hello there") } }] },
                      { "mimeType": "application/pdf", "filename": "plan.pdf", "body": { "size": 10 } }] } });
        let v = message(&m);
        assert_eq!((v["from"].as_str(), v["subject"].as_str(), v["body"].as_str()), (Some("Bo <bo@example.com>"), Some("Plans"), Some("Hello there")));
        assert_eq!(v["attachments"][0]["filename"], "plan.pdf");
        assert_eq!(re("Re", "re: Plans"), "re: Plans");
        assert_eq!(re("Re", "Plans"), "Re: Plans");
        assert_eq!(enc("from:a b&c"), "from%3Aa%20b%26c");
    }

    /// A draft is read with every recipient, Bcc too, as the person
    /// approves sending it.
    #[test]
    fn a_draft_is_read_with_every_recipient() {
        let hs = |h: &[(&str, &str)]| h.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>();
        let d = json!({ "id": "dm1", "threadId": "t1", "payload": { "mimeType": "text/plain", "headers": hs(&[("To", "a@example.com"), ("Cc", "c@example.com"), ("Bcc", "boss@example.com"), ("Subject", "Plan")]), "body": { "data": URL_SAFE.encode("Hi") } } });
        let v = message(&d);
        assert_eq!((v["to"].as_str(), v["cc"].as_str(), v["bcc"].as_str()), (Some("a@example.com"), Some("c@example.com"), Some("boss@example.com")));
    }

    /// A reply's draft, edited, stays in its thread with the headers that
    /// make it a reply; a new draft's has none to keep.
    #[test]
    fn an_edited_reply_stays_a_reply() {
        let hs = |h: &[(&str, &str)]| h.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>();
        let reply = json!({ "id": "d1", "message": { "id": "dm1", "threadId": "t1", "payload": { "headers": hs(&[("To", "bo@example.com"), ("Subject", "Re: Plans"), ("In-Reply-To", "<1@x>"), ("References", "<0@x> <1@x>")]) } } });
        let edit = json!({ "draft_id": "d1", "to": ["bo@example.com"], "bcc": ["boss@example.com"], "subject": "Re: Plans", "body": "Friday works." });
        let m = rewritten(&reply, &edit).unwrap();
        assert_eq!(m["threadId"], "t1");
        let text = String::from_utf8(URL_SAFE.decode(m["raw"].as_str().unwrap()).unwrap()).unwrap();
        assert!(text.contains("Bcc: boss@example.com\r\n") && text.contains("In-Reply-To: <1@x>\r\nReferences: <0@x> <1@x>\r\n"), "{text}");
        let fresh = json!({ "id": "d2", "message": { "id": "dm2", "threadId": "t2", "payload": { "headers": hs(&[("To", "bo@example.com"), ("Subject", "Plans")]) } } });
        let m = rewritten(&fresh, &edit).unwrap();
        let text = String::from_utf8(URL_SAFE.decode(m["raw"].as_str().unwrap()).unwrap()).unwrap();
        assert!(!text.contains("In-Reply-To") && !text.contains("References"), "{text}");
        assert_eq!(m["threadId"], "t2");
    }

    fn answer(status: u16, head: &str, body: Value) -> Response {
        Response { status, head: format!("HTTP/1.1 {status} X\r\n{head}"), body: body.to_string().into_bytes() }
    }

    /// Gmail's refusals are said as what the person or an admin does about
    /// them, and a write with no clear answer is not said to have failed
    /// (AtonoRobotics/magnus-claude#117's cases, as Google's REST APIs give
    /// them).
    #[test]
    fn gmails_refusals_are_said_as_what_to_do() {
        let says = |f: Failure| match f {
            Failure::NotDone(t) => t,
            other => panic!("{other:?}"),
        };
        let gone = json!({ "error": { "code": 401, "message": "Request had invalid authentication credentials.", "status": "UNAUTHENTICATED" } });
        assert!(says(explain(&answer(401, "", gone), false)).contains("Sign in again"));
        let off = json!({ "error": { "code": 403, "message": "Gmail API has not been used in project 123 before or it is disabled.", "errors": [{ "reason": "accessNotConfigured" }], "status": "PERMISSION_DENIED" } });
        assert!(says(explain(&answer(403, "", off), false)).contains("not turned on for this Google Cloud project (accessNotConfigured)"));
        let scope = json!({ "error": { "code": 403, "message": "Request had insufficient authentication scopes.", "errors": [{ "reason": "insufficientPermissions" }], "details": [{ "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] } });
        let challenge = "WWW-Authenticate: Bearer realm=\"https://accounts.google.com/\", error=\"insufficient_scope\", scope=\"https://www.googleapis.com/auth/gmail.modify\"\r\n";
        assert!(matches!(explain(&answer(403, challenge, scope.clone()), true), Failure::NeedsScope(_, s) if s == [SCOPE]));
        assert!(matches!(explain(&answer(403, "", scope), true), Failure::NeedsScope(_, s) if s == [SCOPE]));
        let busy = json!({ "error": { "code": 429, "errors": [{ "reason": "rateLimitExceeded" }] } });
        assert!(says(explain(&answer(429, "Retry-After: 30\r\n", busy), true)).contains("Try again shortly (in 30 seconds)"));
        assert!(says(explain(&answer(404, "", json!({})), false)).contains("could not find"));
        assert!(matches!(explain(&answer(503, "", json!({})), true), Failure::Unknown(_)));
        assert!(says(explain(&answer(503, "", json!({})), false)).contains("HTTP 503"));
    }

    /// A write Gmail answered with something that is not JSON is an unknown
    /// outcome, not a failure to try again (review of #126, which fixed this
    /// in `brokered/google` and left Gmail's copy saying the write had not
    /// happened). A read's unreadable answer is still not done.
    #[test]
    fn an_answer_that_is_not_json_after_a_write_is_an_unknown_outcome() {
        assert!(matches!(read_body(b"<html>ok</html>", true), Err(Failure::Unknown(m)) if m.contains("unknown")));
        assert!(matches!(read_body(b"<html>ok</html>", false), Err(Failure::NotDone(_))));
        assert!(read_body(b"", true).is_ok());
        assert_eq!(read_body(br#"{"id":"m1"}"#, true).unwrap()["id"], "m1");
    }

    /// Tool drift (connectors spec, Required tests): the catalog manifest
    /// classes exactly the tools this server lists.
    #[test]
    fn every_tool_is_classed_in_the_manifest() {
        let m: toml::Table = include_str!("../../../catalog/google-gmail.toml").parse().unwrap();
        let mut classed: Vec<&str> = m["tools"].as_table().unwrap().keys().map(String::as_str).collect();
        let mut listed: Vec<&str> = tools().iter().map(|t| t.name).collect();
        classed.sort();
        listed.sort();
        assert_eq!(classed, listed);
    }

    #[test]
    fn every_tool_names_its_schemas() {
        let ts = tools();
        assert_eq!(ts.len(), 26);
        for t in &ts {
            assert_eq!(t.input["type"], "object", "{}", t.name);
            assert_eq!(t.output["type"], "object", "{}", t.name);
        }
        // Ids go in paths only as Gmail makes them.
        assert!(id(&json!({ "m": "../labels" }), "m").is_err());
        assert!(id(&json!({ "m": "18c2a9f0e1b2c3d4" }), "m").is_ok());
    }
}
