//! Push subscriptions (decisions/2026-11-10-push-subscriptions.md): where a
//! provider lets a person subscribe an https address to their own changes,
//! Magnus does it for them with their own connection, so the app's pushes
//! reach `push::receive` without anyone copying an address.
//!
//! Only with a public address (`[appliance] public_url`): without one no
//! provider could reach the server, and nothing is attempted. A sweep keeps
//! each subscription to what should be: made for a connected app (on
//! connect, and on start for apps already connected), renewed a day before
//! it ends, made again when the public address changes, and deleted on
//! disconnect. Each step is an event. A push is still only a hint that
//! polls: nothing a provider sends is read.

pub mod provider;

use crate::config::Connector;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::push::Change;
use chrono::{DateTime, Utc};
use provider::{Endpoints, Kind, Sub};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How often the sweep looks at subscriptions.
pub const PUSH_SUBS_EVERY_SECS: i64 = 60;
/// A subscription is renewed once it has less than this left.
pub const RENEW_BEFORE_HOURS: i64 = 24;
/// After a failed subscribe or renewal, how long before trying again.
pub const RETRY_AFTER_MINUTES: i64 = 60;
/// How long a provider may take to answer a subscription call. Graph calls
/// the address back before it answers, so this is generous.
const CALL_TIMEOUT_SECS: u64 = 30;
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// What to do with one person's subscription to one app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Keep,
    Create,
    Renew,
    /// Delete it and make a new one: it was made for another public
    /// address, or it has already ended.
    Replace,
    Remove,
}

/// The step that brings a subscription to what it should be. `want`: the
/// person uses the app now. `base`: the server's public address, if any.
/// `failed`: the last failure, which holds back a new try for a while.
pub fn step(want: bool, have: Option<&Sub>, failed: Option<DateTime<Utc>>, base: Option<&str>, now: DateTime<Utc>) -> Step {
    let Some(base) = base.filter(|_| want) else { return if have.is_some() { Step::Remove } else { Step::Keep } };
    if failed.is_some_and(|f| now.signed_duration_since(f).num_minutes() < RETRY_AFTER_MINUTES) {
        return Step::Keep;
    }
    match have {
        None => Step::Create,
        Some(s) if s.base != base || s.expires <= now => Step::Replace,
        Some(s) if s.expires.signed_duration_since(now).num_hours() < RENEW_BEFORE_HOURS => Step::Renew,
        Some(_) => Step::Keep,
    }
}

/// The subscription kind of an app Magnus subscribes to by itself: a
/// catalog app as the catalog has it (an app of the same name pointing
/// elsewhere holds some other token), with something watched.
pub fn kind_of(c: &Connector) -> Option<Kind> {
    let from_catalog = crate::catalog::manifests().iter().any(|m| m.id == c.name && m.endpoint == c.url);
    provider::kind(&c.name).filter(|_| from_catalog && !c.watch.is_empty())
}

fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS)).timeout(std::time::Duration::from_secs(CALL_TIMEOUT_SECS)).build()?)
}

impl Kernel {
    /// Whether Magnus subscribes to the app's notifications by itself here.
    pub fn push_automatic(&self, c: &Connector) -> bool {
        self.cfg.appliance.public_url.is_some() && kind_of(c).is_some()
    }

