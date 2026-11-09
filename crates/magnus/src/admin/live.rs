//! Live data in the console (admin console spec, Shell: "Live data (status,
//! tasks, activity) streams over server-sent events"), at `/api/v1/events`.
//! Each event is a JSON object named by what changed:
//!
//! - `status`: the appliance's health strip, the cards and the kill switch,
//!   as Overview and the top bar show them.
//!
//! A stream lasts no longer than the provider check (`refresh_seconds`), and
//! ends as soon as its session does: the browser reconnects through the
//! guard, which asks the provider again, so an admin turned off there loses
//! the stream as they lose every other route.

use super::session::Admin;
use super::Console;
use axum::extract::State;
use axum::http::Method;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Extension;
use chrono::Utc;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

/// How often the console looks for a change.
const TICK: Duration = Duration::from_secs(2);

/// What the top bar and Overview show live.
pub async fn status(c: &Console) -> Value {
    let (k, switch) = tokio::join!(c.kernel.call(Method::GET, "/v1/overview", None), c.kernel.call(Method::GET, "/v1/kill-switch", None));
    let up = k.is_ok();
    let k = k.unwrap_or(Value::Null);
    json!({
        "kernel_up": up,
        "log_up": k["log_up"] == true,
        "version": k["version"],
        "running_tasks": k["running_tasks"],
        "waiting_tasks": k["waiting_tasks"],
        "signed_in": k["signed_in"],
        "microvms_running": k["microvms_running"],
        "knowledge_changes_24h": k["knowledge_changes_24h"],
        "knowledge_last_change": k["knowledge_last_change"],
        "kill_switch": switch.unwrap_or(Value::Null),
    })
}

struct Tick {
    c: Arc<Console>,
    sid: String,
    until: chrono::DateTime<Utc>,
    last: Option<Value>,
    first: bool,
}

pub async fn events(State(c): State<Arc<Console>>, Extension(a): Extension<Admin>) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let until = Utc::now() + chrono::Duration::seconds(c.refresh_seconds.max(30));
    let t = Tick { c, sid: a.sid, until, last: None, first: true };
    let s = futures::stream::unfold(t, |mut t| async move {
        loop {
            if !t.first {
                tokio::time::sleep(TICK).await;
            }
            t.first = false;
            if Utc::now() > t.until || !super::session::open(&t.c, &t.sid) {
                return None;
            }
            let now = status(&t.c).await;
            if t.last.as_ref() != Some(&now) {
                t.last = Some(now.clone());
                let ev = Event::default().event("status").data(now.to_string());
                return Some((Ok(ev), t));
            }
        }
    });
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
