//! Outlook mail, brokered (connectors spec, Brokered API connectors;
//! Microsoft has no remote mail server a public client can sign in to from
//! any organisation). An MCP server over Microsoft Graph's mail API
//! (`https://graph.microsoft.com/v1.0/me/messages`), the `/init` of its own
//! microVM for each person: each call comes with the person's access
//! token, goes to Graph through the appliance's egress proxy over TLS
//! checked here, and leaves nothing behind. Its tools are those a person
//! uses in Outlook: find and read mail, write drafts and replies, and send
//! a draft. Every tool is classified in the catalog manifest
//! (`catalog/microsoft-outlook.toml`); sending asks the person first, and
//! the gate reads who it reaches from the draft itself.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use magnus_brokered_kit::{serve, Failure, Tool};
use magnus_brokered_microsoft::{addresses, id, list, next_page, obj, opt_num, page, people, query, s, Graph};
use serde_json::{json, Value};

/// `Mail.ReadWrite` reads, searches and writes drafts; `Mail.Send` sends
/// (Microsoft Graph permissions reference: each method's least privileged
/// delegated permission). Neither can delete mail for good.
const API: Graph = Graph { name: "Outlook", scopes: &["https://graph.microsoft.com/Mail.ReadWrite", "https://graph.microsoft.com/Mail.Send"] };

/// Where every path this server asks for begins, and every page token.
const ME: &str = "/v1.0/me/";

/// The fields of a message in a list.
const SUMMARY: &str = "id,conversationId,subject,from,toRecipients,ccRecipients,receivedDateTime,isRead,isDraft,hasAttachments,bodyPreview";
/// And of one read whole.
const WHOLE: &str = "id,conversationId,subject,from,toRecipients,ccRecipients,bccRecipients,replyTo,receivedDateTime,sentDateTime,isRead,isDraft,hasAttachments,body";

/// The most messages one page of a list asks for, when the call names none.
const PAGE_SIZE_DEFAULT: u64 = 25;

/// A message in a list, as a person reads it.
fn summary(m: &Value) -> Value {
    json!({
        "message_id": m["id"], "conversation_id": m["conversationId"], "subject": m["subject"].as_str().unwrap_or_default(),
        "from": m["from"]["emailAddress"]["address"], "to": addresses(&m["toRecipients"]), "cc": addresses(&m["ccRecipients"]),
        "received": m["receivedDateTime"], "read": m["isRead"] == true, "draft": m["isDraft"] == true,
        "has_attachments": m["hasAttachments"] == true, "preview": m["bodyPreview"].as_str().unwrap_or_default(),
    })
}

/// A message read whole: its every recipient (a draft's Bcc too, shown
/// before the person approves sending it), its text and its attachments.
fn message(m: &Value) -> Value {
    let files: Vec<Value> = m["attachments"].as_array().into_iter().flatten().map(|a| json!({ "name": a["name"], "type": a["contentType"], "size": a["size"] })).collect();
    json!({
        "message_id": m["id"], "conversation_id": m["conversationId"], "subject": m["subject"].as_str().unwrap_or_default(),
        "from": m["from"]["emailAddress"]["address"], "to": addresses(&m["toRecipients"]), "cc": addresses(&m["ccRecipients"]),
        "bcc": addresses(&m["bccRecipients"]), "reply_to": addresses(&m["replyTo"]),
        "received": m["receivedDateTime"], "sent": m["sentDateTime"], "draft": m["isDraft"] == true,
        "body_type": m["body"]["contentType"].as_str().unwrap_or("text").to_ascii_lowercase(), "body": m["body"]["content"].as_str().unwrap_or_default(),
        "attachments": files,
    })
}

fn messages(path: String, a: &Value, t: &str) -> Result<Value, Failure> {
    let path = match a["page_token"].as_str().filter(|p| !p.is_empty()) {
        Some(p) => page(p, ME)?,
        None => path,
    };
    let r = API.call("GET", &path, t, None)?;
    Ok(json!({ "messages": r["value"].as_array().into_iter().flatten().map(summary).collect::<Vec<_>>(), "next_page_token": next_page(&r) }))
}

fn top(a: &Value) -> Option<String> {
    opt_num(a, "max_results").or(Some(PAGE_SIZE_DEFAULT.to_string()))
}

/// A folder by Outlook's own name for it (`inbox`, `sentitems`, `drafts`,
/// `archive`, `deleteditems`, `junkemail`) or its id.
fn folder(a: &Value) -> String {
    magnus_brokered_microsoft::enc(a["folder"].as_str().filter(|f| !f.is_empty()).unwrap_or("inbox"))
}

