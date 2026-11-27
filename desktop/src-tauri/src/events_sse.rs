//! Relay `/v1/events` SSE invalidation frames to the desktop app's WebView.

use crate::{App, Res};
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tokio::task::AbortHandle;

static EVENTS_JOB: Mutex<Option<AbortHandle>> = Mutex::new(None);

pub(crate) fn stop() {
    if let Some(j) = EVENTS_JOB.lock().unwrap_or_else(|e| e.into_inner()).take() {
        j.abort();
    }
}

#[tauri::command]
pub(crate) async fn events_start(handle: AppHandle, app: State<'_, App>) -> Res<()> {
    let (url, c, token) = app.session()?;
    stop();
    let page = handle.clone();
    let job = tokio::spawn(async move {
        let Ok(r) = c.get(format!("{url}/v1/events")).bearer_auth(&token).send().await else {
            let _ = page.emit("events-state", false);
            return;
        };
        if !r.status().is_success() {
            let _ = page.emit("events-state", false);
            return;
        }
        let _ = page.emit("events-state", true);
        let mut stream = r.bytes_stream();
        let mut buf = String::new();
        while let Some(Ok(chunk)) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buf.find("\n\n") {
                let event: String = buf.drain(..end + 2).collect();
                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            let _ = page.emit("events", v);
                        }
                    }
                }
            }
        }
        let _ = page.emit("events-state", false);
    });
    *EVENTS_JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(job.abort_handle());
    Ok(())
}

#[tauri::command]
pub(crate) fn events_stop() {
    stop();
}
