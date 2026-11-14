//! Tool vocabulary: risk classes, inbound labels, audiences, and the built-in
//! tools every run is offered. App tools come from MCP servers, named
//! `<app>__<tool>`, and are classified by the owner in the config.

use crate::tool_registry::{Need, Offer};
use magnus_adapter_proto::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A tool's reach (approvals spec, The approval model): `self` stays in the
/// person's own account (a draft, a label, the trash) and reaches no one, so
/// neither the Rule of Two nor the audience rule counts it as sending, as
/// for a write to Magnus's memory. A tool that declares no reach is
/// unknown, and asks as before. Money never counts as `self`.
pub const REACH_SELF: &str = "self";

/// Whether a declared reach is one Magnus knows, on a tool of this class:
/// `self` on a write; never on a read (nothing to reach), money (it asks
/// every time) or a prohibited tool.
pub fn reach_fits(reach: &str, risk: &str) -> bool {
    reach == REACH_SELF && ["reversible_write", "irreversible"].contains(&risk)
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Read,
    /// Changes only the run's own workspace, which has no network (K42).
    Sandbox,
    ReversibleWrite,
    Irreversible,
    Financial,
    Prohibited,
}

impl Risk {
    pub fn parse(s: &str) -> Option<Risk> {
        Some(match s {
            "read" => Risk::Read,
            "sandbox" => Risk::Sandbox,
            "reversible_write" => Risk::ReversibleWrite,
            "irreversible" => Risk::Irreversible,
            "financial" => Risk::Financial,
            "prohibited" => Risk::Prohibited,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Sandbox => "sandbox",
            Risk::ReversibleWrite => "reversible_write",
            Risk::Irreversible => "irreversible",
            Risk::Financial => "financial",
            Risk::Prohibited => "prohibited",
        }
    }
    /// State change or external communication, for the Rule of Two (K34).
    pub fn changes_state(self) -> bool {
        !matches!(self, Risk::Read | Risk::Sandbox)
    }
}

/// How content is labeled on the way in (B3). A source can be both private
/// and untrusted: a mailbox holds the person's private mail, written by others.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Label {
    pub untrusted: bool,
    pub private: bool,
}

impl Label {
    pub const PRINCIPAL: Label = Label { untrusted: false, private: false };
    pub const PRIVATE: Label = Label { untrusted: false, private: true };
    pub const UNTRUSTED: Label = Label { untrusted: true, private: false };

    /// `principal`, `private`, `untrusted`, or a combination like `private+untrusted`.
    pub fn parse(s: &str) -> Option<Label> {
        let mut l = Label::PRINCIPAL;
        for part in s.split(['+', ',', ' ']).filter(|p| !p.is_empty()) {
            match part {
                "principal" => {}
                "private" => l.private = true,
                "untrusted" => l.untrusted = true,
                _ => return None,
            }
        }
        Some(l)
    }

    pub fn join(self, o: Label) -> Label {
        Label { untrusted: self.untrusted || o.untrusted, private: self.private || o.private }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    Private,
    Internal,
    Public,
}

impl Audience {
    pub fn parse(s: &str) -> Option<Audience> {
        Some(match s {
            "private" => Audience::Private,
            "internal" => Audience::Internal,
            "public" => Audience::Public,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::Private => "private",
            Audience::Internal => "internal",
            Audience::Public => "public",
        }
    }
}

/// Run records the loop writes to the log itself, outside execute() (K21):
/// the registry's, the rich-answer tools', and the retired `task_contract`,
/// whose stored calls still read as a record.
pub static RECORDS: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| crate::tool_registry::records().chain(crate::tools_ui::RECORDS.iter().copied()).chain([crate::tools_ui::RETIRED_CONTRACT]).collect());

/// Built-in actions that pass the boundary like app tools.
pub struct Builtin {
    pub name: &'static str,
    pub app: &'static str,
    pub risk: Risk,
    pub label: Label,
    pub audience: Audience,
}

/// The core family's built-ins, as `tool_registry` declares them.
pub(crate) use crate::tool_registry::CORE_BUILTINS as BUILTINS;

pub fn builtin(name: &str) -> Option<&'static Builtin> {
    crate::families::builtins().find(|b| b.name == name)
}

/// Tools whose calls are their person's alone: no other person's run, the
/// owner, an admin or another developer sees their words (behavior spec,
/// Per-user relationship; #232, #233). Reads served to anyone but the run's
/// person redact these calls' inputs and results (#257).
pub fn person_only_tool(name: &str) -> bool {
    crate::tool_registry::decl(name).is_some_and(|d| d.person_only)
}

/// What a redacted call's input and result say, so the call stays listed.
pub const REDACTED_CALL: &str = "redacted: this call is the person's alone";

/// The appliance's own apps; no added app may take one of their names.
pub fn builtin_apps() -> Vec<&'static str> {
    let mut a: Vec<&str> = crate::families::builtins().map(|b| b.app).collect();
    a.sort();
    a.dedup();
    a
}

pub(crate) fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

/// The tools a chat turn may call besides reads: start a task, propose a
/// schedule the person confirms in the thread, and control their tasks and
/// schedules.
pub fn chat_specs() -> Vec<ToolSpec> {
    crate::tool_registry::specs(|o| o == Offer::Chat)
}

/// Up to this many tools across a person's connected apps, a run is offered
/// all of them; past it, those it finds with `find_tools`.
pub const LOAD_ALL: usize = 40;

/// `find_tools`, offered only when a run's apps have more tools than `LOAD_ALL`.
pub fn find_tools_spec() -> ToolSpec {
    let mut v = crate::tool_registry::specs(|o| o == Offer::FindTools);
    v.pop().unwrap_or_else(|| ToolSpec { name: String::new(), description: String::new(), input_schema: Value::Null })
}

