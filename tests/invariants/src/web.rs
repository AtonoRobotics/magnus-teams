//! A fake website. Pages are untrusted content; every hit is recorded with its
//! full path and query, so an exfiltration attempt by URL is visible.

use axum::{extract::State, http::Uri, response::Html, routing::get, Router};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

struct Inner {
    pages: HashMap<String, String>,
    hits: Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct FakeWeb {
    pub base: String,
    inner: Arc<Inner>,
}

impl FakeWeb {
    pub async fn start(pages: &[(&str, &str)]) -> Self {
        let inner = Arc::new(Inner { pages: pages.iter().map(|(p, h)| (p.to_string(), h.to_string())).collect(), hits: Mutex::new(vec![]) });
        let app = Router::new().fallback(get(page)).with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeWeb { base, inner }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Every path-and-query requested, in order.
    pub fn hits(&self) -> Vec<String> {
        self.inner.hits.lock().unwrap().clone()
    }
}

async fn page(State(inner): State<Arc<Inner>>, uri: Uri) -> Html<String> {
    inner.hits.lock().unwrap().push(uri.path_and_query().map(|p| p.to_string()).unwrap_or_default());
    Html(inner.pages.get(uri.path()).cloned().unwrap_or_else(|| "<html>not found</html>".into()))
}
