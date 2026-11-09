//! A takeover's screen. The webview cannot trust the appliance's pinned
//! certificate, so the app carries the display's bytes itself.
use crate::{App, Res};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;

const DISPLAY_PROTOCOL: &str = "magnus.display";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Default)]
pub struct Displays(Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>);

#[tauri::command]
pub async fn display_open(events: AppHandle, app: State<'_, App>, displays: State<'_, Displays>, handle: String, computer: String, ticket: String, prefix: String) -> Res<()> {
    if computer.is_empty() || !computer.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("not a computer id".into());
    }
    let (url, tls) = app.display_tls()?;
    let target = format!("{}/v1/computers/{computer}/display", url.replacen("http", "ws", 1));
    let mut request = target.into_client_request().map_err(|e| e.to_string())?;
    let offered = format!("{prefix}{ticket}, {DISPLAY_PROTOCOL}");
    request.headers_mut().insert("sec-websocket-protocol", HeaderValue::from_str(&offered).map_err(|e| e.to_string())?);
    let connector = tls.map(|t| Connector::Rustls(Arc::new(t)));
    let (socket, _) = tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector).await.map_err(|e| format!("The screen could not be opened: {e}"))?;
    let (to_screen, mut from_person) = mpsc::unbounded_channel::<Vec<u8>>();
    displays.0.lock().map_err(|e| e.to_string())?.insert(handle.clone(), to_screen);
    tokio::spawn(async move {
        let (mut sink, mut stream) = socket.split();
        let reason = loop {
            tokio::select! {
                from_screen = stream.next() => match from_screen {
                    Some(Ok(Message::Binary(b))) => { let _ = events.emit("display-bytes", json!({ "handle": handle, "b64": B64.encode(&b) })); }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break Some(e.to_string()),
                    None => break None,
                },
                to_send = from_person.recv() => match to_send {
                    Some(bytes) => { if let Err(e) = sink.send(Message::Binary(bytes.into())).await { break Some(e.to_string()); } }
                    None => { let _ = sink.close().await; return; }
                },
            }
        };
        let _ = events.emit("display-end", json!({ "handle": handle, "reason": reason }));
    });
    Ok(())
}

#[tauri::command]
pub fn display_send(displays: State<'_, Displays>, handle: String, b64: String) -> Res<()> {
    let bytes = B64.decode(b64).map_err(|e| e.to_string())?;
    if let Some(d) = displays.0.lock().map_err(|e| e.to_string())?.get(&handle) {
        let _ = d.send(bytes);
    }
    Ok(())
}

#[tauri::command]
pub fn display_close(displays: State<'_, Displays>, handle: String) {
    if let Ok(mut d) = displays.0.lock() {
        d.remove(&handle);
    }
}
