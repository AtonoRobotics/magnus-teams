//! What Magnus knows of the person's apps while it works (connectors spec,
//! How the agent uses connectors): every run and chat holds the person's
//! app index, each app their organisation lets them use with whether they
//! have connected it; `find_apps` searches their Library by an app's name
//! or by what they want done; `app_details` reads one app; `offer_app`
//! puts its card in front of them, saying what it is for. Only the person
//! signs in: Magnus never connects an app itself.

use crate::boundary::may_use;
use crate::config::{Connector, Person};
use crate::kernel::Kernel;
use serde_json::{json, Value};

/// Longest reason an offered card gives.
const MAX_WHY: usize = 200;

impl Kernel {
    /// The apps the person may use, on for the organisation, each with
    /// where they stand: connected (or it takes no sign-in), their sign-in
    /// refused by the app, or not connected.
    pub fn apps_of(&self, person: &Person) -> Vec<(Connector, State)> {
        self.connectors()
            .into_iter()
            .filter(|c| may_use(person, c))
            .map(|c| {
                let state = if self.lapsed(&c, &person.id) {
                    State::Lapsed
                } else if !self.wanted_scopes(&c, &person.id).is_empty() {
                    State::MoreAccess
                } else if self.connected(&c, &person.id) {
                    State::Connected
                } else {
                    State::Not
                };
                (c, state)
            })
            .collect()
    }

    /// What a connected app answered when Magnus last asked it for its tools
    /// and it refused, if it did.
    fn refused(&self, c: &Connector, person: &Person) -> Option<String> {
        self.refused_apps.lock().unwrap().get(&crate::connectors::connection_name(&c.name, &person.id)).cloned()
    }

    /// The person's app index, as the run's context holds it.
    pub fn app_index(&self, person: &Person) -> Option<String> {
        let apps = self.apps_of(person);
        if apps.is_empty() {
            return None;
        }
        let mut t = "The person's apps (their Library), as of now. A connected app's tools are offered to you. For one they have not connected, offer_app puts its card in front of them, saying what it is for; only they sign in. find_apps and app_details tell you more.\n".to_string();
        for (c, state) in &apps {
            let l = line(c, *state);
            let refused = (*state == State::Connected).then(|| self.refused(c, person)).flatten().map(|r| {
                format!("Connected, but the app refused Magnus when asked for its tools ({r}), so none of them can be used now. Tell the person plainly that the app is refusing Magnus; do not try another way, such as the browser.")
            });
            let says = match state {
                State::Connected if refused.is_some() => refused.as_deref().unwrap_or_default(),
                State::Connected => "Connected.",
                State::Lapsed => "Needs signing in again: the app no longer accepts their sign-in; offer_app lets them sign in again.",
                State::MoreAccess => "Connected, but it needs more access for something you asked of it; offer_app asks them to approve it.",
                State::Not => "Not connected.",
            };
            t.push_str(&format!("- {} ({}): {} {says}\n", l["title"].as_str().unwrap_or_default(), c.name, l["summary"].as_str().unwrap_or_default()));
        }
        Some(t)
    }

