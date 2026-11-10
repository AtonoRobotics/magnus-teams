//! OneDrive, brokered (connectors spec, Brokered API connectors). An MCP
//! server over Microsoft Graph's files API
//! (`https://graph.microsoft.com/v1.0/me/drive`), the `/init` of its own
//! microVM for each person: each call comes with the person's access
//! token, goes to Graph through the appliance's egress proxy over TLS
//! checked here, and leaves nothing behind. Its tools find, list and read
//! the person's files, and upload a new one; it never overwrites, moves,
//! shares or deletes a file. Every tool is classified in the catalog
//! manifest (`catalog/microsoft-onedrive.toml`).
//!
//! A file's bytes come from Graph's `contentStream` (beta), which answers
//! them itself. `/content` (v1.0) answers with a redirect to the
//! organisation's own SharePoint host, which differs for each organisation
//! and so cannot be among the exact hosts the manifest names.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use magnus_brokered_kit::{serve, Body, Failure, Tool};
use magnus_brokered_microsoft::{enc, id, next_page, obj, opt_num, page, query, s, Graph};
use serde_json::{json, Value};

/// `Files.ReadWrite` alone: the person's own files (Microsoft Graph
/// permissions reference, each method's least privileged delegated
/// permission). Not `Files.ReadWrite.All` or `Sites.*`: no other person's
/// files and no SharePoint site.
const API: Graph = Graph { name: "OneDrive", scopes: &["https://graph.microsoft.com/Files.ReadWrite"] };

const ME: &str = "/v1.0/me/";
/// The most of a file one read brings back.
const READ_MAX_BYTES: u64 = 4 * 1024 * 1024;
/// The most one upload sends (Graph's simple upload; larger files need an
/// upload session, on another host).
const UPLOAD_MAX_BYTES: usize = 4 * 1024 * 1024;
/// The most items one page asks for, when the call names none.
const PAGE_SIZE_DEFAULT: u64 = 50;

/// A file or folder as a person reads it.
fn item(i: &Value) -> Value {
    json!({
        "item_id": i["id"], "name": i["name"].as_str().unwrap_or_default(), "folder": i["folder"].is_object(),
        "size": i["size"], "type": i["file"]["mimeType"], "modified": i["lastModifiedDateTime"],
        "modified_by": i["lastModifiedBy"]["user"]["displayName"], "path": i["parentReference"]["path"], "link": i["webUrl"],
    })
}

fn items(path: String, a: &Value, t: &str) -> Result<Value, Failure> {
    let path = match a["page_token"].as_str().filter(|p| !p.is_empty()) {
        Some(p) => page(p, ME)?,
        None => path,
    };
    let r = API.call("GET", &path, t, None)?;
    Ok(json!({ "items": r["value"].as_array().into_iter().flatten().map(item).collect::<Vec<_>>(), "next_page_token": next_page(&r) }))
}

/// A folder path under the root (`Reports/2026`), each part encoded; one
/// that climbs out (`..`) is refused.
fn folder_path(p: &str) -> Result<String, String> {
    let parts: Vec<&str> = p.split('/').filter(|x| !x.is_empty()).collect();
    if parts.iter().any(|x| *x == "." || *x == "..") {
        return Err(format!("{p} is not a folder path under the root"));
    }
    Ok(parts.iter().map(|x| enc(x)).collect::<Vec<_>>().join("/"))
}

/// The folder a call is about: by id, by path, or the root.
fn folder(a: &Value) -> Result<String, String> {
    if let Some(i) = a["folder_id"].as_str().filter(|x| !x.is_empty()) {
        return Ok(format!("{ME}drive/items/{}", enc(i)));
    }
    let p = folder_path(&s(a, "folder"))?;
    Ok(if p.is_empty() { format!("{ME}drive/root") } else { format!("{ME}drive/root:/{p}:") })
}

fn top(a: &Value) -> Option<String> {
    opt_num(a, "max_results").or(Some(PAGE_SIZE_DEFAULT.to_string()))
}

fn list_files(a: &Value, t: &str) -> Result<Value, Failure> {
    items(format!("{}/children{}", folder(a)?, query(&[("$top", top(a))])), a, t)
}

fn search_files(a: &Value, t: &str) -> Result<Value, Failure> {
    // OData quotes a quote by doubling it.
    let q = s(a, "query").replace('\'', "''");
    items(format!("{ME}drive/root/search(q='{}'){}", enc(&q), query(&[("$top", top(a))])), a, t)
}

/// A file's bytes as text when they are UTF-8, and as base64 otherwise.
fn content(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => json!({ "text": text }),
        Err(_) => json!({ "content_base64": STANDARD.encode(bytes) }),
    }
}