    /// How an app's instant updates come, for its page: `automatic`
    /// (Magnus subscribes), `address` (the person gives the app its push
    /// address), `needs_public_url` (the server has no public address), or
    /// `none` (nothing is watched).
    pub fn push_instant(&self, c: &Connector) -> &'static str {
        match (c.watch.is_empty(), self.cfg.appliance.public_url.is_some(), self.push_automatic(c)) {
            (true, _, _) => "none",
            (false, false, _) => "needs_public_url",
            (false, true, true) => "automatic",
            (false, true, false) => "address",
        }
    }

    /// Bring subscriptions to what they should be, at most once a minute
    /// and one sweep at a time, without holding up the caller.
    pub(crate) fn push_subs_due(self: &Arc<Self>, now: DateTime<Utc>) {
        static LAST: Mutex<Option<DateTime<Utc>>> = Mutex::new(None);
        static RUNNING: AtomicBool = AtomicBool::new(false);
        if self.cfg.appliance.public_url.is_none() && self.read(|s| s.push.subs().is_empty()) {
            return;
        }
        {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| now.signed_duration_since(t).num_seconds() < PUSH_SUBS_EVERY_SECS) {
                return;
            }
            *last = Some(now);
        }
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let k = self.clone();
        tokio::spawn(async move {
            k.reconcile_push_subs(&Endpoints::default(), now).await;
            RUNNING.store(false, Ordering::SeqCst);
        });
    }

    /// Everyone's subscriptions, one step each.
    pub async fn reconcile_push_subs(&self, ep: &Endpoints, now: DateTime<Utc>) {
        let base = self.cfg.appliance.public_url.as_ref().map(|u| u.as_str().to_string());
        let apps: Vec<(Connector, Kind)> = self.connectors().into_iter().filter_map(|c| kind_of(&c).map(|k| (c, k))).collect();
        let mut people: std::collections::BTreeSet<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        people.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        for (c, kind) in &apps {
            for person in &people {
                let want = !self.read(|s| s.turned_off.contains(person)) && self.connected(c, person) && !self.lapsed(c, person);
                let (have, failed) = self.read(|s| (s.push.sub(person, &c.name), s.push.failed(person, &c.name)));
                let st = step(want, have.as_ref(), failed, base.as_deref(), now);
                if st != Step::Keep {
                    self.push_sub_step(ep, c, kind, person, st, have, base.as_deref().unwrap_or_default(), now).await;
                }
            }
        }
        // Subscriptions for apps no longer subscribed to (turned off,
        // removed): ended, with the token where there still is one.
        for (person, app, sub) in self.read(|s| s.push.subs()) {
            if apps.iter().all(|(c, _)| c.name != app) {
                let gone = Change::Unsubscribed {
                    person: person.clone(),
                    app: app.clone(),
                    why: format!("the app is no longer subscribed to here; it ends at the provider by {}", sub.expires),
                    at: now,
                };
                self.record_push_sub(&app, gone).await;
            }
        }
    }

    /// Take one step for one person and app, and record what happened.
    #[allow(clippy::too_many_arguments)]
    async fn push_sub_step(&self, ep: &Endpoints, c: &Connector, kind: &Kind, person: &str, st: Step, have: Option<Sub>, base: &str, now: DateTime<Utc>) {
        let app = &c.name;
        let bearer = match self.connector_secret(c, person).await {
            Ok(Some(s)) => Some(s),
            Ok(None) | Err(_) => None,
        };
        let address = self.push_path(person, app).map(|p| format!("{base}{p}"));
        let ev = match (st, bearer, address, have) {
            (Step::Remove, bearer, _, Some(sub)) => {
                let why = match bearer {
                    Some(b) => match provider::delete(&http_or_default(), ep, kind, b.expose(), &sub).await {
                        Ok(()) => "deleted at the provider".to_string(),
                        Err(e) => format!("the provider did not delete it ({e:#}); it ends there by {}", sub.expires),
                    },
                    None => format!("no sign-in to delete it with; it ends at the provider by {}", sub.expires),
                };
                Change::Unsubscribed { person: person.into(), app: app.clone(), why, at: now }
            }
            (Step::Remove, ..) | (Step::Keep, ..) => return,
            (_, None, _, _) => Change::SubscribeFailed { person: person.into(), app: app.clone(), why: "no current sign-in to subscribe with".into(), at: now },
            (_, _, Err(e), _) => Change::SubscribeFailed { person: person.into(), app: app.clone(), why: format!("no push address: {e:#}"), at: now },
            (st, Some(b), Ok(address), have) => {
                let h = http_or_default();
                let made = match (st, have) {
                    (Step::Renew, Some(sub)) => provider::renew(&h, ep, kind, b.expose(), &sub, &address, now).await.map(|s| (s, true)),
                    (Step::Replace, Some(old)) => {
                        if let Err(e) = provider::delete(&h, ep, kind, b.expose(), &old).await {
                            tracing::warn!("{app} subscription for {person} not deleted before it was replaced: {e:#}");
                        }
                        provider::create(&h, ep, kind, b.expose(), &address, base, now).await.map(|s| (s, false))
                    }
                    _ => provider::create(&h, ep, kind, b.expose(), &address, base, now).await.map(|s| (s, false)),
                };
                match made {
                    Ok((sub, true)) => Change::Renewed { person: person.into(), app: app.clone(), sub, at: now },
                    Ok((sub, false)) => Change::Subscribed { person: person.into(), app: app.clone(), sub, at: now },
                    Err(e) => Change::SubscribeFailed { person: person.into(), app: app.clone(), why: format!("{e:#}"), at: now },
                }
            }
        };
        self.record_push_sub(app, ev).await;
    }

    async fn record_push_sub(&self, app: &str, c: Change) {
        if let Change::SubscribeFailed { person, why, .. } = &c {
            tracing::warn!("push subscription to {app} for {person} failed: {why}");
        }
        if let Err(e) = self.emit("kernel", Ev::Push(c)).await {
            tracing::warn!("push subscription change for {app} not recorded: {e:#}");
        }
    }

    /// End the person's subscription to an app as they disconnect it,
    /// while their sign-in still works to do it with.
    pub(crate) async fn unsubscribe_push(&self, app: &str, person: &str) {
        let Some(c) = self.connector(app) else { return };
        let (Some(kind), Some(_)) = (kind_of(&c), self.read(|s| s.push.sub(person, app))) else { return };
        let have = self.read(|s| s.push.sub(person, app));
        self.push_sub_step(&Endpoints::default(), &c, &kind, person, Step::Remove, have, "", chrono::Utc::now()).await;
    }
}

