//! Push from connected apps (product spec, Proactive › Triggers):
//! notifications reach the server as events, by push where the service
//! supports it and by polling where it does not, and each event wakes the
//! agent.
//!
//! A push is a hint, never content. The app calls the person's push
//! address; the server checks the address, records that the app pushed,
//! and polls that app's watched reads at once, through the same run and
//! the same `note_watched` diff as a scheduled poll (#406). New items wake
//! a review exactly as a poll's would. Nothing the app sent reaches a
//! model, so a forged or hostile push can at most cause one extra read.
//!
//! The address carries an HMAC of the person and the app under a key held
//! in the vault, so it needs no table and is never stored.

use crate::api::{err, who};
use crate::config::Watch;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::push_subs::provider::Sub;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The least time between two polls one app's pushes start: a burst of
/// pushes polls once now and once after the gap, never once per push.
pub const PUSH_MIN_GAP_SECS: i64 = 30;
/// The most an app may send in one push. Its body is read only for a
/// handshake; everything else in it is dropped.
const PUSH_BODY_MAX_BYTES: usize = 64 * 1024;
/// The longest handshake token echoed back to an app.
const HANDSHAKE_MAX_CHARS: usize = 1024;
/// The vault secret that keys every push address. Made on first use.
const PUSH_KEY_SECRET: &str = "push_key";
/// The key's length, in bytes of randomness.
const PUSH_KEY_BYTES: usize = 32;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// An app pushed for a person. Recorded once per outstanding push: more
    /// pushes before the poll that answers it add nothing.
    Received { person: String, app: String, at: DateTime<Utc> },
    /// Magnus subscribed to the app's notifications for the person, at the
    /// provider (decisions/2026-11-10-push-subscriptions.md).
    Subscribed { person: String, app: String, sub: Sub, at: DateTime<Utc> },
    /// The subscription was kept going: extended, or replaced by a new one.
    Renewed { person: String, app: String, sub: Sub, at: DateTime<Utc> },
    /// The subscription ended: deleted at the provider, or given up when it
    /// could not be (it then ends there by itself at its end time).
    Unsubscribed { person: String, app: String, why: String, at: DateTime<Utc> },
    /// Subscribing or renewing failed; tried again after a wait.
    SubscribeFailed { person: String, app: String, why: String, at: DateTime<Utc> },
}

/// The latest push not yet known to be answered, by person and app, and
/// the subscriptions Magnus holds at providers.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    received: BTreeMap<(String, String), DateTime<Utc>>,
    subs: BTreeMap<(String, String), Sub>,
    failed: BTreeMap<(String, String), DateTime<Utc>>,
}

/// Their pushes go with them: whoever is next known by their name starts
/// with none.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        let mine = |p: &String| who.is(p);
        self.received.retain(|(p, _), _| !mine(p));
        self.subs.retain(|(p, _), _| !mine(p));
        self.failed.retain(|(p, _), _| !mine(p));
    }
}

impl Ledger {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::Received { person, app, at } => {
                self.received.insert((person.clone(), app.clone()), *at);
            }
            Change::Subscribed { person, app, sub, .. } | Change::Renewed { person, app, sub, .. } => {
                self.subs.insert((person.clone(), app.clone()), sub.clone());
                self.failed.remove(&(person.clone(), app.clone()));
            }
            Change::Unsubscribed { person, app, .. } => {
                self.subs.remove(&(person.clone(), app.clone()));
                self.failed.remove(&(person.clone(), app.clone()));
            }
            Change::SubscribeFailed { person, app, at, .. } => {
                self.failed.insert((person.clone(), app.clone()), *at);
            }
        }
    }

    pub fn get(&self, person: &str, app: &str) -> Option<DateTime<Utc>> {
        self.received.get(&(person.to_string(), app.to_string())).copied()
    }

    pub fn all(&self) -> Vec<(String, String, DateTime<Utc>)> {
        self.received.iter().map(|((p, a), t)| (p.clone(), a.clone(), *t)).collect()
    }

    pub fn sub(&self, person: &str, app: &str) -> Option<Sub> {
        self.subs.get(&(person.to_string(), app.to_string())).cloned()
    }

    pub fn subs(&self) -> Vec<(String, String, Sub)> {
        self.subs.iter().map(|((p, a), s)| (p.clone(), a.clone(), s.clone())).collect()
    }

    pub fn failed(&self, person: &str, app: &str) -> Option<DateTime<Utc>> {
        self.failed.get(&(person.to_string(), app.to_string())).copied()
    }
}

