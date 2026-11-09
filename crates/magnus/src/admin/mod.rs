//! The admin console (owner's "Magnus Admin Console Spec", built as
//! written, amended by the owner's later rulings): the root control surface
//! for the organisation's appliance, served by its own process,
//! `magnus-admin`, not the user API, reachable only from the organisation's
//! network or its VPN (`net`). Requests must name the console's hostname;
//! anything else is not found. Every page is hardened: a strict
//! Content-Security-Policy, nothing from anywhere but the appliance.
//!
//! Its parts: the process, its listener and its shell (`net`), signing in
//! (`auth`, `session`), live data (`live`), and each section's own module.
//! `docs/admin-console.md` checks every line of the spec against them.

pub mod auth;
pub mod cert;
pub mod connectors;
pub mod kernel;
pub mod knowledge;
pub mod live;
pub mod net;
pub mod ops;
pub mod policy;
pub mod session;
pub mod signin;
pub mod trusted;
pub mod users;

use crate::config::Config;
use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

/// What every page may load: only the appliance's own files, no inline
/// script, never framed.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; manifest-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// The console's settings, checked, and what it keeps while it runs.
pub struct Console {
    pub hostname: String,
    /// The other names it answers to (`admin.aliases`).
    pub aliases: Vec<String>,
    pub org: String,
    pub store: auth::Store,
    /// The console's own session lifetimes: the config's, until an admin
    /// sets them under Security › Sign-in (kept in the log).
    pub idle_minutes: std::sync::atomic::AtomicI64,
    pub session_hours: std::sync::atomic::AtomicI64,
    pub step_up_seconds: i64,
    pub refresh_seconds: i64,
    pub kernel: kernel::Kernel,
    /// The appliance's state, whose disk Health watches.
    pub state_dir: std::path::PathBuf,
    /// The certificates the appliance serves, by what each is for: Overview
    /// warns two weeks before one expires.
    pub certs: Vec<(String, std::path::PathBuf)>,
    /// The subnets connections may come from, as the listener checks them,
    /// and as written: the config's, until an admin changes them.
    pub allow: Arc<std::sync::RwLock<Vec<net::Subnet>>>,
    pub allow_text: std::sync::RwLock<Vec<String>>,
    /// Where the console listens, its tunnel's readiness address and the
    /// tunnel's public names (Operations › Network).
    pub listen: Vec<String>,
    pub tunnel_ready: Option<String>,
    pub public_hostnames: Vec<String>,
    /// The certificates it serves, renewed while it runs.
    pub tls: Arc<trusted::Tls>,
}

impl Console {
    /// Write one console action to the log (admin console spec: every
    /// action, with who, what, when, the source address, and for a change
    /// its before and after), before the action answers.
    pub async fn audit(
        &self,
        who: (&str, &str),
        action: &str,
        target: Option<&str>,
        source: std::net::SocketAddr,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) -> Result<i64> {
        self.kernel
            .audit(&json!({ "admin": who.0, "name": who.1, "action": action, "target": target, "source": source.ip().to_string(), "before": before, "after": after }))
            .await
            .context("record it in the audit log")
    }
}

/// `magnus-admin --config <file>`, or `magnus-admin jwk --config <file>`
/// to print the console's public key, for its client in the realm.
pub fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_env("MAGNUS_LOG").unwrap_or_else(|_| "info".into())).with_writer(std::io::stderr).init();
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.iter().position(|a| a == "--config").and_then(|i| args.get(i + 1)) else {
        eprintln!("usage: magnus-admin --config <file>");
        std::process::exit(2);
    };
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let jwk = args.get(1).map(String::as_str) == Some("jwk");
    let r = rt.block_on(async {
        let cfg = Config::load(std::path::Path::new(path)).with_context(|| format!("load {path}"))?;
        if jwk {
            println!("{}", auth::Key::load_or_make(&key_dir(&cfg))?.jwks());
            return Ok(());
        }
        serve(cfg).await
    });
    if let Err(e) = r {
        eprintln!("magnus-admin: {e:#}");
        std::process::exit(1);
    }
}