fn read_file(a: &Value, t: &str) -> Result<Value, Failure> {
    let i = id(a, "item_id")?;
    let meta = API.call("GET", &format!("{ME}drive/items/{i}"), t, None)?;
    let mut out = json!({ "file": item(&meta) });
    if meta["folder"].is_object() {
        return Err("That is a folder: list_files lists what is in it.".to_string().into());
    }
    let size = meta["size"].as_u64().unwrap_or_default();
    if size > READ_MAX_BYTES {
        return Err(
            format!("That file is {size} bytes, more than the {READ_MAX_BYTES} one read brings back. Its link opens it: {}", meta["webUrl"].as_str().unwrap_or_default()).into()
        );
    }
    let r = API.bytes("GET", &format!("/beta/me/drive/items/{i}/contentStream"), t, None)?;
    if let (Some(o), Value::Object(c)) = (out.as_object_mut(), content(&r.body)) {
        o.extend(c);
    }
    Ok(out)
}

/// The bytes an upload sends: text, or base64.
fn upload_bytes(a: &Value) -> Result<Vec<u8>, String> {
    let bytes = match (a["text"].as_str(), a["content_base64"].as_str()) {
        (Some(t), None) => t.as_bytes().to_vec(),
        (None, Some(b)) => STANDARD.decode(b).map_err(|e| format!("content_base64 is not base64: {e}"))?,
        _ => return Err("give the file as text or as content_base64, one of them".into()),
    };
    if bytes.len() > UPLOAD_MAX_BYTES {
        return Err(format!("the file is {} bytes, more than the {UPLOAD_MAX_BYTES} one upload sends", bytes.len()));
    }
    Ok(bytes)
}

/// Where an upload goes: a new file, never one that is there already
/// (`conflictBehavior=fail`), so an upload can always be undone by
/// deleting what it made.
fn upload_path(a: &Value) -> Result<String, String> {
    let name = s(a, "name");
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err("name is the new file's name, with no folder in it".into());
    }
    let dir = folder_path(&s(a, "folder"))?;
    let at = if dir.is_empty() { enc(&name) } else { format!("{dir}/{}", enc(&name)) };
    Ok(format!("{ME}drive/root:/{at}:/content?@microsoft.graph.conflictBehavior=fail"))
}

fn upload_file(a: &Value, t: &str) -> Result<Value, Failure> {
    let bytes = upload_bytes(a)?;
    let kind = a["content_type"].as_str().filter(|k| !k.is_empty() && !k.contains(['\r', '\n'])).unwrap_or("application/octet-stream");
    let r = API.bytes("PUT", &upload_path(a)?, t, Some(Body { kind, bytes: &bytes }))?;
    Ok(item(&API.json(&r.body, true)?))
}

fn tools() -> Vec<Tool> {
    let st = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    let maybe = json!({ "type": ["string", "null"] });
    let it = json!({ "type": "object", "required": ["item_id", "name", "folder"] });
    let listed = obj(json!({ "items": { "type": "array", "items": it }, "next_page_token": maybe }), &["items"]);
    let tool = |name, description, input, output, run| Tool { name, description, input, output, run };
    vec![
        tool(
            "list_files",
            "List what is in a folder of the person's OneDrive: the root unless a folder (a path such as Reports/2026) or folder_id is given.",
            obj(json!({ "folder": st, "folder_id": st, "max_results": n, "page_token": st }), &[]),
            listed.clone(),
            list_files,
        ),
        tool(
            "search_files",
            "Find files in the person's OneDrive whose name or words match.",
            obj(json!({ "query": st, "max_results": n, "page_token": st }), &["query"]),
            listed,
            search_files,
        ),
        tool(
            "read_file",
            "Read a file (up to 4 MiB): as text when it is text, otherwise as base64, with what it is and where.",
            obj(json!({ "item_id": st }), &["item_id"]),
            obj(json!({ "file": it, "text": st, "content_base64": st }), &["file"]),
            read_file,
        ),
        tool(
            "upload_file",
            "Upload a new file (up to 4 MiB) to a folder of the person's OneDrive, the root unless named, as text or base64. It never replaces a file that is there.",
            obj(json!({ "name": st, "folder": st, "text": st, "content_base64": st, "content_type": st }), &["name"]),
            it,
            upload_file,
        ),
    ]
}

fn main() {
    serve("magnus-onedrive", tools())
}

#[cfg(test)]
#[path = "../../../tests/brokered-onedrive/onedrive.rs"]
mod tests;