fn list_messages(a: &Value, t: &str) -> Result<Value, Failure> {
    let q = query(&[("$top", top(a)), ("$select", Some(SUMMARY.into())), ("$orderby", Some("receivedDateTime desc".into()))]);
    messages(format!("{ME}mailFolders/{}/messages{q}", folder(a)), a, t)
}

/// Words as Outlook searches them (KQL, such as `from:bo@example.com
/// budget`), across every folder, best match first.
fn search_messages(a: &Value, t: &str) -> Result<Value, Failure> {
    let words = s(a, "query").replace('"', "");
    let q = query(&[("$search", Some(format!("\"{words}\""))), ("$top", top(a)), ("$select", Some(SUMMARY.into()))]);
    messages(format!("{ME}messages{q}"), a, t)
}

fn whole(a: &Value, k: &str, t: &str) -> Result<Value, Failure> {
    let q = query(&[("$select", Some(WHOLE.into())), ("$expand", Some("attachments($select=name,contentType,size)".into()))]);
    API.call("GET", &format!("{ME}messages/{}{q}", id(a, k)?), t, None)
}

fn get_message(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(message(&whole(a, "message_id", t)?))
}

fn list_drafts(a: &Value, t: &str) -> Result<Value, Failure> {
    let q = query(&[("$top", top(a)), ("$select", Some(SUMMARY.into())), ("$orderby", Some("lastModifiedDateTime desc".into()))]);
    let mut r = messages(format!("{ME}mailFolders/drafts/messages{q}"), a, t)?;
    if let Some(ms) = r["messages"].as_array_mut() {
        for m in ms {
            m["draft_id"] = m["message_id"].clone();
        }
    }
    Ok(json!({ "drafts": r["messages"], "next_page_token": r["next_page_token"] }))
}

/// A draft as the gate reads it before a send: every recipient, the
/// subject, text and attachments the card shows, and a digest of what it
/// does not show.
fn get_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let m = whole(a, "draft_id", t)?;
    if m["isDraft"] != true {
        return Err("That message is not a draft. Nothing was changed.".to_string().into());
    }
    let files = API.call("GET", &format!("{ME}messages/{}/attachments", id(a, "draft_id")?), t, None)?;
    let mut shown = message(&m);
    shown["digest"] = json!(digest(&m, &files["value"]));
    Ok(json!({ "draft_id": m["id"], "message": shown }))
}

/// A digest of every part of a draft as it would be sent that the card
/// does not show whole: its body's type and every byte of it, who it is
/// from and who replies go to, and each attachment's type, name and bytes,
/// in an order of their own (their ids are not stable; their bytes are).
/// The approval holds for it, so a change to any of them asks again (#177).
fn digest(m: &Value, attachments: &Value) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    let mut field = |b: &[u8]| {
        h.update((b.len() as u64).to_be_bytes());
        h.update(b);
    };
    let from = m["from"]["emailAddress"]["address"].as_str().unwrap_or_default();
    let reply_to = addresses(&m["replyTo"]).join(",");
    for f in [m["body"]["contentType"].as_str().unwrap_or_default(), m["body"]["content"].as_str().unwrap_or_default(), from, &reply_to] {
        field(f.as_bytes());
    }
    let mut parts: Vec<Vec<Vec<u8>>> = attachments
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            // An attached item or link has no bytes here: its size and when
            // it last changed stand for them.
            let bytes = a["contentBytes"].as_str().and_then(|b| STANDARD.decode(b).ok()).unwrap_or_else(|| format!("{} {}", a["size"], a["lastModifiedDateTime"]).into_bytes());
            let kind = a["@odata.type"].as_str().unwrap_or_default();
            vec![kind.as_bytes().to_vec(), a["contentType"].as_str().unwrap_or_default().as_bytes().to_vec(), a["name"].as_str().unwrap_or_default().as_bytes().to_vec(), bytes]
        })
        .collect();
    parts.sort();
    for p in &parts {
        for f in p {
            field(f);
        }
    }
    STANDARD.encode(h.finalize())
}

/// The fields of a draft a call sets: only those it names, its text as
/// plain text.
fn draft_body(a: &Value) -> Value {
    let mut b = json!({});
    if let Some(v) = a["subject"].as_str() {
        b["subject"] = json!(v);
    }
    if let Some(v) = a["body"].as_str() {
        b["body"] = json!({ "contentType": "Text", "content": v });
    }
    for (k, field) in [("to", "toRecipients"), ("cc", "ccRecipients"), ("bcc", "bccRecipients")] {
        if a.get(k).is_some() {
            b[field] = people(&list(a, k));
        }
    }
    b
}