/// Whether a push at `pushed` still needs a poll of a watch last polled at
/// `last`: it does until a poll starts after it, and that poll starts only
/// once the gap since the last one has passed.
pub fn due(pushed: DateTime<Utc>, last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last.is_none_or(|l| l < pushed && now.signed_duration_since(l).num_seconds() >= PUSH_MIN_GAP_SECS)
}

/// Whether a new push is worth recording: not while an earlier one is
/// still waiting for its poll.
pub fn outstanding(pending: Option<DateTime<Utc>>, last: &[Option<DateTime<Utc>>]) -> bool {
    pending.is_some_and(|p| last.iter().any(|l| l.is_none_or(|l| l < p)))
}

/// The address's token: an HMAC of the person and the app.
pub fn token(key: &[u8], person: &str, app: &str) -> String {
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    hex::encode(ring::hmac::sign(&k, format!("magnus push\n{person}\n{app}").as_bytes()).as_ref())
}

/// Whether `given` is the address's token, compared in constant time.
pub fn verify(key: &[u8], person: &str, app: &str, given: &str) -> bool {
    let Ok(tag) = hex::decode(given) else { return false };
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::verify(&k, format!("magnus push\n{person}\n{app}").as_bytes(), &tag).is_ok()
}

/// What an app's subscription handshake wants echoed, if this is one:
/// Microsoft Graph's `validationToken` query and Slack's `url_verification`
/// challenge. Anything else is a push.
pub fn handshake(query: &HashMap<String, String>, body: &[u8]) -> Option<String> {
    let echo = match query.get("validationToken") {
        Some(t) => t.clone(),
        None => {
            let v: Value = serde_json::from_slice(body).ok()?;
            if v["type"] != "url_verification" {
                return None;
            }
            v["challenge"].as_str()?.to_string()
        }
    };
    (echo.chars().count() <= HANDSHAKE_MAX_CHARS).then_some(echo)
}

impl Kernel {
    /// The key every push address is signed with, made on first use.
    fn push_key(&self) -> anyhow::Result<Vec<u8>> {
        static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = MAKING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = self.vault.get(PUSH_KEY_SECRET)? {
            return Ok(hex::decode(s.expose())?);
        }
        let key: [u8; PUSH_KEY_BYTES] = rand::random();
        self.vault.put(PUSH_KEY_SECRET, &hex::encode(key))?;
        Ok(key.to_vec())
    }

    /// The app's watches the person can use now: the app is on, they are
    /// connected, their sign-in has not lapsed and they are not turned off.
    fn push_watches(&self, person: &str, app: &str) -> Vec<Watch> {
        if self.read(|s| s.turned_off.contains(person)) {
            return vec![];
        }
        self.connectors().into_iter().find(|c| c.name == app).filter(|c| self.connected(c, person) && !self.lapsed(c, person)).map(|c| c.watch).unwrap_or_default()
    }

    /// An app pushed for the person. False when the app has nothing to
    /// watch for them now, so the app can stop pushing.
    pub async fn pushed(self: &Arc<Self>, person: &str, app: &str, now: DateTime<Utc>) -> bool {
        let watches = self.push_watches(person, app);
        if watches.is_empty() {
            return false;
        }
        let waiting = self.read(|s| {
            let last: Vec<_> = watches.iter().map(|w| s.watch_last_polled.get(&(person.to_string(), app.to_string(), w.tool.clone())).copied()).collect();
            outstanding(s.push.get(person, app), &last)
        });
        if !waiting {
            let ev = Ev::Push(Change::Received { person: person.into(), app: app.into(), at: now });
            if let Err(e) = self.emit(&format!("app:{app}"), ev).await {
                tracing::warn!("push from {app} for {person} not recorded: {e:#}");
                return true;
            }
        }
        if self.cfg.proactive.on {
            self.poll_pushed(person, app, now).await;
        }
        true
    }

