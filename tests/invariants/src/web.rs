//! A fake website. Pages are untrusted content; every hit is recorded with its
//! full path and query, so an exfiltration attempt by URL is visible.

use axum::{
    extract::State,
    http::header::{AUTHORIZATION, COOKIE, SET_COOKIE},
    http::HeaderMap,
    http::Uri,
    response::{Html, IntoResponse, Redirect, Response},
    routing::any,
    Router,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

struct Inner {
    pages: HashMap<String, String>,
    hits: Mutex<Vec<String>>,
    seen: Mutex<Vec<Seen>>,
}

/// One request as the site saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub cookie: Option<String>,
    pub authorization: Option<String>,
    pub body: String,
}

#[derive(Clone)]
pub struct FakeWeb {
    pub base: String,
    inner: Arc<Inner>,
}

impl FakeWeb {
    pub async fn start(pages: &[(&str, &str)]) -> Self {
        let inner = Arc::new(Inner { pages: pages.iter().map(|(p, h)| (p.to_string(), h.to_string())).collect(), seen: Mutex::new(vec![]), hits: Mutex::new(vec![]) });
        let app = Router::new().fallback(any(page)).with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeWeb { base, inner }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Every request, with the headers a credential travels in.
    pub fn seen(&self) -> Vec<Seen> {
        self.inner.seen.lock().unwrap().clone()
    }
    /// Every path-and-query requested, in order.
    pub fn hits(&self) -> Vec<String> {
        self.inner.hits.lock().unwrap().clone()
    }
}

/// A page whose text is `redirect:<url>` redirects there, and one that starts
/// `cookie:<value>|` sets that cookie.
async fn page(State(inner): State<Arc<Inner>>, uri: Uri, headers: HeaderMap, body: String) -> Response {
    let path = uri.path_and_query().map(|p| p.to_string()).unwrap_or_default();
    let header = |name| headers.get(name).and_then(|v: &axum::http::HeaderValue| v.to_str().ok()).map(String::from);
    inner.seen.lock().unwrap().push(Seen { path: path.clone(), cookie: header(COOKIE), authorization: header(AUTHORIZATION), body });
    inner.hits.lock().unwrap().push(path);
    let text = inner.pages.get(uri.path()).cloned().unwrap_or_else(|| "<html>not found</html>".into());
    if let Some((cookie, html)) = text.strip_prefix("cookie:").and_then(|t| t.split_once('|')) {
        return ([(SET_COOKIE, cookie.to_string())], Html(html.to_string())).into_response();
    }
    match text.strip_prefix("redirect:") {
        Some(to) => Redirect::temporary(to).into_response(),
        None => Html(text).into_response(),
    }
}
