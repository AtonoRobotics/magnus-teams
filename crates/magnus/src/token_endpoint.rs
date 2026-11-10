//! A request to an authorization server's token endpoint.
//!
//! RFC 6749 §5.1 has the endpoint answer in JSON, but GitHub's
//! (`https://github.com/login/oauth/access_token`) answers a form-encoded
//! body unless the request says it accepts JSON. Every exchange and refresh
//! says so, and a server that follows the RFC is unaffected.

/// A form POST to `endpoint` that asks for a JSON answer.
pub(crate) fn post(client: &reqwest::Client, endpoint: &str, form: &[(&str, String)]) -> reqwest::RequestBuilder {
    client.post(endpoint).header(reqwest::header::ACCEPT, "application/json").form(form)
}

#[cfg(test)]
mod tests {
    use axum::{http::HeaderMap, routing::post, Router};

    /// A token endpoint that answers like GitHub's: form-encoded, unless the
    /// request accepts JSON. The exchange reads the JSON answer.
    #[tokio::test]
    async fn a_github_style_token_endpoint_answers_in_json() {
        let app = Router::new().route(
            "/login/oauth/access_token",
            post(|h: HeaderMap| async move {
                let json = h.get("accept").and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"));
                if json {
                    ([("content-type", "application/json")], r#"{"access_token":"gho_x","token_type":"bearer","scope":"repo"}"#.to_string())
                } else {
                    ([("content-type", "application/x-www-form-urlencoded")], "access_token=gho_x&scope=repo&token_type=bearer".to_string())
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/login/oauth/access_token", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await });
        let form = [("grant_type", "authorization_code".to_string()), ("code", "c".to_string())];
        let v: serde_json::Value = super::post(&reqwest::Client::new(), &url, &form).send().await.unwrap().json().await.unwrap();
        assert_eq!(v["access_token"], "gho_x");
    }
}