    /// Poll every watch of the app whose push is still due.
    async fn poll_pushed(self: &Arc<Self>, person: &str, app: &str, now: DateTime<Utc>) {
        let Some(pushed) = self.read(|s| s.push.get(person, app)) else { return };
        for w in self.push_watches(person, app) {
            let last = self.read(|s| s.watch_last_polled.get(&(person.to_string(), app.to_string(), w.tool.clone())).copied());
            if due(pushed, last, now) {
                self.start_poll(person, app, &w).await;
            }
        }
    }

    /// Pushes that arrived inside the gap: polled by the sweep once it
    /// passes, so the last push of a burst is never lost.
    pub(crate) async fn push_due(self: &Arc<Self>, now: DateTime<Utc>) {
        for (person, app, _) in self.read(|s| s.push.all()) {
            self.poll_pushed(&person, &app, now).await;
        }
    }

    /// The person's push address for an app, as a path on this server.
    pub(crate) fn push_path(&self, person: &str, app: &str) -> anyhow::Result<String> {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        Ok(format!("/v1/push/{}/{}/{}", enc(app), enc(person), token(&self.push_key()?, person, app)))
    }
}

type K = State<Arc<Kernel>>;

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/push/{app}/{person}/{token}", post(receive).layer(DefaultBodyLimit::max(PUSH_BODY_MAX_BYTES))).route("/v1/connectors/{name}/push", get(address))
}

/// An app's push. No session: the address is the credential. A wrong
/// address answers as if nothing were there.
async fn receive(State(k): K, Path((app, person, given)): Path<(String, String, String)>, Query(q): Query<HashMap<String, String>>, body: Bytes) -> Response {
    let key = match k.push_key() {
        Ok(key) => key,
        Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "the server cannot take pushes now"),
    };
    if !verify(&key, &person, &app, &given) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(echo) = handshake(&q, &body) {
        return (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], echo).into_response();
    }
    if k.pushed(&person, &app, Utc::now()).await {
        StatusCode::ACCEPTED.into_response()
    } else {
        // Gone: the app may drop the subscription.
        StatusCode::GONE.into_response()
    }
}

