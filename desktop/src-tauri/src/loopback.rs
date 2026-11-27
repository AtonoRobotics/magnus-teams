//! The loopback address a browser sign-in comes back to (RFC 8252).

use std::collections::HashMap;

/// The query of the first redirect to `/callback` on this listener: it has
/// a code and state, or the sign-in page's error. The browser is told to
/// return to Magnus either way.
pub(crate) async fn redirected(l: tokio::net::TcpListener, wait: std::time::Duration) -> crate::Res<HashMap<String, String>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let wait_for = async {
        loop {
            let (mut s, _) = l.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let mut n = 0;
            while n < buf.len() {
                let k = s.read(&mut buf[n..]).await.map_err(|e| e.to_string())?;
                if k == 0 {
                    break;
                }
                n += k;
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buf[..n]);
            let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default().to_string();
            let Ok(u) = url::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
            // The provider sends the browser here by navigating; a fetch from
            // some page in that browser (Sec-Fetch-Mode cors or no-cors) is
            // not the provider and must not end the wait.
            let scripted =
                head.lines().filter_map(|l| l.split_once(':')).any(|(k, v)| k.trim().eq_ignore_ascii_case("sec-fetch-mode") && !v.trim().eq_ignore_ascii_case("navigate"));
            if u.path() != "/callback" || scripted {
                let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
                continue;
            }
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            let (title, result) = match (q.get("code"), q.get("state"), q.get("error")) {
                (Some(_), Some(_), _) => ("Signed in. You can close this page and return to Magnus.", Ok(q.clone())),
                (_, _, Some(e)) => ("The sign-in did not finish. Return to Magnus.", Err(format!("the sign-in page said: {}", q.get("error_description").unwrap_or(e)))),
                _ => ("The sign-in did not finish. Return to Magnus.", Err("the sign-in page sent no code".to_string())),
            };
            let body = format!("<!doctype html><meta charset=utf-8><title>Magnus</title><p style=\"font:16px system-ui;margin:40px\">{title}</p>");
            let _ = s
                .write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes())
                .await;
            return result;
        }
    };
    tokio::time::timeout(wait, wait_for).await.map_err(|_| "the sign-in took too long; start again".to_string())?
}

#[cfg(test)]
mod tests {
    use super::redirected;
    use tokio::io::AsyncWriteExt;

    async fn hit(port: u16, extra: &str, query: &str) {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET /callback?{query} HTTP/1.1\r\nhost: 127.0.0.1\r\n{extra}\r\n").as_bytes()).await.unwrap();
    }

    /// A page in the browser fetching the callback with an error does not
    /// end the wait; the provider's own redirect, a navigation, still does.
    #[tokio::test]
    async fn a_fetch_from_a_page_does_not_end_the_wait() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let wait = tokio::spawn(redirected(l, std::time::Duration::from_secs(5)));
        hit(port, "sec-fetch-mode: no-cors\r\n", "error=access_denied").await;
        hit(port, "sec-fetch-mode: navigate\r\n", "code=c1&state=s1").await;
        let q = wait.await.unwrap().unwrap();
        assert_eq!(q["code"], "c1");
    }
}
