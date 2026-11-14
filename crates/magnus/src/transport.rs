//! How a connector's calls travel: the one seam between the kernel and the
//! two adapters that exist, a remote app over MCP's streamable HTTP
//! (`apps::Mcp`) and a brokered server in a microVM (`brokered::Brokered`).
//!
//! A connector's `transport` string is parsed here, once, into a `Kind`;
//! everything that differs between the two is asked of the `Transport` the
//! kernel hands out for a connector, never by comparing the string.

use crate::apps::{AppTool, CallError, CallOutput, Mcp};
use crate::brokered::Brokered;
use crate::config::Connector;
use anyhow::Result;
use serde_json::Value;

pub use crate::brokered::Fence;

/// A remote app's `transport`, the default.
pub const MCP_HTTP: &str = "mcp_http";
/// A brokered connector's `transport`.
pub const BROKERED: &str = "brokered";

/// The `transport` a connector has when its config names none.
pub fn default_name() -> String {
    MCP_HTTP.into()
}

/// Which adapter a connector's calls go through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A remote app, over MCP's streamable HTTP.
    McpHttp,
    /// A brokered server, run from its image in a microVM per person.
    Brokered,
}

impl Kind {
    /// The kind a `transport` string names; None for any other string,
    /// which the config refuses at startup.
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            MCP_HTTP => Some(Kind::McpHttp),
            BROKERED => Some(Kind::Brokered),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::McpHttp => MCP_HTTP,
            Kind::Brokered => BROKERED,
        }
    }

    /// Whether its tool definitions are fixed by the release: a brokered
    /// server's come from the image its manifest names by digest, checked
    /// before every boot, so they cannot change after it was signed.
    pub fn definitions_fixed(self) -> bool {
        self == Kind::Brokered
    }

    /// Whether its sign-in is the provider's own, at the authorization
    /// server its manifest names and for the provider's API (no resource
    /// indicator), rather than the one its MCP server asks for.
    pub fn signs_in_at_provider(self) -> bool {
        self == Kind::Brokered
    }
}

impl Connector {
    /// This connector's kind. A configured one was refused at startup
    /// unless its `transport` parses; one the server made (a catalog
    /// install, a generated connector) carries one of the two names, and
    /// anything else read from an older event is a remote app, as it was.
    pub fn kind(&self) -> Kind {
        Kind::parse(&self.transport).unwrap_or(Kind::McpHttp)
    }
}

/// A connector's adapter, borrowed from the kernel for one use.
pub(crate) enum Transport<'a> {
    Mcp {
        mcp: &'a Mcp,
        url: &'a str,
    },
    Brokered {
        brokered: &'a Brokered,
        c: &'a Connector,
    },
    #[cfg(test)]
    Fake(std::sync::Arc<fake::Fake>),
}

impl Transport<'_> {
    /// The app's tools as it lists them now: a remote one's with the
    /// credential, a brokered one's from its image.
    pub(crate) async fn tools(&self, bearer: Option<&str>) -> Result<Vec<AppTool>> {
        match self {
            Transport::Mcp { mcp, url } => mcp.list_tools(url, bearer).await,
            Transport::Brokered { brokered, c } => brokered.list_tools(c).await,
            #[cfg(test)]
            Transport::Fake(f) => Ok(f.tools.clone()),
        }
    }

    /// Call `tool` for `person`. `fence` is checked once a brokered call has
    /// its microVM and again just before it sends; a remote app's call has
    /// no such wait, so the caller's own checks just before it are its
    /// fence. `key` is the idempotency key sent to a remote app that honours
    /// one; a brokered connector never does (the config refuses it).
    pub(crate) async fn call(&self, person: &str, bearer: Option<&str>, tool: &str, args: &Value, fence: &Fence<'_>, key: Option<&str>) -> Result<CallOutput, CallError> {
        match self {
            Transport::Mcp { mcp, url } => mcp.call(url, bearer, tool, args, key).await,
            Transport::Brokered { brokered, c } => brokered.call(c, person, bearer, tool, args, fence).await,
            #[cfg(test)]
            Transport::Fake(f) => f.call(person, bearer, tool, args, fence, key).await,
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        match self {
            Transport::Mcp { .. } => Kind::McpHttp,
            Transport::Brokered { .. } => Kind::Brokered,
            #[cfg(test)]
            Transport::Fake(f) => f.kind,
        }
    }

    /// Where `tool`'s view lives, when it ships one (MCP Apps). Only a
    /// remote app has views; a brokered server ships none.
    pub(crate) fn view_of(&self, tool: &str) -> Option<String> {
        match self {
            Transport::Mcp { mcp, url } => mcp.view_of(url, tool),
            _ => None,
        }
    }

    pub(crate) async fn read_view(&self, bearer: Option<&str>, uri: &str) -> Result<String, CallError> {
        match self {
            Transport::Mcp { mcp, url } => mcp.read_view(url, bearer, uri).await,
            _ => Err(CallError::NotSent("this app has no views".into())),
        }
    }
}

impl crate::kernel::Kernel {
    /// The adapter `c`'s calls go through.
    pub(crate) fn transport<'a>(&'a self, c: &'a Connector) -> Transport<'a> {
        #[cfg(test)]
        if let Some(f) = fake::installed(&c.url) {
            return Transport::Fake(f);
        }
        match c.kind() {
            Kind::McpHttp => Transport::Mcp { mcp: &self.mcp, url: &c.url },
            Kind::Brokered => Transport::Brokered { brokered: &self.brokered, c },
        }
    }

