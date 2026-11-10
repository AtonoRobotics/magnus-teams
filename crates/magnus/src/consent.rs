//! Connecting an app grants its use (spec rev 140, "Permissions and
//! tasks"). Connecting shows one screen: what Magnus can read in the app,
//! what it can change and send, and the risks, in plain words. Accepting it
//! lets Magnus use every tool the app offers within those terms, with no
//! review of individual tools. A catalog connector's terms come from its
//! signed manifest, and its updates ship with a Magnus release and are
//! accepted with it. A third-party server's come from the tools it lists,
//! classed by its own hints; one that later adds a tool able to change or
//! send something asks once, for the whole app, on one card the client
//! draws: keep it, or disconnect.

use crate::api::{err, internal, who};
use crate::config::Connector;
use crate::connectors::{suggested_risk, title, Refusal};
use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use crate::state::Item;
use crate::tool_gate::Change;
use axum::{
    extract::{Path, State as AxState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The card a third-party server's new tools that change or send raise.
pub const APP_TOOLS: &str = "app_tools";
/// The retired reviews of tool descriptions: put away wherever they are read.
pub const RETIRED_KINDS: [&str; 2] = ["tool_review", "tool_change"];
pub const RETIRED_TEXT: &str = "Tool descriptions are screened now, and connecting an app accepts its tools: nothing waits on you here.";

/// A tool's name as people read it: "list_events" is "list events".
fn plain(tool: &str) -> String {
    tool.replace(['_', '-'], " ")
}

fn sentence(head: &str, tools: &[String]) -> String {
    format!("{head}: {}.", tools.join(", "))
}

/// The connect screen for an app whose tools are classed as `classes`
/// (tool to risk). `listed`: whether its tools could be listed at all.
pub fn terms(c: &Connector, classes: &BTreeMap<String, String>, listed: bool) -> Value {
    let name = c.title.clone().unwrap_or_else(|| title(&c.name));
    let of = |risk: &str| classes.iter().filter(|(_, r)| r.as_str() == risk).map(|(t, _)| plain(t)).collect::<Vec<String>>();
    let (reads, undoable, lasting, money) = (of("read"), of("reversible_write"), of("irreversible"), of("financial"));
    let mut can_read = vec![];
    if !reads.is_empty() {
        can_read.push(sentence("It can", &reads));
    }
    let mut can_change = vec![];
    if !undoable.is_empty() {
        can_change.push(sentence("Changes that can be undone", &undoable));
    }
    if !lasting.is_empty() {
        can_change.push(sentence("Changes and sends that cannot be undone", &lasting));
    }
    if !money.is_empty() {
        can_change.push(sentence("Moves money", &money));
    }
    let mut risks = vec![];
    if c.label.contains("untrusted") {
        risks.push(format!("What Magnus reads in {name} can be written by other people, and could try to steer it. Magnus treats it as information, never as instructions."));
    }
    match c.audience.as_str() {
        "public" => risks.push(format!("What Magnus sends through {name} can reach anyone, outside your organisation too.")),
        "org" | "internal" => risks.push(format!("What Magnus sends through {name} can reach people in your organisation.")),
        _ => {}
    }
    if !lasting.is_empty() {
        risks.push(format!("Some of what it does in {name} cannot be undone: {}.", lasting.join(", ")));
    }
    if !money.is_empty() {
        risks.push("It can move money. Magnus asks you before every payment.".into());
    }
    if !listed {
        risks.push(format!("{name} shows what it offers once you sign in. Magnus can use all of it: what reads, and what changes or sends."));
    }
    if c.catalog {
        risks.push(format!("Alpha Vector reviewed {name}. Its updates come with Magnus updates."));
    } else {
        risks.push(format!("Alpha Vector has not reviewed {name}. If it adds something that can change or send, Magnus asks you once whether to keep it."));
    }
    let cannot = crate::catalog::manifest(&c.name).filter(|_| c.catalog).map(|m| m.cannot.clone());
    let shown = json!({ "app": c.name, "reads": can_read, "changes": can_change, "risks": risks, "cannot": cannot });
    let hash = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(crate::boundary::canonical(&shown).as_bytes()))
    };
    json!({ "app": c.name, "title": name, "reads": can_read, "changes": can_change, "risks": risks, "cannot": cannot, "terms": hash })
}