/// The client for subscription calls; the default one if it cannot be
/// built with its timeouts.
fn http_or_default() -> reqwest::Client {
    http().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + h * 3600, 0).unwrap()
    }

    fn sub(base: &str, expires: DateTime<Utc>) -> Sub {
        Sub { id: "s1".into(), resource: None, expires, base: base.into() }
    }

    const B: &str = "https://teams.example.com";

    /// A connected app with a public address is subscribed to; renewed a
    /// day before its end; made again on a new address or once ended.
    #[test]
    fn a_subscription_is_made_renewed_and_replaced_as_it_should_be() {
        assert_eq!(step(true, None, None, Some(B), t(0)), Step::Create);
        assert_eq!(step(true, Some(&sub(B, t(100))), None, Some(B), t(0)), Step::Keep);
        assert_eq!(step(true, Some(&sub(B, t(23))), None, Some(B), t(0)), Step::Renew, "a day before its end it was not renewed");
        assert_eq!(step(true, Some(&sub(B, t(-1))), None, Some(B), t(0)), Step::Replace, "an ended subscription was not made again");
        assert_eq!(step(true, Some(&sub("https://old.example.com", t(100))), None, Some(B), t(0)), Step::Replace, "a subscription for the old address was kept");
    }

    /// Without a public address, or once the person no longer uses the
    /// app, nothing is made and what there is is removed.
    #[test]
    fn without_an_address_or_a_connection_nothing_is_subscribed() {
        assert_eq!(step(true, None, None, None, t(0)), Step::Keep, "a subscription was attempted with no public address");
        assert_eq!(step(false, None, None, Some(B), t(0)), Step::Keep);
        assert_eq!(step(false, Some(&sub(B, t(100))), None, Some(B), t(0)), Step::Remove, "a disconnected app kept its subscription");
        assert_eq!(step(true, Some(&sub(B, t(100))), None, None, t(0)), Step::Remove, "a subscription outlived the public address");
    }

    /// A failure holds back the next try for a while, then it is tried
    /// again; removal is never held back.
    #[test]
    fn a_failure_waits_before_trying_again() {
        let failed = Some(t(0));
        assert_eq!(step(true, None, failed, Some(B), t(0)), Step::Keep, "a failed subscribe was tried again at once");
        let later = t(0) + chrono::Duration::minutes(RETRY_AFTER_MINUTES);
        assert_eq!(step(true, None, failed, Some(B), later), Step::Create, "a failed subscribe was never tried again");
        assert_eq!(step(false, Some(&sub(B, t(100))), failed, Some(B), t(0)), Step::Remove);
    }

    /// The subscription events keep the ledger: made, renewed, failed and
    /// ended, read back from their logged form.
    #[test]
    fn subscription_events_keep_the_ledger() {
        let mut l = crate::push::Ledger::default();
        let s = sub(B, t(100));
        for c in [
            Change::Subscribed { person: "sam".into(), app: "microsoft-outlook".into(), sub: s.clone(), at: t(0) },
            Change::SubscribeFailed { person: "sam".into(), app: "microsoft-outlook".into(), why: "x".into(), at: t(1) },
        ] {
            let v = serde_json::to_value(Ev::Push(c)).unwrap();
            assert_eq!(v["kind"], "push");
            let Ev::Push(back) = serde_json::from_value(v).unwrap() else { panic!("read back as another kind") };
            l.apply(&back);
        }
        assert_eq!(l.sub("sam", "microsoft-outlook"), Some(s.clone()));
        assert_eq!(l.failed("sam", "microsoft-outlook"), Some(t(1)));
        l.apply(&Change::Renewed { person: "sam".into(), app: "microsoft-outlook".into(), sub: sub(B, t(200)), at: t(2) });
        assert_eq!((l.sub("sam", "microsoft-outlook").map(|s| s.expires), l.failed("sam", "microsoft-outlook")), (Some(t(200)), None));
        l.apply(&Change::Unsubscribed { person: "sam".into(), app: "microsoft-outlook".into(), why: "deleted".into(), at: t(3) });
        assert_eq!(l.sub("sam", "microsoft-outlook"), None);
        l.apply(&Change::Subscribed { person: "sam".into(), app: "google-drive".into(), sub: s, at: t(4) });
        crate::forget::Forget::forget(&mut l, &crate::forget::Erased { person: "sam".into(), ..Default::default() });
        assert!(l.subs().is_empty(), "an erased person's subscriptions were kept");
    }
}
