//! Buying things. A purchase is a one-time card from the person's Link wallet,
//! which they approve in Link. Where Link cannot serve them, the card saved in
//! the vault stands in, and every fill of it is approved here. Card details go
//! from the vault or Link straight into the page: never to the model, the log
//! or a file.
mod link;
mod routes;

pub use routes::router;

use crate::boundary::Action;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::tools::{obj, Audience, Builtin, Label, Risk};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CARD_NAME: &str = "payments/card";
pub(crate) const RULE: &str = "kernel:saved_card";
pub(crate) const MONEY_RULE: &str = "kernel:payment";
const MIN_CONTEXT_CHARS: usize = 100;
const MAX_CONTEXT_CHARS: usize = 2000;
/// Link gives a person 30 minutes, then the request is `expired`.
const LINK_APPROVAL_WAIT: Duration = Duration::from_secs(31 * 60);
const LINK_POLL: Duration = Duration::from_secs(2);

pub const BUILTINS: &[Builtin] = &[
    Builtin { name: "purchase_request", app: "payments", risk: Risk::Financial, label: Label::PRINCIPAL, audience: Audience::Private },
    Builtin { name: "purchase_fill", app: "payments", risk: Risk::Financial, label: Label::PRINCIPAL, audience: Audience::Private },
];

fn default_api_url() -> String {
    "https://api.link.com".into()
}