/// The events that keep a card's tools: each classed as the card said.
pub fn kept(item: &Item) -> Vec<Ev> {
    let app = item.details["app"].as_str().unwrap_or_default();
    item.details["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some(Ev::ToolClassified { connector: app.into(), tool: t["tool"].as_str()?.into(), risk: Some(t["risk"].as_str()?.into()) }))
        .collect()
}

/// A server the organisation added by its address: not the catalog's, not
/// the config file's.
fn third_party(k: &Kernel, c: &Connector) -> bool {
    !c.catalog && k.cfg.connectors.iter().all(|f| f.name != c.name) && k.read(|s| s.connectors.contains_key(&c.name))
}

/// A third-party server that takes no sign-in, which no one has connected
/// through its screen yet: its tools are not classed, so it is not in use.
pub(crate) fn awaits_connect(k: &Kernel, c: &Connector) -> bool {
    c.tools.is_empty() && third_party(k, c)
}

impl Kernel {
    /// The tools of an app as classed now, and whether they could be
    /// listed: a third-party server not yet classed is listed through
    /// `person`'s sign-in and classed by its own hints.
    async fn classes(&self, c: &Connector, person: &str) -> (BTreeMap<String, String>, bool) {
        if !c.tools.is_empty() || !third_party(self, c) {
            return (c.tools.iter().map(|(t, x)| (t.clone(), x.risk.clone())).collect(), true);
        }
        let secret = self.connector_secret(c, person).await.ok().flatten();
        if c.credential.is_some() && secret.is_none() {
            return (BTreeMap::new(), false);
        }
        match self.app_tools(c, secret.as_ref().map(|s| s.expose())).await {
            Ok(ts) => (ts.iter().map(|t| (t.name.clone(), suggested_risk(&t.annotations).to_string())).collect(), true),
            Err(e) => {
                tracing::warn!("list {}'s tools for its connect screen: {e:#}", c.name);
                (BTreeMap::new(), false)
            }
        }
    }

    /// The connect screen for `person`.
    pub async fn connect_terms(&self, c: &Connector, person: &str) -> Value {
        let (classes, listed) = self.classes(c, person).await;
        terms(c, &classes, listed)
    }

    /// `person` accepted the connect screen whose terms hash is `seen`. A
    /// third-party server's tools are classed as the screen showed them.
    pub async fn consent(&self, c: &Connector, person: &str, seen: &str) -> Result<Value, Refusal> {
        let (classes, listed) = self.classes(c, person).await;
        let shown = terms(c, &classes, listed);
        if shown["terms"] != seen {
            return Err((StatusCode::CONFLICT, "what Magnus can do in this app changed since the screen was shown: read it again".into()));
        }
        let actor = format!("person:{person}");
        let mut evs = vec![Ev::Tools(Change::Consented { app: c.name.clone(), person: person.into(), terms: seen.into(), shown: shown.clone() })];
        if c.tools.is_empty() && third_party(self, c) {
            evs.extend(classes.into_iter().map(|(tool, risk)| Ev::ToolClassified { connector: c.name.clone(), tool, risk: Some(risk) }));
        }
        let evs: Vec<(&str, Ev)> = evs.into_iter().map(|e| (actor.as_str(), e)).collect();
        self.emit_all(evs).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
        Ok(shown)
    }

    /// Signed in to an app whose connect screen could not list its tools
    /// before: class them now, as the screen said it would.
    pub(crate) async fn signed_in_after_consent(&self, app: &str, person: &str) {
        let Some(c) = self.connector(app).filter(|c| awaits_connect(self, c)) else { return };
        let secret = self.connector_secret(&c, person).await.ok().flatten();
        match self.app_tools(&c, secret.as_ref().map(|s| s.expose())).await {
            Ok(live) => {
                if let Err(e) = self.tools_added(&c, &live).await {
                    tracing::warn!("class {app}'s tools after its sign-in: {e:#}");
                }
            }
            Err(e) => tracing::warn!("list {app}'s tools after its sign-in: {e:#}"),
        }
    }

    /// A third-party server's tools that are not classed yet, as it lists
    /// them now. Those its connect screen covered (it could not list them
    /// before the sign-in) and new ones that only read are classed by its
    /// hints; new ones that change or send raise one card for the app, to
    /// its owner: keep them, or disconnect. Never one card per tool, and
    /// never a second while one is open.
    pub(crate) async fn tools_added(&self, c: &Connector, live: &[crate::apps::AppTool]) -> anyhow::Result<()> {
        if !third_party(self, c) || self.read(|s| s.tools.consents.get(&c.name).is_none_or(|p| p.is_empty())) {
            return Ok(());
        }
        static ADDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one_at_a_time = ADDING.lock().await;
        // Read again under the lock: another run may have classed them.
        let Some(c) = self.read(|s| s.connectors.get(&c.name).cloned()) else { return Ok(()) };
        let new: Vec<(String, &'static str)> = live.iter().filter(|t| !c.tools.contains_key(&t.name)).map(|t| (t.name.clone(), suggested_risk(&t.annotations))).collect();
        if new.is_empty() {
            return Ok(());
        }
        let first = c.tools.is_empty();
        let classed = |tool: &str, risk: &str| ("kernel", Ev::ToolClassified { connector: c.name.clone(), tool: tool.into(), risk: Some(risk.into()) });
        let mut evs: Vec<(&str, Ev)> = new.iter().filter(|(_, r)| first || *r == "read").map(|(t, r)| classed(t, r)).collect();
        let writes: Vec<&(String, &str)> = new.iter().filter(|(_, r)| !first && *r != "read").collect();
        let open = self.read(|s| s.items.values().any(|i| i.kind == APP_TOOLS && i.resolution.is_none() && i.details["app"] == c.name.as_str()));
        if !writes.is_empty() && !open {
            let name = c.title.clone().unwrap_or_else(|| title(&c.name));
            let names: Vec<String> = writes.iter().map(|(t, _)| plain(t)).collect();
            let what = if names.len() == 1 { "something new that can change or send" } else { "new things that can change or send" };
            let owner = self.owners().first().map(|p| p.id.clone()).unwrap_or_default();
            evs.push((
                "kernel",
                Ev::Item {
                    id: new_id("item"),
                    item_kind: APP_TOOLS.into(),
                    task: None,
                    run: None,
                    person: owner,
                    summary: format!("{name} added {what}: {}. Keep it, or disconnect {name}.", names.join(", ")),
                    details: json!({ "app": c.name, "title": name, "tools": writes.iter().map(|(t, r)| json!({ "tool": t, "risk": r, "does": plain(t) })).collect::<Vec<Value>>() }),
                },
            ));
        }
        if !evs.is_empty() {
            self.emit_all(evs).await?;
        }
        Ok(())
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/connectors/{name}/terms", get(connect_screen)).route("/v1/connectors/{name}/consent", post(accept)).route("/v1/dev/tool-screens", get(screens))
}

type K = AxState<Arc<Kernel>>;

/// The app as the person may connect it.
async fn usable(k: &Kernel, h: &HeaderMap, name: &str) -> std::result::Result<(String, Connector), Box<Response>> {
    let p = who(k, h).await.map_err(Box::new)?;
    match k.connector(name).filter(|c| crate::boundary::may_use(&p, c)) {
        Some(c) => Ok((p.id, c)),
        None => Err(Box::new(err(StatusCode::NOT_FOUND, "no such app"))),
    }
}

/// The one screen connecting shows.
async fn connect_screen(AxState(k): K, h: HeaderMap, Path(name): Path<String>) -> Response {
    match usable(&k, &h, &name).await {
        Ok((person, c)) => Json(k.connect_terms(&c, &person).await).into_response(),
        Err(r) => *r,
    }
}

/// Accept it: `{ "terms": <the screen's terms> }`.
async fn accept(AxState(k): K, h: HeaderMap, Path(name): Path<String>, Json(b): Json<Value>) -> Response {
    let (person, c) = match usable(&k, &h, &name).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    match k.consent(&c, &person, b["terms"].as_str().unwrap_or_default()).await {
        Ok(v) => Json(v).into_response(),
        Err((st, m)) if st == StatusCode::INTERNAL_SERVER_ERROR => internal(anyhow::anyhow!(m)),
        Err((st, m)) => err(st, m),
    }
}

/// Every tool definition screened lately, newest first, with what the
/// screen found (developer view).
async fn screens(AxState(k): K, h: HeaderMap) -> Response {
    if let Err(r) = crate::api::developer(&k, &h).await {
        return r;
    }
    let (screens, held) = k.read(|s| {
        let held: Vec<Value> = s.tools.held.iter().map(|(t, x)| json!({ "tool": t, "hash": x.hash, "verdict": x.verdict, "finding": x.finding })).collect();
        (s.tools.screens.iter().rev().cloned().collect::<Vec<Value>>(), held)
    });
    Json(json!({ "screens": screens, "held": held })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(catalog: bool, audience: &str, label: &str) -> Connector {
        let mut c = crate::catalog::connector(crate::catalog::manifest("google-gmail").expect("gmail is in the catalog"));
        c.name = "crm".into();
        c.title = None;
        c.catalog = catalog;
        c.audience = audience.into();
        c.label = label.into();
        c
    }

    fn classes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(t, r)| (t.to_string(), r.to_string())).collect()
    }

    #[test]
    fn the_screen_says_what_it_reads_changes_and_sends_and_the_risks() {
        let t = terms(&app(false, "public", "private+untrusted"), &classes(&[("list_deals", "read"), ("update_deal", "reversible_write"), ("send_quote", "irreversible")]), true);
        assert_eq!(t["reads"], json!(["It can: list deals."]));
        assert_eq!(t["changes"], json!(["Changes that can be undone: update deal.", "Changes and sends that cannot be undone: send quote."]));
        let risks = t["risks"].to_string();
        for want in ["could try to steer it", "can reach anyone", "cannot be undone: send quote", "has not reviewed Crm", "asks you once"] {
            assert!(risks.contains(want), "the risks do not say {want:?}: {risks}");
        }
        assert!(!t.to_string().contains("send_quote"), "a tool's own name, not plain words, is on the screen");
    }

    #[test]
    fn the_terms_change_when_what_it_can_do_changes() {
        let a = terms(&app(false, "private", "private"), &classes(&[("list_deals", "read")]), true);
        let b = terms(&app(false, "private", "private"), &classes(&[("list_deals", "read"), ("send_quote", "irreversible")]), true);
        assert_ne!(a["terms"], b["terms"], "consent to reading covered sending");
        assert_eq!(a["terms"], terms(&app(false, "private", "private"), &classes(&[("list_deals", "read")]), true)["terms"]);
    }

    #[test]
    fn a_catalog_app_says_its_updates_come_with_magnus() {
        let t = terms(&app(true, "private", "private"), &classes(&[("list_deals", "read")]), true);
        assert!(t["risks"].to_string().contains("updates come with Magnus updates"), "{t}");
    }

    #[test]
    fn keeping_the_card_classes_each_of_its_tools_as_it_said() {
        let item = Item {
            id: "i".into(),
            kind: APP_TOOLS.into(),
            task: None,
            run: None,
            person: "sam".into(),
            summary: String::new(),
            details: json!({ "app": "crm", "tools": [{ "tool": "send_quote", "risk": "irreversible" }, { "tool": "update_deal", "risk": "reversible_write" }] }),
            created_at: chrono::Utc::now(),
            resolution: None,
        };
        let evs = kept(&item);
        assert_eq!(evs.len(), 2);
        assert!(matches!(&evs[0], Ev::ToolClassified { connector, tool, risk } if connector == "crm" && tool == "send_quote" && risk.as_deref() == Some("irreversible")));
    }
}