/// The person's push address for an app they use, to give the app as its
/// notification address. Only apps with something watched have one. `url`
/// is the whole address on the server's public address, and null when it
/// has none (a provider cannot reach a network address); `automatic` says
/// Magnus subscribes for the person by itself.
async fn address(State(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(c) = k.connector(&name).filter(|c| crate::boundary::may_use(&p, c)) else { return err(StatusCode::NOT_FOUND, "no such app") };
    if c.watch.is_empty() {
        return err(StatusCode::NOT_FOUND, "Magnus does not watch anything in this app");
    }
    match k.push_path(&p.id, &c.name) {
        Ok(path) => {
            let url = k.cfg.appliance.public_url.as_ref().map(|u| u.join(&path));
            let watches: Vec<String> = c.watch.iter().map(|w| w.tool.clone()).collect();
            Json(json!({ "path": path, "url": url, "automatic": k.push_automatic(&c), "watches": watches })).into_response()
        }
        Err(_) => err(StatusCode::SERVICE_UNAVAILABLE, "the server cannot make push addresses now"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + s, 0).unwrap()
    }

    /// The address is bound to its person and app: another person's, another
    /// app's, an altered or a garbled token is refused.
    #[test]
    fn the_address_is_bound_to_its_person_and_app() {
        let key = [7u8; PUSH_KEY_BYTES];
        let good = token(&key, "sam", "mail");
        assert!(verify(&key, "sam", "mail", &good));
        assert!(!verify(&key, "ada", "mail", &good), "another person's push passed");
        assert!(!verify(&key, "sam", "calendar", &good), "another app's push passed");
        assert!(!verify(&[8u8; PUSH_KEY_BYTES], "sam", "mail", &good), "a token from another key passed");
        let mut bad = good.clone();
        bad.replace_range(0..1, if good.starts_with('0') { "1" } else { "0" });
        assert!(!verify(&key, "sam", "mail", &bad), "an altered token passed");
        assert!(!verify(&key, "sam", "mail", "zz"), "a garbled token passed");
        assert!(!verify(&key, "sam", "mail", ""), "an empty token passed");
    }

    /// A push polls at once unless a poll started within the gap; one that
    /// lands inside the gap polls once the gap passes, and a poll after it
    /// answers it.
    #[test]
    fn a_push_polls_once_per_gap_and_is_answered_by_a_later_poll() {
        let gap = PUSH_MIN_GAP_SECS;
        assert!(due(t(0), None, t(0)), "a never-polled watch did not poll on push");
        assert!(due(t(100), Some(t(0)), t(100)), "a push long after the last poll did not poll");
        assert!(!due(t(100), Some(t(100 - gap + 1)), t(100)), "a push inside the gap polled at once");
        assert!(due(t(100), Some(t(100 - gap + 1)), t(101)), "a push inside the gap never polled");
        assert!(!due(t(100), Some(t(100)), t(100 + gap * 10)), "a poll at the push did not answer it");
        assert!(!due(t(100), Some(t(150)), t(500)), "a later poll did not answer the push");
    }

    /// More pushes before the poll add nothing to the log.
    #[test]
    fn pushes_waiting_for_their_poll_are_not_recorded_again() {
        assert!(!outstanding(None, &[Some(t(0))]), "the first push was not recorded");
        assert!(outstanding(Some(t(10)), &[Some(t(0))]), "a second push before the poll was recorded");
        assert!(outstanding(Some(t(10)), &[None]), "a push before any poll was recorded twice");
        assert!(!outstanding(Some(t(10)), &[Some(t(10)), Some(t(12))]), "a push after the answering poll was not recorded");
        assert!(outstanding(Some(t(10)), &[Some(t(12)), Some(t(5))]), "a push one watch has not answered was recorded again");
    }

    /// Graph's and Slack's handshakes are echoed; anything else is a push.
    #[test]
    fn subscription_handshakes_are_echoed() {
        let q = |k: &str, v: &str| HashMap::from([(k.to_string(), v.to_string())]);
        assert_eq!(handshake(&q("validationToken", "abc"), b"").as_deref(), Some("abc"));
        assert_eq!(handshake(&HashMap::new(), br#"{"type":"url_verification","challenge":"xyz"}"#).as_deref(), Some("xyz"));
        assert_eq!(handshake(&HashMap::new(), br#"{"type":"event_callback","challenge":"xyz"}"#), None);
        assert_eq!(handshake(&HashMap::new(), b"not json"), None);
        assert_eq!(handshake(&q("validationToken", &"x".repeat(HANDSHAKE_MAX_CHARS + 1)), b""), None, "an oversized token was echoed");
    }

    /// The event's shape is fixed once written: kind `push`, change
    /// `received`, and it reads back into the state the sweep polls from.
    #[test]
    fn a_push_is_logged_as_its_own_kind_and_read_back() {
        let ev = Ev::Push(Change::Received { person: "sam".into(), app: "mail".into(), at: t(3) });
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!((v["kind"].as_str(), v["change"].as_str(), v["app"].as_str()), (Some("push"), Some("received"), Some("mail")));
        let back: Ev = serde_json::from_value(v).unwrap();
        let mut s = crate::state::State::default();
        let Ev::Push(c) = back else { panic!("read back as another kind") };
        s.push.apply(&c);
        assert_eq!(s.push.get("sam", "mail"), Some(t(3)));
    }

    /// The ledger keeps the latest push and forgets an erased person.
    #[test]
    fn the_ledger_keeps_the_latest_push_and_forgets_the_erased() {
        let mut l = Ledger::default();
        l.apply(&Change::Received { person: "sam".into(), app: "mail".into(), at: t(1) });
        l.apply(&Change::Received { person: "sam".into(), app: "mail".into(), at: t(5) });
        l.apply(&Change::Received { person: "ada".into(), app: "mail".into(), at: t(2) });
        assert_eq!(l.get("sam", "mail"), Some(t(5)));
        crate::forget::Forget::forget(&mut l, &crate::forget::Erased { person: "sam".into(), ..Default::default() });
        assert_eq!(l.get("sam", "mail"), None);
        assert_eq!(l.all().len(), 1);
    }
}