/// The run's built-in tools and records, for what this server has.
pub fn builtin_specs(sandbox: bool, search: bool, browser: bool, computer: bool) -> Vec<ToolSpec> {
    crate::tool_registry::specs(|o| match o {
        Offer::Run(n) => match n {
            Need::Always => true,
            Need::Search => search,
            Need::Sandbox => sandbox,
            Need::Browser => browser,
            Need::Computer => computer,
        },
        _ => false,
    })
}

/// The addresses in a recipients field, lowercased, as they match a grant
/// (approvals spec, Email end to end: exact addresses, and a mailing list
/// is one). It reads a header (`Ann <ann@x.example>, "Doe, Bo" <bo@y>`),
/// a list of them, or objects carrying an `email` or `address`.
pub fn addresses(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Value::String(s) => {
            let (mut part, mut quoted, mut angle) = (String::new(), false, false);
            for ch in s.chars().chain([',']) {
                match ch {
                    '"' if !angle => quoted = !quoted,
                    '<' if !quoted => angle = true,
                    '>' if !quoted => angle = false,
                    ',' | ';' if !quoted && !angle => {
                        if let Some(a) = address(&part) {
                            out.push(a);
                        }
                        part.clear();
                        continue;
                    }
                    _ => {}
                }
                part.push(ch);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| out.extend(addresses(i))),
        Value::Object(m) => out.extend(m.get("email").or_else(|| m.get("address")).map(addresses).unwrap_or_default()),
        _ => {}
    }
    out
}

/// The email addresses written in free text, lowercased. Each is a whole
/// run of the characters an address can hold (RFC 5322 atext, `.` and `@`),
/// so none is ever a piece cut from a longer one: `o'connor@corp.example`
/// and `!alerts@corp.example` stay what they are (review of #213). Only
/// what follows the domain is dropped (`bob@x.example).` reads as
/// `bob@x.example`); the domain must be letters, digits, `-` and dots.
/// Anything else reads as no address, and the send asks.
pub fn addresses_in(text: &str) -> Vec<String> {
    let in_address = |c: char| c.is_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.@".contains(c);
    text.split(|c: char| !in_address(c))
        .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| match w.split_once('@') {
            Some((name, domain)) => !name.is_empty() && domain.contains('.') && !domain.starts_with('.') && domain.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.'),
            None => false,
        })
        .collect()
}

/// One address from `Name <a@x>` or a bare `a@x`.
fn address(part: &str) -> Option<String> {
    let inner = match (part.rfind('<'), part.rfind('>')) {
        (Some(l), Some(r)) if l < r => &part[l + 1..r],
        _ => part,
    };
    let a = inner.trim().trim_matches('"').trim().to_lowercase();
    (!a.is_empty()).then_some(a)
}

#[cfg(test)]
mod descriptions_tests {
    /// The descriptions are compiled in; a broken file would send the tools out without them.
    #[test]
    fn the_tool_descriptions_parse() {
        let d: serde_json::Value = serde_json::from_str(include_str!("../instructions/tools.json")).unwrap();
        assert!(d.is_object() && crate::tool_registry::descriptions() == d);
    }
}

#[cfg(test)]
mod builtin_app_tests {
    /// No added app may take a built-in's app name, the images tools' too:
    /// a connector named "images" would share their app's switches.
    #[test]
    fn every_builtin_app_is_reserved() {
        let apps = super::builtin_apps();
        assert!(apps.contains(&"images"), "images is not reserved: {apps:?}");
        for b in crate::tools_ui::BUILTINS {
            assert!(super::builtin(b.name).is_some_and(|f| apps.contains(&f.app)), "{} is not reserved", b.app);
        }
    }
}

#[cfg(test)]
mod address_tests {
    use super::{addresses, addresses_in};
    use serde_json::json;

    #[test]
    fn addresses_are_read_from_what_a_person_wrote() {
        let said = "Send Bob (Bob@Corp.example) the Q3 draft, cc <ann@x.example>; not @handle, me@, a@b or a@b. or \"quoted@q.example\".";
        assert_eq!(addresses_in(said), ["bob@corp.example", "ann@x.example", "quoted@q.example"]);
        assert!(addresses_in("Reply to Bob about Friday.").is_empty());
    }

    /// An address is never read as a piece of a longer one: the person who
    /// named `o'connor@` did not name `connor@` (review of #213).
    #[test]
    fn an_address_is_read_whole_never_as_a_piece_of_another() {
        assert_eq!(addresses_in("Send it to o'connor@corp.example."), ["o'connor@corp.example"]);
        assert_eq!(addresses_in("cc !alerts@corp.example!"), ["!alerts@corp.example"]);
        assert_eq!(addresses_in("to a+tag@corp.example, b.c@corp.example?"), ["a+tag@corp.example", "b.c@corp.example"]);
        for said in ["o'connor@corp.example", "!alerts@corp.example", "x.connor@corp.example", "'connor@corp.example"] {
            assert_eq!(addresses_in(said), [said], "a piece of {said} was read as an address");
        }
        assert!(addresses_in("\"john doe\"@corp.example and a@b'c.example").is_empty());
    }

    #[test]
    fn a_recipients_field_reads_as_exact_lowercased_addresses() {
        let h = json!(r#"Ann <Ann@X.example>, "Doe, Bo" <bo@y.example>,  team@lists.example ; "#);
        assert_eq!(addresses(&h), ["ann@x.example", "bo@y.example", "team@lists.example"]);
        assert_eq!(addresses(&json!(["A@b.example", { "email": "c@d.example" }, null])), ["a@b.example", "c@d.example"]);
        assert!(addresses(&json!("")).is_empty() && addresses(&json!(null)).is_empty());
    }
}
