//! Each provider's subscription calls, with the person's own access token:
//! create, renew and delete. Only subscription metadata goes out and comes
//! back; nothing of the person's content does.
//!
//! - Microsoft Graph (Outlook mail, Outlook Calendar, OneDrive): one
//!   subscription per resource, created with `POST /subscriptions` (Graph
//!   calls the address with `validationToken` first, which `push::receive`
//!   echoes), renewed in place with `PATCH`, ended with `DELETE`.
//! - Google (Calendar events, Drive changes): a `web_hook` channel. A
//!   channel cannot be renewed: a new one is made and the old one stopped.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Graph's own address.
pub const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
/// Google's API address, for Calendar and Drive.
pub const GOOGLE_BASE: &str = "https://www.googleapis.com";
/// What Magnus asks for an Outlook mail or calendar subscription: under
/// Graph's most (10,080 minutes, under seven days).
pub const GRAPH_OUTLOOK_LIFE_MINUTES: i64 = 10_000;
/// What Magnus asks for a OneDrive subscription: under Graph's most
/// (42,300 minutes, under 30 days).
pub const GRAPH_DRIVE_LIFE_MINUTES: i64 = 42_000;
/// What Magnus asks for a Google channel: under a week, Drive's most for
/// changes. Google keeps the shorter of this and its own limit, and says
/// which in its answer.
pub const GOOGLE_CHANNEL_LIFE_SECS: i64 = 7 * 24 * 3600 - 3600;

/// Where the providers are: their real addresses, or a stand-in's in tests.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub graph: String,
    pub google: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints { graph: GRAPH_BASE.into(), google: GOOGLE_BASE.into() }
    }
}

/// How an app's provider takes a subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Graph { resource: &'static str, change_type: &'static str, life_minutes: i64 },
    GoogleCalendar,
    GoogleDrive,
}

/// The subscription kind for a catalog app, where Magnus subscribes by
/// itself. None for every other app: it is polled, or its address is given
/// to it by hand.
pub fn kind(app: &str) -> Option<Kind> {
    Some(match app {
        "microsoft-outlook" => Kind::Graph { resource: "me/mailFolders('inbox')/messages", change_type: "created", life_minutes: GRAPH_OUTLOOK_LIFE_MINUTES },
        "microsoft-calendar" => Kind::Graph { resource: "me/events", change_type: "created,updated,deleted", life_minutes: GRAPH_OUTLOOK_LIFE_MINUTES },
        // A drive's root takes only `updated`, and it covers the whole drive.
        "microsoft-onedrive" => Kind::Graph { resource: "me/drive/root", change_type: "updated", life_minutes: GRAPH_DRIVE_LIFE_MINUTES },
        "google-calendar" => Kind::GoogleCalendar,
        "google-drive" => Kind::GoogleDrive,
        _ => return None,
    })
}

/// A subscription at a provider: its id there (Graph's subscription id, or
/// Google's channel id with the resource id stopping it needs), when it
/// ends, and the public address it was made for. The push address itself
/// is a credential and is never kept.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sub {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub expires: DateTime<Utc>,
    pub base: String,
}

/// Make a subscription that sends the provider's notifications to `address`.
pub async fn create(http: &reqwest::Client, ep: &Endpoints, kind: &Kind, bearer: &str, address: &str, base: &str, now: DateTime<Utc>) -> Result<Sub> {
    match kind {
        Kind::Graph { resource, change_type, life_minutes } => {
            let body = json!({ "changeType": change_type, "notificationUrl": address, "resource": resource, "expirationDateTime": stamp(now + Duration::minutes(*life_minutes)) });
            let v = answer(http.post(format!("{}/subscriptions", ep.graph)).bearer_auth(bearer).json(&body).send().await?).await?;
            let id = v["id"].as_str().ok_or_else(|| anyhow!("Graph made a subscription with no id"))?;
            Ok(Sub { id: id.into(), resource: None, expires: graph_expiry(&v)?, base: base.into() })
        }
        Kind::GoogleCalendar => channel(http, bearer, format!("{}/calendar/v3/calendars/primary/events/watch", ep.google), address, base, now).await,
        Kind::GoogleDrive => {
            let start = answer(http.get(format!("{}/drive/v3/changes/startPageToken", ep.google)).bearer_auth(bearer).send().await?).await?;
            let token = start["startPageToken"].as_str().ok_or_else(|| anyhow!("Drive gave no start page token"))?;
            let url = format!("{}/drive/v3/changes/watch?pageToken={}", ep.google, url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>());
            channel(http, bearer, url, address, base, now).await
        }
    }
}

/// Keep a subscription going: Graph's is extended in place (made again if
/// Graph no longer has it); Google's is replaced by a new channel, and the
/// old one stopped.
pub async fn renew(http: &reqwest::Client, ep: &Endpoints, kind: &Kind, bearer: &str, sub: &Sub, address: &str, now: DateTime<Utc>) -> Result<Sub> {
    match kind {
        Kind::Graph { life_minutes, .. } => {
            let body = json!({ "expirationDateTime": stamp(now + Duration::minutes(*life_minutes)) });
            let r = http.patch(format!("{}/subscriptions/{}", ep.graph, sub.id)).bearer_auth(bearer).json(&body).send().await?;
            if r.status() == StatusCode::NOT_FOUND {
                return create(http, ep, kind, bearer, address, &sub.base, now).await;
            }
            let v = answer(r).await?;
            Ok(Sub { expires: graph_expiry(&v)?, ..sub.clone() })
        }
        Kind::GoogleCalendar | Kind::GoogleDrive => {
            let new = create(http, ep, kind, bearer, address, &sub.base, now).await?;
            if let Err(e) = delete(http, ep, kind, bearer, sub).await {
                tracing::warn!("the replaced channel {} was not stopped, and ends by itself at {}: {e:#}", sub.id, sub.expires);
            }
            Ok(new)
        }
    }
}