    /// `find_apps`: the person's apps whose name, publisher, summary or
    /// abilities hold the words asked for, best first; every app for no words.
    pub fn find_apps(&self, person: &Person, query: &str) -> Value {
        let words: Vec<String> = query.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect();
        let mut found: Vec<(usize, Value)> = self
            .apps_of(person)
            .into_iter()
            .filter_map(|(c, state)| {
                let l = line(&c, state);
                let hay = format!("{} {} {} {} {}", c.name, l["title"], l["summary"], l["publisher"], l["abilities"]).to_lowercase();
                let score = words.iter().filter(|w| hay.contains(w.as_str())).count();
                (words.is_empty() || score > 0).then_some((score, l))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        let apps: Vec<Value> = found.into_iter().map(|(_, l)| l).collect();
        if apps.is_empty() {
            return json!({ "apps": [], "note": "No app in the person's Library does that. Say so plainly; do not suggest one that is not there." });
        }
        json!({ "apps": apps })
    }

    /// `app_details`: what an app lets Magnus do, the access it asks for,
    /// where the person signs in, and whether they have.
    pub fn app_details(&self, person: &Person, name: &str) -> Result<Value, String> {
        let (c, state) =
            self.apps_of(person).into_iter().find(|(c, _)| c.name == name).ok_or_else(|| format!("{name} is not in the person's Library. find_apps lists what is."))?;
        let mut l = line(&c, state);
        let listing = crate::connectors::listing(&c);
        for k in ["asks", "can", "cannot", "category"] {
            l[k] = listing[k].clone();
        }
        l["signs_in"] = json!(c.auth.as_deref() == Some(crate::connectors::OAUTH));
        if let Some(r) = (state == State::Connected).then(|| self.refused(&c, person)).flatten() {
            l["refused"] = json!(format!("The app refused Magnus when asked for its tools ({r}): none of them can be used until it accepts Magnus again."));
        }
        Ok(l)
    }

    /// An offered app's card as the person sees it now, from what
    /// `offer_app` recorded: the app as they stand with it, or gone from
    /// their Library. The same in a conversation and in a task.
    pub fn offered_card(&self, person: &str, offered: &Value) -> Value {
        let mut e = offered.clone();
        e["kind"] = json!("connector_card");
        let app = offered["connector"].as_str().unwrap_or_default();
        match self.person(person).and_then(|p| self.apps_of(&p).into_iter().find(|(c, _)| c.name == app)) {
            Some((c, state)) => {
                let l = crate::connectors::listing(&c);
                e["title"] = json!(c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name)));
                e["summary"] = json!(c.summary.clone().unwrap_or_else(|| crate::connectors::allows(&c).0));
                e["letters"] = l["letters"].clone();
                e["tint"] = l["tint"].clone();
                e["publisher"] = l["publisher"].clone();
                e["state"] = json!(match state {
                    State::Connected => "connected",
                    State::Lapsed => "needs_attention",
                    State::MoreAccess => "needs_more_access",
                    State::Not => "available",
                });
                e["ready"] = json!(self.provider_ready(&c));
            }
            None => e["state"] = json!("unavailable"),
        }
        e
    }

    /// `offer_app`: the app's card, in front of the person with why it is
    /// needed. Nothing is connected until they sign in themselves.
    pub fn offer_app(&self, person: &Person, input: &Value, card: String) -> (String, bool, Option<Value>) {
        let name = input["name"].as_str().map(str::trim).unwrap_or_default();
        let why = input["why"].as_str().map(str::trim).unwrap_or_default();
        if why.is_empty() || why.chars().count() > MAX_WHY {
            return (format!("offer_app needs why: what the app is for here, in a short sentence (at most {MAX_WHY} characters)."), true, None);
        }
        let Some((c, state)) = self.apps_of(person).into_iter().find(|(c, _)| c.name == name) else {
            return (format!("{name} is not in the person's Library, so it cannot be offered. find_apps lists what is; if nothing fits, say so."), true, None);
        };
        let t = line(&c, state);
        if state == State::Connected {
            return (format!("{} is already connected: its tools are yours to use.", t["title"].as_str().unwrap_or(name)), false, None);
        }
        let data = json!({ "card": card, "connector": c.name, "why": why });
        (
            format!(
                "{}'s card is in front of the person with your reason. Nothing is connected until they sign in themselves; once they have, its tools are offered to you.",
                t["title"].as_str().unwrap_or(name)
            ),
            false,
            Some(data),
        )
    }
}

/// Where the person stands with one of their apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Connected,
    /// The app refused their sign-in: they sign in again.
    Lapsed,
    /// Connected, and the app asked for more access than it holds.
    MoreAccess,
    Not,
}

/// One app as the agent reads it: no protocol words.
fn line(c: &Connector, state: State) -> Value {
    let (sentence, _) = crate::connectors::allows(c);
    let l = crate::connectors::listing(c);
    json!({
        "name": c.name,
        "title": c.title.clone().unwrap_or_else(|| crate::connectors::title(&c.name)),
        "summary": c.summary.clone().unwrap_or(sentence),
        "publisher": l["publisher"], "abilities": l["abilities"],
        "state": match state {
            State::Connected => "connected",
            State::Lapsed => "needs signing in again",
            State::MoreAccess => "needs more access",
            State::Not => "not connected",
        },
    })
}