    /// An app's tools as it lists them now.
    pub(crate) async fn app_tools(&self, c: &Connector, bearer: Option<&str>) -> Result<Vec<AppTool>> {
        self.transport(c).tools(bearer).await
    }
}

/// An adapter for tests: what it was sent, answered as told, with the fence
/// checked as a brokered call checks it. Installed under a connector's url.
#[cfg(test)]
pub(crate) mod fake {
    use super::{Fence, Kind};
    use crate::apps::{AppTool, CallError, CallOutput};
    use serde_json::Value;
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex, OnceLock};

    /// One call the fake was sent.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct Sent {
        pub person: String,
        pub bearer: Option<String>,
        pub tool: String,
        pub args: Value,
        pub key: Option<String>,
    }

    pub(crate) struct Fake {
        pub kind: Kind,
        pub tools: Vec<AppTool>,
        /// What the next calls answer, in turn; "done" once they run out.
        pub answers: Mutex<VecDeque<Result<CallOutput, CallError>>>,
        pub sent: Mutex<Vec<Sent>>,
        /// When set, a call says it is waiting (the first) and goes on to its
        /// fence only once told to (the second), as a microVM's boot waits.
        pub hold: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    }

    fn fakes() -> &'static Mutex<HashMap<String, Arc<Fake>>> {
        static FAKES: OnceLock<Mutex<HashMap<String, Arc<Fake>>>> = OnceLock::new();
        FAKES.get_or_init(Default::default)
    }

    pub(crate) fn installed(url: &str) -> Option<Arc<Fake>> {
        fakes().lock().unwrap_or_else(|e| e.into_inner()).get(url).cloned()
    }

    impl Fake {
        pub(crate) fn new(kind: Kind) -> Fake {
            Fake { kind, tools: vec![], answers: Mutex::default(), sent: Mutex::default(), hold: None }
        }

        /// Install it under a fresh url, for the connector it stands for.
        pub(crate) fn install(self) -> (String, Arc<Fake>) {
            let url = format!("fake://{}", uuid::Uuid::new_v4().simple());
            let f = Arc::new(self);
            fakes().lock().unwrap_or_else(|e| e.into_inner()).insert(url.clone(), f.clone());
            (url, f)
        }

        pub(crate) fn sent(&self) -> Vec<Sent> {
            self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        pub(super) async fn call(&self, person: &str, bearer: Option<&str>, tool: &str, args: &Value, fence: &Fence<'_>, key: Option<&str>) -> Result<CallOutput, CallError> {
            if let Some((waiting, go)) = &self.hold {
                waiting.notify_one();
                go.notified().await;
            }
            fence().map_err(CallError::NotSent)?;
            let s = Sent { person: person.into(), bearer: bearer.map(String::from), tool: tool.into(), args: args.clone(), key: key.map(String::from) };
            self.sent.lock().unwrap_or_else(|e| e.into_inner()).push(s);
            let next = self.answers.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            next.unwrap_or_else(|| Ok(CallOutput { text: "done".into(), structured: None, is_error: false }))
        }
    }
}

#[cfg(test)]
#[test]
fn a_transport_name_parses_to_its_kind_and_back_and_no_other_name_does() {
    for k in [Kind::McpHttp, Kind::Brokered] {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
    assert_eq!(default_name(), "mcp_http");
    assert_eq!(BROKERED, "brokered");
    for other in ["http", "", "Brokered", "mcp_stdio"] {
        assert_eq!(Kind::parse(other), None, "{other}");
    }
    assert!(Kind::Brokered.definitions_fixed() && Kind::Brokered.signs_in_at_provider());
    assert!(!Kind::McpHttp.definitions_fixed() && !Kind::McpHttp.signs_in_at_provider());
}

/// A configured connector names one of the two transports, or none (a
/// remote app); any other name stops the server at startup rather than
/// being taken for a remote app.
#[cfg(test)]
#[test]
fn the_config_refuses_a_transport_it_does_not_know() {
    use crate::legal::TEST_CONFIG as LEGAL;
    let load = |transport: &str| {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("magnus.toml");
        let text = format!(
            "[appliance]\nstate_dir = \"/s\"\nlisten = \"127.0.0.1:0\"\n[database]\nurl = \"postgres://x\"\n[model]\nname = \"m\"\nadapter = \"a\"\nmax_tokens = 1\n{LEGAL}[[connectors]]\nname = \"mail\"\naudience = \"public\"\nlabel = \"private+untrusted\"\nurl = \"https://a.example/mcp\"\n{transport}"
        );
        std::fs::write(&p, text).unwrap();
        crate::config::Config::load(&p)
    };
    let e = load("transport = \"mcp_stdio\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("connector mail: unknown transport mcp_stdio"), "{e:#}");
    let c = load("").unwrap().connectors.remove(0);
    assert_eq!((c.transport.as_str(), c.kind()), ("mcp_http", Kind::McpHttp));
    assert_eq!(load("transport = \"mcp_http\"\n").unwrap().connectors[0].kind(), Kind::McpHttp);
}