fn draft(m: &Value) -> Value {
    json!({ "draft_id": m["id"], "conversation_id": m["conversationId"] })
}

fn create_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    Ok(draft(&API.call("POST", &format!("{ME}messages"), t, Some(&draft_body(a)))?))
}

/// A draft changed: only a draft, never a message received or sent.
fn update_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let d = id(a, "draft_id")?;
    let m = API.call("GET", &format!("{ME}messages/{d}?$select=isDraft"), t, None)?;
    if m["isDraft"] != true {
        return Err("That message is not a draft, so it is not changed here. Nothing was changed.".to_string().into());
    }
    Ok(draft(&API.call("PATCH", &format!("{ME}messages/{d}"), t, Some(&draft_body(a)))?))
}

/// A reply as a draft, in the conversation, to its sender (or everyone on
/// it), with the text given above the message answered.
fn reply_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    let how = if a["reply_all"] == true { "createReplyAll" } else { "createReply" };
    let body = json!({ "comment": s(a, "body") });
    Ok(draft(&API.call("POST", &format!("{ME}messages/{}/{how}", id(a, "message_id")?), t, Some(&body))?))
}

fn send_draft(a: &Value, t: &str) -> Result<Value, Failure> {
    API.call("POST", &format!("{ME}messages/{}/send", id(a, "draft_id")?), t, None)?;
    Ok(json!({ "sent": true }))
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let yes = json!({ "type": "boolean" });
    let strs = json!({ "type": "array", "items": { "type": "string" } });
    let maybe = json!({ "type": ["string", "null"] });
    let summary = json!({ "type": "object", "required": ["message_id", "subject", "from", "received"] });
    let listed = obj(json!({ "messages": { "type": "array", "items": summary }, "next_page_token": maybe }), &["messages"]);
    let whole = json!({ "type": "object", "required": ["message_id", "subject", "to", "cc", "bcc", "body", "attachments"] });
    let made = obj(json!({ "draft_id": st, "conversation_id": maybe }), &["draft_id"]);
    let fields = json!({ "to": strs, "cc": strs, "bcc": strs, "subject": st, "body": st });
    let mut change = fields.clone();
    change["draft_id"] = st.clone();
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "list_messages",
            "List the messages in a folder, newest first: inbox unless named (sentitems, drafts, archive, deleteditems, junkemail, or a folder's id).",
            obj(json!({ "folder": st, "max_results": n, "page_token": st }), &[]),
            listed.clone(),
            list_messages,
        ),
        tool(
            "search_messages",
            "Find messages in every folder whose words match, as Outlook searches (from:, to:, subject:, or plain words).",
            obj(json!({ "query": st, "max_results": n, "page_token": st }), &["query"]),
            listed,
            search_messages,
        ),
        tool(
            "get_message",
            "Read one message: who it is from and to, its text and its attachments.",
            obj(json!({ "message_id": st }), &["message_id"]),
            whole.clone(),
            get_message,
        ),
        tool(
            "list_drafts",
            "List the person's drafts, the latest changed first.",
            obj(json!({ "max_results": n, "page_token": st }), &[]),
            obj(json!({ "drafts": { "type": "array" }, "next_page_token": maybe }), &["drafts"]),
            list_drafts,
        ),
        tool(
            "get_draft",
            "Read one draft as it would be sent: every recipient, its subject, text and attachments.",
            obj(json!({ "draft_id": st }), &["draft_id"]),
            obj(json!({ "draft_id": st, "message": whole }), &["draft_id", "message"]),
            get_draft,
        ),
        tool("create_draft", "Write a new draft in plain text. Nothing is sent.", obj(fields, &["to", "subject", "body"]), made.clone(), create_draft),
        tool("update_draft", "Change a draft: only what is given. Nothing is sent.", obj(change, &["draft_id"]), made.clone(), update_draft),
        tool(
            "reply_draft",
            "Write a reply to a message as a draft, to its sender (or everyone on it, with reply_all), the text given above the message answered. Nothing is sent.",
            obj(json!({ "message_id": st, "body": st, "reply_all": yes }), &["message_id", "body"]),
            made,
            reply_draft,
        ),
        tool(
            "send_draft",
            "Send a draft to everyone it is addressed to. Sent mail cannot be called back.",
            obj(json!({ "draft_id": st }), &["draft_id"]),
            obj(json!({ "sent": yes }), &["sent"]),
            send_draft,
        ),
    ]
}

fn main() {
    serve("magnus-outlook", tools())
}

#[cfg(test)]
#[path = "../../../tests/brokered-outlook/outlook.rs"]
mod tests;