/// End a subscription. One the provider no longer has is ended already.
pub async fn delete(http: &reqwest::Client, ep: &Endpoints, kind: &Kind, bearer: &str, sub: &Sub) -> Result<()> {
    let r = match kind {
        Kind::Graph { .. } => http.delete(format!("{}/subscriptions/{}", ep.graph, sub.id)).bearer_auth(bearer).send().await?,
        Kind::GoogleCalendar | Kind::GoogleDrive => {
            let api = if *kind == Kind::GoogleCalendar { "calendar/v3" } else { "drive/v3" };
            let body = json!({ "id": sub.id, "resourceId": sub.resource });
            http.post(format!("{}/{api}/channels/stop", ep.google)).bearer_auth(bearer).json(&body).send().await?
        }
    };
    if matches!(r.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
        return Ok(());
    }
    answer(r).await.map(|_| ())
}

/// Open a Google `web_hook` channel at a watch endpoint.
async fn channel(http: &reqwest::Client, bearer: &str, url: String, address: &str, base: &str, now: DateTime<Utc>) -> Result<Sub> {
    let id = uuid::Uuid::new_v4().to_string();
    let until = (now + Duration::seconds(GOOGLE_CHANNEL_LIFE_SECS)).timestamp_millis();
    let body = json!({ "id": id, "type": "web_hook", "address": address, "expiration": until.to_string() });
    let v = answer(http.post(url).bearer_auth(bearer).json(&body).send().await?).await?;
    let resource = v["resourceId"].as_str().ok_or_else(|| anyhow!("Google opened a channel with no resource id"))?;
    // Milliseconds, as a string; a number is taken too.
    let ms = v["expiration"].as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v["expiration"].as_i64()).unwrap_or(until);
    let expires = DateTime::from_timestamp_millis(ms).ok_or_else(|| anyhow!("Google gave a channel end that is no time: {ms}"))?;
    Ok(Sub { id, resource: Some(resource.into()), expires, base: base.into() })
}

/// A provider's answer as JSON, or what refused it. An empty success is
/// an empty object.
async fn answer(r: reqwest::Response) -> Result<Value> {
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    if !status.is_success() {
        // The provider's own words on why, cut short: never a token, since
        // no token is in a request's answer.
        let why: String = text.chars().take(300).collect();
        bail!("HTTP {status}: {why}");
    }
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).context("the provider's answer is not JSON")
}

fn graph_expiry(v: &Value) -> Result<DateTime<Utc>> {
    let s = v["expirationDateTime"].as_str().ok_or_else(|| anyhow!("Graph gave a subscription no end"))?;
    Ok(DateTime::parse_from_rfc3339(s).with_context(|| format!("Graph's subscription end {s:?}"))?.with_timezone(&Utc))
}

fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each app Magnus subscribes for by itself has its provider's kind, at
    /// most the provider's longest lifetime; every other app has none.
    #[test]
    fn only_the_apps_whose_provider_takes_a_subscription_have_a_kind() {
        for app in ["microsoft-outlook", "microsoft-calendar", "microsoft-onedrive", "google-calendar", "google-drive"] {
            assert!(kind(app).is_some(), "{app} has no subscription kind");
        }
        for app in ["google-gmail", "google-chat", "slack", "github", "asana", "vercel", "mail"] {
            assert!(kind(app).is_none(), "{app} has a subscription kind it cannot have");
        }
        let Some(Kind::Graph { life_minutes, change_type, .. }) = kind("microsoft-onedrive") else { panic!("OneDrive is not Graph") };
        assert!(life_minutes < 42_300 && change_type == "updated", "OneDrive's root takes only `updated`, under 30 days");
        let Some(Kind::Graph { life_minutes, .. }) = kind("microsoft-outlook") else { panic!("Outlook is not Graph") };
        assert!(life_minutes < 10_080);
        let asked = (Utc::now() + Duration::seconds(GOOGLE_CHANNEL_LIFE_SECS)).signed_duration_since(Utc::now());
        assert!(asked < Duration::weeks(1), "a Google channel asks for more than Drive's week");
    }

    /// Every app given a kind is a catalog app that has watched reads, so a
    /// subscription always has something to poll.
    #[test]
    fn every_subscribed_app_is_a_watched_catalog_app() {
        for m in crate::catalog::manifests() {
            if kind(&m.id).is_some() {
                assert!(!crate::catalog::connector(m).watch.is_empty(), "{} is subscribed to but watches nothing", m.id);
            }
        }
        for app in ["microsoft-outlook", "microsoft-calendar", "microsoft-onedrive", "google-calendar", "google-drive"] {
            assert!(crate::catalog::manifests().iter().any(|m| m.id == app), "{app} is not in the catalog");
        }
    }
}