fn default_login_url() -> String {
    "https://login.link.com".into()
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct LinkSettings {
    pub client_id: String,
    pub publishable_key: String,
    /// The vault entry holding the OAuth client secret.
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(default = "default_login_url")]
    pub login_url: String,
    /// Spend requests return test cards and never charge anything.
    #[serde(default)]
    pub test_mode: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub link: Option<LinkSettings>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Card {
    pub number: String,
    pub cvc: String,
    pub exp_month: u32,
    pub exp_year: u32,
    pub name: String,
    pub postal_code: String,
}

fn card_name(person: &str) -> String {
    format!("{CARD_NAME}/{person}")
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Rail {
    Link,
    SavedCard,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Waiting,
    Ready,
    Declined,
    Filled,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Purchase {
    pub id: String,
    pub person: String,
    pub run: String,
    pub merchant_name: String,
    pub merchant_host: String,
    pub amount_minor: i64,
    pub currency: String,
    pub rail: Rail,
    #[serde(default)]
    pub link_id: Option<String>,
    pub status: Status,
    pub at: DateTime<Utc>,
}

/// What happened to a purchase. Never a card's details.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    Requested { purchase: Purchase },
    Decided { id: String, status: Status },
}

#[derive(Default, Clone, Debug)]
pub struct Ledger {
    all: BTreeMap<String, Purchase>,
}

/// Their purchases go with their runs. The log keeps each one's record,
/// which never held a card's details.
impl crate::forget::Forget for Ledger {
    fn forget(&mut self, who: &crate::forget::Erased) {
        self.all.retain(|_, p| !who.is(&p.person));
    }
}

impl Ledger {
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::Requested { purchase } => {
                self.all.insert(purchase.id.clone(), purchase.clone());
            }
            Change::Decided { id, status } => {
                if let Some(p) = self.all.get_mut(id) {
                    p.status = *status;
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Purchase> {
        self.all.get(id)
    }
}

pub fn specs() -> Vec<ToolSpec> {
    let text = json!({ "type": "string" });
    vec![
        ToolSpec {
            name: "purchase_request".into(),
            description: "Ask to buy one thing from one seller. The person approves it. Gives a purchase_id for purchase_fill; you never see the card.".into(),
            input_schema: obj(
                json!({
                    "merchant_name": text,
                    "merchant_url": { "type": "string", "description": "The seller's checkout or product page." },
                    "amount_minor": { "type": "integer", "description": "The total in the smallest unit: 3500 is 35.00." },
                    "currency": { "type": "string", "description": "Three letters. Defaults to usd." },
                    "context": { "type": "string", "description": "What is being bought and why, in at least 100 characters. The person reads this to decide." }
                }),
                &["merchant_name", "merchant_url", "amount_minor", "context"],
            ),
        },
        ToolSpec {
            name: "purchase_fill".into(),
            description: "Put the approved purchase's card into the checkout page open in the site's browser. Works once. `fields` maps number, cvc, and exp_month, exp_year or exp, name and postal_code to the page's field references.".into(),
            input_schema: obj(
                json!({
                    "purchase_id": text,
                    "identity": text,
                    "fields": { "type": "object", "additionalProperties": { "type": "string" } },
                    "submit": { "type": "boolean", "description": "Press Enter in the last field filled. Leave this off to check the page and click its own button." }
                }),
                &["purchase_id", "identity", "fields"],
            ),
        },
    ]
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https")).and_then(|u| u.host_str().map(str::to_ascii_lowercase))
}

fn same_site(page_host: &str, merchant_host: &str) -> bool {
    page_host == merchant_host || page_host.strip_suffix(merchant_host).is_some_and(|rest| rest.ends_with('.'))
}

/// The page's address, from a browser snapshot's first line.
fn address_in(snapshot: &str) -> Option<&str> {
    snapshot.lines().next()?.strip_prefix("URL: ").map(str::trim)
}

pub(crate) fn page_address(snapshot: &str) -> Option<String> {
    address_in(snapshot).map(String::from)
}

fn amount_text(p: &Purchase) -> String {
    format!("{} {}.{:02}", p.currency.to_uppercase(), p.amount_minor / 100, p.amount_minor % 100)
}

/// The value for one of the page's fields. The number and code are the
/// secrets, and only the number is long enough to hide wherever a page repeats it.
fn value_of<'a>(card: &'a Card, field: &str, expiry: &'a str) -> Option<(std::borrow::Cow<'a, str>, bool)> {
    Some(match field {
        "number" => (card.number.as_str().into(), false),
        "cvc" => (card.cvc.as_str().into(), true),
        "exp" => (expiry.into(), true),
        "exp_month" => (format!("{:02}", card.exp_month).into(), true),
        "exp_year" => (card.exp_year.to_string().into(), true),
        "name" => (card.name.as_str().into(), true),
        "postal_code" => (card.postal_code.as_str().into(), true),
        _ => return None,
    })
}

/// Any other action that moves money (a connector tool classed financial)
/// asks the person every time, whatever the policy permits: only
/// `purchase_request` and `purchase_fill` have an approval of their own (in
/// Link, or the saved card's ask).
pub(crate) fn money_ask(tool: &str, risk: Risk) -> Option<(String, Option<String>)> {
    (risk == Risk::Financial && !BUILTINS.iter().any(|b| b.name == tool))
        .then(|| (format!("{tool} moves money, and every payment is the person's to approve"), Some(MONEY_RULE.to_string())))
}

impl Kernel {
    /// The ask every payment gets, whatever the policy says.
    pub(crate) fn payment_ask(&self, a: &Action, risk: Risk) -> Option<(String, Option<String>)> {
        self.saved_card_ask(a).or_else(|| money_ask(&a.tool, risk))
    }

    /// A saved card has no limit of its own, so its every fill is asked about,
    /// whatever the policy says. The approval covers this one action.
    fn saved_card_ask(&self, a: &Action) -> Option<(String, Option<String>)> {
        if a.tool != "purchase_fill" {
            return None;
        }
        let p = self.read(|s| a.input["purchase_id"].as_str().and_then(|id| s.payments.get(id).cloned()))?;
        (p.rail == Rail::SavedCard).then(|| (format!("this would charge the saved card {} at {}", amount_text(&p), p.merchant_name), Some(RULE.to_string())))
    }

    /// What a saved card's ask shows the person: that it is the saved card,
    /// what it costs and where. Not reasoning: the card is drawn from it.
    pub(crate) fn saved_card_charge(&self, a: &Action) -> Option<Value> {
        if a.tool != "purchase_fill" {
            return None;
        }
        let p = self.read(|s| a.input["purchase_id"].as_str().and_then(|id| s.payments.get(id).cloned()))?;
        (p.rail == Rail::SavedCard).then(|| json!({ "saved_card": true, "amount": amount_text(&p), "merchant": p.merchant_name }))
    }

    pub(crate) async fn purchase_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let person = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone())).context("no such run")?;
        if a.tool == "purchase_fill" {
            return self.purchase_fill(a, &person).await;
        }
        self.purchase_request(a, &person).await
    }

    async fn record_purchase(&self, purchase: Purchase) -> Result<()> {
        self.emit("kernel", Ev::Payment(Change::Requested { purchase })).await.map(|_| ())
    }

    async fn decide_purchase(&self, id: &str, status: Status) -> Result<()> {
        self.emit("kernel", Ev::Payment(Change::Decided { id: id.to_string(), status })).await.map(|_| ())
    }

    fn saved_card(&self, person: &str) -> Result<Option<Card>> {
        self.vault.get(&card_name(person))?.map(|raw| serde_json::from_str(raw.expose()).context("the saved card")).transpose()
    }

    async fn purchase_request(self: &Arc<Self>, a: &Action, person: &str) -> Result<(String, bool, Option<Value>)> {
        let Some(settings) = self.cfg.payments.as_ref() else { return Ok(("Payments are not set up here.".into(), true, None)) };
        let text = |key: &str| a.input[key].as_str().map(str::trim).unwrap_or_default().to_string();
        let (merchant_name, merchant_url, context) = (text("merchant_name"), text("merchant_url"), text("context"));
        let amount_minor = a.input["amount_minor"].as_i64().unwrap_or_default();
        let currency = Some(text("currency").to_ascii_lowercase()).filter(|c| !c.is_empty()).unwrap_or_else(|| "usd".to_string());
        let Some(merchant_host) = host_of(&merchant_url) else { return Ok(("merchant_url is not a web address.".into(), true, None)) };
        if merchant_name.is_empty() || amount_minor <= 0 || currency.len() != 3 {
            return Ok(("A purchase needs a seller and an amount above zero.".into(), true, None));
        }
        if !(MIN_CONTEXT_CHARS..=MAX_CONTEXT_CHARS).contains(&context.chars().count()) {
            return Ok((format!("context must say what is being bought and why, in {MIN_CONTEXT_CHARS} to {MAX_CONTEXT_CHARS} characters."), true, None));
        }
        // The same step retried after a restart is the same purchase.
        let key = hex::encode(Sha256::digest(format!("{}:{}", a.run, a.step_id).as_bytes()));
        let id = format!("pur_{}", &key[..24]);
        let purchase = |rail, link_id, status| Purchase {
            id: id.clone(),
            person: person.to_string(),
            run: a.run.clone(),
            merchant_name: merchant_name.clone(),
            merchant_host: merchant_host.clone(),
            amount_minor,
            currency: currency.clone(),
            rail,
            link_id,
            status,
            at: Utc::now(),
        };
        let mut unavailable = "Link is not set up here".to_string();
        if let Some(s) = settings.link.as_ref().filter(|_| link::connected(self, person)) {
            let new = link::NewSpend { amount_minor, currency: &currency, merchant_name: &merchant_name, merchant_url: &merchant_url, context: &context, idempotency_key: &key };
            match link::create(self, s, person, &new).await {
                Ok(spend) => {
                    self.record_purchase(purchase(Rail::Link, Some(spend.id.clone()), Status::Waiting)).await?;
                    return self.wait_for_link(a, s, person, &id, spend).await;
                }
                Err(link::Failure::Refused(why)) => return Ok((format!("Link refused this purchase: {why}"), true, None)),
                Err(link::Failure::Unavailable(why)) => unavailable = why,
            }
        } else if settings.link.is_some() {
            unavailable = "this person has not connected Link".to_string();
        }
        if self.saved_card(person)?.is_none() {
            return Ok((format!("This purchase cannot go ahead: {unavailable}, and no card is saved. The person can connect Link or save a card in Settings."), true, None));
        }
        tracing::info!("purchase {id}: the saved card stands in for Link: {unavailable}");
        self.record_purchase(purchase(Rail::SavedCard, None, Status::Ready)).await?;
        Ok((
            format!("Link is not available, so the saved card will be used. purchase_id: {id}. The person approves it when you call purchase_fill on the checkout page."),
            false,
            None,
        ))
    }

    /// Until the person has answered in Link. An answer of no ends the
    /// purchase: nothing else may pay for what they just declined.
    async fn wait_for_link(self: &Arc<Self>, a: &Action, s: &LinkSettings, person: &str, id: &str, first: link::Spend) -> Result<(String, bool, Option<Value>)> {
        let mut items = vec![self.link_item(a, person, id, "Approve this purchase in Link.".to_string(), first.approval_url.clone()).await?];
        let started = Instant::now();
        let mut spend = first;
        let mut told = false;
        let outcome = loop {
            match spend.status.as_str() {
                "approved" => break Status::Ready,
                "denied" | "expired" | "canceled" | "cancelled" | "failed" => break Status::Declined,
                // Not a decline: the wallet needs the person's attention.
                "requires_action" if !told => {
                    told = true;
                    let next = &spend.status_details["requires_action"]["next_action"];
                    let said = next["display_message"].as_str().unwrap_or("Link needs your attention before this purchase can go ahead.").to_string();
                    items.push(self.link_item(a, person, id, said, next["action_url"].as_str().map(String::from)).await?);
                }
                _ => {}
            }
            if started.elapsed() > LINK_APPROVAL_WAIT {
                break Status::Declined;
            }
            tokio::time::sleep(LINK_POLL).await;
            match link::retrieve(self, s, person, &spend.id, false).await {
                Ok(next) => spend = next,
                Err(link::Failure::Unavailable(why) | link::Failure::Refused(why)) => tracing::warn!("purchase {id}: {why}"),
            }
        };
        self.decide_purchase(id, outcome).await?;
        for item in items {
            self.emit("kernel", Ev::ItemResolved { id: item, decision: if outcome == Status::Ready { "approved" } else { "declined" }.into(), scope: None, text: None }).await?;
        }
        Ok(match outcome {
            Status::Ready => (format!("The person approved it. purchase_id: {id}. Open the seller's checkout page and call purchase_fill."), false, None),
            _ => (format!("The person did not approve this purchase in Link ({}). Do not buy it another way.", spend.status), true, None),
        })
    }

    /// Something for the person to do in Link, with the address to do it at.
    async fn link_item(&self, a: &Action, person: &str, id: &str, summary: String, url: Option<String>) -> Result<String> {
        let item = crate::kernel::new_id("item");
        let task = self.read(|s| s.runs.get(&a.run).map(|r| r.task.clone()));
        let details = json!({ "purchase_id": id, "url": url });
        self.emit("kernel", Ev::Item { id: item.clone(), item_kind: "payment".into(), task, run: Some(a.run.clone()), person: person.to_string(), summary, details }).await?;
        Ok(item)
    }

    async fn purchase_fill(self: &Arc<Self>, a: &Action, person: &str) -> Result<(String, bool, Option<Value>)> {
        let wanted = a.input["purchase_id"].as_str().unwrap_or_default();
        let Some(p) = self.read(|s| s.payments.get(wanted).cloned()).filter(|p| p.person == person) else { return Ok(("There is no such purchase.".into(), true, None)) };
        match p.status {
            Status::Ready => {}
            Status::Filled => return Ok(("This purchase's card was already used. Each purchase is filled once.".into(), true, None)),
            Status::Waiting | Status::Declined => return Ok(("This purchase is not approved.".into(), true, None)),
        }
        let Some(fields) = a.input["fields"].as_object().filter(|f| f.contains_key("number") && f.contains_key("cvc")) else {
            return Ok(("fields needs at least number and cvc.".into(), true, None));
        };
        let identity = a.input["identity"].as_str().unwrap_or_default();
        let page = match self.checkout_page(person, identity).await {
            Ok(page) => page,
            Err(e) => return Ok((format!("The checkout page could not be read: {e:#}"), true, None)),
        };
        if !host_of(&page).is_some_and(|h| same_site(&h, &p.merchant_host)) {
            return Ok((format!("This card is only for {}. The page open is {page}.", p.merchant_host), true, None));
        }
        let card = match self.card_for(&p).await {
            Ok(card) => card,
            Err(e) => return Ok((format!("The card could not be fetched: {e:#}"), true, None)),
        };
        let expiry = format!("{:02}/{:02}", card.exp_month, card.exp_year % 100);
        let mut plan = Vec::with_capacity(fields.len());
        for (field, target) in fields {
            let (Some((value, quiet)), Some(target)) = (value_of(&card, field, &expiry), target.as_str()) else {
                return Ok((format!("{field} is not a card field."), true, None));
            };
            plan.push((target.to_string(), value.into_owned(), quiet));
        }
        // Written first: a card that may have reached a page is spent.
        self.decide_purchase(&p.id, Status::Filled).await?;
        let last = plan.len().saturating_sub(1);
        for (at, (target, value, quiet)) in plan.iter().enumerate() {
            let submit = at == last && a.input["submit"].as_bool().unwrap_or(false);
            if let Err(e) = self.fill_checkout_field(identity, target, value, *quiet, submit).await {
                tracing::warn!("purchase {}: a field could not be filled: {e:#}", p.id);
                return Ok(("The page could not be filled, and this purchase's card is spent. Ask for a new purchase if the person still wants it.".into(), true, None));
            }
        }
        Ok((format!("The card is on the page for {} at {}. Check the page and finish the checkout.", amount_text(&p), p.merchant_name), false, None))
    }

    async fn card_for(&self, p: &Purchase) -> Result<Card> {
        if p.rail == Rail::SavedCard {
            return self.saved_card(&p.person)?.context("the saved card is gone");
        }
        let s = self.cfg.payments.as_ref().and_then(|c| c.link.as_ref()).context("Link is not set up")?;
        let id = p.link_id.as_deref().context("no spend request")?;
        let spend = link::retrieve(self, s, &p.person, id, true).await.map_err(|f| match f {
            link::Failure::Unavailable(why) | link::Failure::Refused(why) => anyhow::anyhow!(why),
        })?;
        anyhow::ensure!(link::valid_until(&spend).is_none_or(|until| until > Utc::now()), "the one-time card has expired");
        link::card_of(&spend).context("Link gave no card")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Card {
        Card { number: "4242424242424242".into(), cvc: "100".into(), exp_month: 6, exp_year: 2029, name: "Jo".into(), postal_code: "94015".into() }
    }

    #[test]
    fn a_card_works_on_its_seller_and_its_subdomains_only() {
        assert!(same_site("shop.test", "shop.test"));
        assert!(same_site("checkout.shop.test", "shop.test"));
        assert!(!same_site("evilshop.test", "shop.test"));
        assert!(!same_site("shop.test.evil.example", "shop.test"));
    }

    #[test]
    fn the_number_is_hidden_wherever_a_page_repeats_it_and_short_values_are_not() {
        let c = card();
        assert_eq!(value_of(&c, "number", "06/29").map(|(_, quiet)| quiet), Some(false));
        for short in ["cvc", "exp", "exp_month", "exp_year", "name", "postal_code"] {
            assert_eq!(value_of(&c, short, "06/29").map(|(_, quiet)| quiet), Some(true), "{short}");
        }
        assert!(value_of(&c, "pin", "06/29").is_none());
        assert_eq!(value_of(&c, "exp_month", "06/29").map(|(v, _)| v.into_owned()), Some("06".to_string()));
    }

    #[test]
    fn a_page_address_comes_from_the_snapshots_first_line() {
        assert_eq!(address_in("URL: https://shop.test/pay\nTitle: Pay\n\nbutton"), Some("https://shop.test/pay"));
        assert_eq!(address_in("Title: x"), None);
    }

    /// A connector's financial tool asks the person even where the policy
    /// permits it; before, a permit left it to Auto Review alone. Link
    /// purchases and saved-card fills keep their own approval.
    #[test]
    fn every_payment_outside_purchases_asks_the_person_whatever_the_policy() {
        let (why, rule) = money_ask("crm__pay", Risk::Financial).expect("a permitted payment went unasked");
        assert!(why.contains("crm__pay"));
        assert_eq!(rule.as_deref(), Some(MONEY_RULE));
        assert!(MONEY_RULE.starts_with("kernel:"), "a broader approval could cover a payment");
        assert!(money_ask("crm__update", Risk::Irreversible).is_none());
        assert!(money_ask("purchase_request", Risk::Financial).is_none(), "a Link purchase was asked twice");
        assert!(money_ask("purchase_fill", Risk::Financial).is_none());
    }

    #[test]
    fn the_ledger_remembers_each_purchases_last_state() {
        let mut l = Ledger::default();
        let p = Purchase {
            id: "p".into(),
            person: "a".into(),
            run: "r".into(),
            merchant_name: "Shop".into(),
            merchant_host: "shop.test".into(),
            amount_minor: 3505,
            currency: "usd".into(),
            rail: Rail::Link,
            link_id: Some("lsrq".into()),
            status: Status::Waiting,
            at: Utc::now(),
        };
        l.apply(&Change::Requested { purchase: p });
        l.apply(&Change::Decided { id: "p".into(), status: Status::Ready });
        assert_eq!(l.get("p").map(|p| p.status), Some(Status::Ready));
        assert_eq!(l.get("p").map(amount_text), Some("USD 35.05".to_string()));
    }

    #[test]
    fn nothing_about_a_card_can_enter_the_log() {
        let v = serde_json::to_string(&Change::Decided { id: "p".into(), status: Status::Filled }).unwrap();
        assert_eq!(v, r#"{"change":"decided","id":"p","status":"filled"}"#);
    }
}