/// Serve the console until the process ends. It refuses to start on an
/// address, a subnet or a certificate that would break its reach rules.
pub async fn serve(cfg: Config) -> Result<()> {
    let a = cfg.admin.clone().context("magnus.toml has no [admin] section: the admin console needs its hostname, listen addresses, allowed subnets and certificate")?;
    let addrs = net::listen_addrs(&a.listen)?;
    let allow = Arc::new(std::sync::RwLock::new(net::allowlist(&a.allow)?));
    anyhow::ensure!(valid_host(&a.hostname), "admin.hostname {:?} is not a hostname", a.hostname);
    for alias in &a.aliases {
        anyhow::ensure!(valid_host(alias), "admin.aliases: {alias:?} is not a hostname or an address");
    }
    let tls = Arc::new(trusted::Tls::new(&a)?);
    let console = Arc::new(Console {
        hostname: a.hostname.to_ascii_lowercase(),
        aliases: a.aliases.iter().map(|x| x.to_ascii_lowercase()).collect(),
        org: a.org.clone().unwrap_or_else(|| "Magnus".into()),
        store: Default::default(),
        idle_minutes: std::sync::atomic::AtomicI64::new(a.idle_minutes.into()),
        session_hours: std::sync::atomic::AtomicI64::new(a.session_hours.into()),
        step_up_seconds: a.step_up_seconds.into(),
        refresh_seconds: a.refresh_seconds.into(),
        kernel: kernel::Kernel { path: crate::control::socket_path(&cfg.appliance.state_dir) },
        state_dir: cfg.appliance.state_dir.clone(),
        certs: [
            ("The admin console's trusted certificate".to_string(), tls.trusted_path()),
            ("The admin console's certificate".to_string(), Some(a.tls_cert.clone())),
            ("The appliance's certificate".to_string(), cfg.appliance.tls_cert.clone()),
        ]
        .into_iter()
        .filter_map(|(what, p)| p.map(|p| (what, p)))
        .collect(),
        allow: allow.clone(),
        allow_text: std::sync::RwLock::new(a.allow.clone()),
        listen: a.listen.clone(),
        tunnel_ready: a.tunnel_ready.clone(),
        public_hostnames: a.public_hostnames.clone(),
        tls: tls.clone(),
    });
    // Lifetimes an admin set before this start.
    if let Ok(v) = console.kernel.call(axum::http::Method::GET, "/v1/admin/console-settings", None).await {
        signin::adopt(&console, &v);
        ops::adopt_allow(&console, &v);
    }
    let l = net::Listener::bind(&addrs, allow, net::tls(tls.certs.clone())?).await?;
    tls.keep();
    // tap_io gives the listener axum's connect-info: each request's source,
    // for the audit log.
    use axum::serve::ListenerExt;
    let l = l.tap_io(|_| {});
    axum::serve(l, router(console).into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    Ok(())
}

/// Where the console keeps its own key: the appliance's state, readable by
/// its owner only.
fn key_dir(cfg: &Config) -> std::path::PathBuf {
    cfg.appliance.state_dir.join("admin")
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !l.starts_with('-') && !l.ends_with('-'))
}

