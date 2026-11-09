//! Pages that run scripts, each served once from its own scheme under its
//! own content security policy. A `srcdoc` frame would take the app's.
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::http::{header, Response, StatusCode};
use tauri::State;

pub const SCHEME: &str = "magnus-frame";
const FALLBACK_POLICY: &str = "default-src 'none'";
const SANDBOX: &str = "sandbox allow-scripts";

#[derive(Default)]
pub struct Frames(Mutex<HashMap<String, (String, String)>>);

#[tauri::command]
pub fn sandbox_frame(frames: State<'_, Frames>, html: String, csp: Option<String>) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let policy = format!("{}; {SANDBOX}", csp.as_deref().unwrap_or(FALLBACK_POLICY));
    frames.0.lock().map_err(|e| e.to_string())?.insert(id.clone(), (policy, html));
    Ok(if cfg!(windows) { format!("http://{SCHEME}.localhost/{id}") } else { format!("{SCHEME}://localhost/{id}") })
}

/// The page, the first time it is asked for.
pub fn serve(frames: &Frames, path: &str) -> Response<Vec<u8>> {
    let found = frames.0.lock().ok().and_then(|mut f| f.remove(path.trim_start_matches('/')));
    let built = match found {
        Some((policy, html)) => Response::builder().header(header::CONTENT_TYPE, "text/html; charset=utf-8").header(header::CONTENT_SECURITY_POLICY, policy).body(html.into_bytes()),
        None => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
    };
    built.unwrap_or_else(|_| Response::new(Vec::new()))
}