pub fn router(console: Arc<Console>) -> Router {
    // Every API route but health is an admin's alone.
    let api = Router::new()
        .route("/api/v1/session", get(session::me).delete(session::logout))
        .route("/api/v1/session/step-up", axum::routing::post(session::confirm))
        .route("/api/v1/events", get(live::events))
        .route("/api/v1/overview", get(ops::overview))
        .route("/api/v1/health", get(ops::health))
        .route("/api/v1/kill-switch", get(ops::switch_state))
        .route("/api/v1/kill-switch/engage", axum::routing::post(ops::engage))
        .route("/api/v1/kill-switch/release", axum::routing::post(ops::release))
        .route("/api/v1/users", get(users::list).post(users::invite))
        .route("/api/v1/users/{id}", get(users::detail).patch(users::update).delete(users::delete))
        .route("/api/v1/users/{id}/schedules", get(users::schedules))
        .route("/api/v1/users/{id}/schedules/{sid}/{change}", axum::routing::post(users::schedule))
        .route("/api/v1/users/{id}/data/{kind}", get(users::data))
        .route("/api/v1/users/{id}/data/{kind}/{item}", get(users::data))
        .route("/api/v1/users/{id}/disable", axum::routing::post(users::disable))
        .route("/api/v1/users/{id}/enable", axum::routing::post(users::enable))
        .route("/api/v1/users/{id}/sessions", axum::routing::delete(users::end_sessions))
        .route("/api/v1/users/{id}/sessions/{sid}", axum::routing::delete(users::end_session))
        .route("/api/v1/users/{id}/invite", axum::routing::post(users::resend).delete(users::cancel))
        .route("/api/v1/users/{id}/invite/password", axum::routing::post(users::one_time_password))
        .route("/api/v1/connectors", get(connectors::list).post(connectors::add))
        .route("/api/v1/connectors/{id}", axum::routing::patch(connectors::update).delete(connectors::remove))
        .route("/api/v1/connectors/{id}/credential", axum::routing::put(connectors::set_credential).delete(connectors::revoke_credential))
        .route("/api/v1/security/sign-in", get(signin::get).patch(signin::patch))
        .route("/api/v1/policy/sets", get(policy::sets))
        .route("/api/v1/policy/sets/{id}/draft", axum::routing::put(policy::draft).delete(policy::drop_draft))
        .route("/api/v1/policy/validate", axum::routing::post(policy::validate))
        .route("/api/v1/policy/test", axum::routing::post(policy::test))
        .route("/api/v1/policy/sets/{id}/apply", axum::routing::post(policy::apply))
        .route("/api/v1/policy/sets/{id}/history", get(policy::history))
        .route("/api/v1/policy/sets/{id}/rollback", axum::routing::post(policy::rollback))
        .route("/api/v1/modules", get(ops::modules).post(ops::module_package))
        .route("/api/v1/modules/{id}", axum::routing::patch(ops::set_module).delete(ops::module_package))
        .route("/api/v1/providers", get(ops::providers))
        .route("/api/v1/providers/{id}", axum::routing::patch(ops::set_provider))
        .route("/api/v1/providers/{id}/test", axum::routing::post(ops::test_provider))
        .route("/api/v1/updates", get(ops::updates))
        .route("/api/v1/updates/install", axum::routing::post(ops::install))
        .route("/api/v1/updates/rollback", axum::routing::post(ops::rollback))
        .route("/api/v1/updates/desktop-channel", axum::routing::patch(ops::desktop_channel))
        .route("/api/v1/network", get(ops::network))
        .route("/api/v1/network/allowlist", axum::routing::patch(ops::set_allowlist))
        .route("/api/v1/network/certificate/renew", axum::routing::post(ops::renew_certificate))
        .route("/api/v1/ontology/types", get(knowledge::types).post(knowledge::create_type))
        .route("/api/v1/ontology/types/merge", axum::routing::post(knowledge::merge_types))
        .route("/api/v1/ontology/types/{id}", get(knowledge::get_type).patch(knowledge::edit_type).delete(knowledge::delete_type))
        .route("/api/v1/ontology/types/{id}/history", get(knowledge::type_history))
        .route("/api/v1/ontology/types/{id}/rollback", axum::routing::post(knowledge::type_rollback))
        .route("/api/v1/knowledge/documents/{id}", get(knowledge::document))
        .route("/api/v1/knowledge/documents/{id}/versions/{v}", get(knowledge::version))
        .route("/api/v1/context/assembled", get(knowledge::assembled))
        .route("/api/v1/context/{section}", get(knowledge::context).patch(knowledge::edit_context))
        .route("/api/v1/memory", get(knowledge::memory))
        .route("/api/v1/memory/bulk", axum::routing::post(knowledge::memory_bulk))
        .route("/api/v1/memory/{id}", get(knowledge::document).patch(knowledge::edit_memory).delete(knowledge::delete_memory))
        .route("/api/v1/memory/{id}/rollback", axum::routing::post(knowledge::memory_rollback))
        .route("/api/v1/activity", get(knowledge::activity))
        .route("/api/v1/activity/revert-batch", axum::routing::post(knowledge::revert_batch))
        .route("/api/v1/activity/{id}", get(knowledge::change))
        .route("/api/v1/activity/{id}/revert", axum::routing::post(knowledge::revert_one))
        .route("/api/v1/audit", get(audit_list))
        .route("/api/v1/audit.csv", get(audit_csv))
        .route("/api/v1/{*rest}", axum::routing::any(api_not_found))
        .layer(axum::middleware::from_fn_with_state(console.clone(), session::guard));
    Router::new()
        .route("/", get(shell))
        .route("/assets/{file}", get(asset))
        .route("/assets/fonts/{file}", get(font))
        .route("/auth/done", get(session::done))
        .route("/auth/local", get(session::local_login).post(session::local_login_post))
        .route("/auth/local/step-up", get(session::local_step_up).post(session::local_step_up_post))
        .route("/status", get(status))
        .merge(api)
        .fallback(not_found)
        .layer(axum::middleware::from_fn_with_state(console.clone(), host_check))
        .layer(axum::middleware::from_fn(harden))
        .with_state(console)
}

/// Only requests naming the console's hostname: anything else is not found,
/// so the console is not even confirmed to be here.
async fn host_check(State(c): State<Arc<Console>>, req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default().to_ascii_lowercase();
    let name = host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map_or(host.as_str(), |(n, _)| n);
    if name != c.hostname && !c.aliases.iter().any(|a| a == name) {
        return not_found().await;
    }
    next.run(req).await
}

/// Every answer: the strict policy, no sniffing, no referrer, never framed,
/// and HTTPS remembered.
async fn harden(req: Request, next: Next) -> Response {
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    for (k, v) in [
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::STRICT_TRANSPORT_SECURITY, "max-age=31536000"),
        (header::CACHE_CONTROL, "no-store"),
    ] {
        h.insert(k, HeaderValue::from_static(v));
    }
    h.insert("cross-origin-opener-policy", HeaderValue::from_static("same-origin"));
    h.insert("cross-origin-resource-policy", HeaderValue::from_static("same-origin"));
    r
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// The audit log, filtered by actor, action, target and time (`since`,
/// `until`), newest first.
async fn audit_list(State(c): State<Arc<Console>>, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Response {
    match audit_rows(&c, q).await {
        Ok(v) => Json(json!({ "entries": v })).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": "kernel", "message": format!("{e:#}") }))).into_response(),
    }
}

async fn audit_rows(c: &Console, q: Option<String>) -> Result<Vec<serde_json::Value>> {
    let path = format!("/v1/audit{}", q.map(|q| format!("?{q}")).unwrap_or_default());
    Ok(c.kernel.call(axum::http::Method::GET, &path, None).await?["entries"].as_array().cloned().unwrap_or_default())
}

/// The same, as CSV.
async fn audit_csv(State(c): State<Arc<Console>>, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Response {
    let rows = match audit_rows(&c, q).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("{e:#}")).into_response(),
    };
    let cell = |v: &serde_json::Value| {
        let s = match v {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        // A cell a spreadsheet would run as a formula is kept as text.
        let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s };
        format!("\"{}\"", s.replace('"', "\"\""))
    };
    let mut out = String::from("seq,at,admin,name,action,target,source,before,after\r\n");
    for r in &rows {
        let cols = ["seq", "at", "admin", "name", "action", "target", "source", "before", "after"].map(|f| cell(&r[f]));
        out.push_str(&cols.join(","));
        out.push_str("\r\n");
    }
    ([(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"magnus-audit.csv\"")], out).into_response()
}

async fn api_not_found() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" }))).into_response()
}

/// Whether the console answers, for the status dot before anyone signs
/// in: nothing more, not even its version.
async fn status() -> Response {
    Json(json!({ "ok": true })).into_response()
}

const SHELL: &str = include_str!("../../../../desktop/admin/index.html");

async fn shell(State(c): State<Arc<Console>>) -> Response {
    let page = SHELL.replace("{{org}}", &html_escape(&c.org));
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page).into_response()
}

/// The console's own files and the desktop app's styles, which it shares
/// (the same components and themes), all from the binary.
async fn asset(axum::extract::Path(file): axum::extract::Path<String>) -> Response {
    let (body, kind): (&'static str, &str) = match file.as_str() {
        "app.css" => (include_str!("../../../../desktop/ui/app.css"), "text/css; charset=utf-8"),
        "theme.js" => (include_str!("../../../../desktop/ui/theme.js"), "text/javascript; charset=utf-8"),
        "admin.css" => (include_str!("../../../../desktop/admin/admin.css"), "text/css; charset=utf-8"),
        "admin.js" => (include_str!("../../../../desktop/admin/admin.js"), "text/javascript; charset=utf-8"),
        "done.js" => (include_str!("../../../../desktop/admin/done.js"), "text/javascript; charset=utf-8"),
        _ => return not_found().await,
    };
    ([(header::CONTENT_TYPE, kind)], body).into_response()
}

/// The desktop app's own fonts, so the console looks as the app does.
async fn font(axum::extract::Path(file): axum::extract::Path<String>) -> Response {
    match crate::ui::assets::FONTS.iter().find(|(n, _)| *n == file) {
        Some((_, body)) => ([(header::CONTENT_TYPE, "font/woff2")], *body).into_response(),
        None => not_found().await,
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_hostname_is_a_hostname() {
        assert!(super::valid_host("admin.acme.internal") && super::valid_host("admin"));
        for bad in ["", "admin..acme", "-admin.acme", "admin.acme/", "admin acme", "10.0.0.1:8443"] {
            assert!(!super::valid_host(bad), "{bad}");
        }
    }
}
