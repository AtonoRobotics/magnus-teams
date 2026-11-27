//! Web access for the agent: fetch and search, run by the kernel behind the
//! boundary. Everything returned is labeled untrusted.

use crate::egress_client::Egress;
use anyhow::{anyhow, Result};
use std::net::IpAddr;
use std::time::Duration;

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_TEXT: usize = 60_000;
/// A fetch: the connection, then the whole request.
const CONNECT_WAIT_SECS: Duration = Duration::from_secs(10);
const FETCH_WAIT_SECS: Duration = Duration::from_secs(45);

pub struct Web {
    /// For the pages the agent asks for: every hop checked (`egress_client`).
    egress: Egress,
    /// For the owner's own search service, at the address the config names.
    search: reqwest::Client,
    search_url: Option<String>,
}

const USER_AGENT: &str = "Mozilla/5.0 (compatible; Magnus/0.1)";

pub(crate) fn private_ip(ip: IpAddr) -> bool {
    crate::net_guard::is_non_public_ip(ip)
}

impl Web {
    pub fn new(cfg: &crate::config::Web) -> Result<Web> {
        Ok(Web {
            egress: Egress::new(cfg.allow_private_networks, CONNECT_WAIT_SECS, FETCH_WAIT_SECS),
            search: crate::egress_client::configured(CONNECT_WAIT_SECS, FETCH_WAIT_SECS)?,
            search_url: cfg.search_url.clone(),
        })
    }

    pub fn can_search(&self) -> bool {
        self.search_url.is_some()
    }

    /// GET with every redirect hop checked like the first request.
    async fn get(&self, start: url::Url) -> Result<reqwest::Response> {
        anyhow::ensure!(matches!(start.scheme(), "http" | "https"), "only http and https URLs can be fetched");
        self.egress.get(start.as_str(), |r| r.header(reqwest::header::USER_AGENT, USER_AGENT)).await.map_err(|e| anyhow!("fetch failed: {e:#}"))
    }

    /// Returns (text, content type).
    pub async fn fetch(&self, raw: &str) -> Result<(String, String)> {
        let url = url::Url::parse(raw).map_err(|e| anyhow!("not a URL: {e}"))?;
        let resp = self.get(url.clone()).await?;
        let status = resp.status();
        let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        let mut bytes = Vec::new();
        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| anyhow!("download failed: {e}"))?;
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_BYTES {
                break;
            }
        }
        let mut text = if ctype.contains("html") {
            html2text::from_read(&bytes[..], 100).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned())
        } else if ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml") {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            format!("[{} bytes of {ctype}; not shown as text]", bytes.len())
        };
        if text.len() > MAX_TEXT {
            let mut cut = MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n[truncated]");
        }
        Ok((format!("HTTP {status} {url}\n\n{text}"), ctype))
    }

    pub async fn search(&self, query: &str) -> Result<String> {
        let base = self.search_url.as_ref().ok_or_else(|| anyhow!("no search service is configured"))?;
        let mut url = url::Url::parse(base)?;
        url.query_pairs_mut().append_pair("q", query).append_pair("format", "json");
        let v: serde_json::Value = self.search.get(url).header(reqwest::header::USER_AGENT, USER_AGENT).send().await?.json().await?; // the owner's own search service
        let mut out = String::new();
        for r in v["results"].as_array().into_iter().flatten().take(10) {
            out.push_str(&format!("- {}\n  {}\n  {}\n", r["title"].as_str().unwrap_or_default(), r["url"].as_str().unwrap_or_default(), r["content"].as_str().unwrap_or_default()));
        }
        if out.is_empty() {
            out = "no results".into();
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server on this machine's loopback that answers "ok".
    async fn local() -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut s, _)) = l.accept().await {
                let mut b = [0u8; 2048];
                let _ = s.read(&mut b).await;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
            }
        });
        port
    }

    fn web(allow_private_networks: bool) -> Web {
        Web::new(&crate::config::Web { allow_private_networks, search_url: None }).unwrap()
    }

    /// An IPv4 address written as IPv6 (`::ffff:a.b.c.d`) is that IPv4
    /// address: loopback, private and the metadata service stay out of
    /// reach however they are written (#196).
    #[tokio::test]
    async fn a_mapped_ipv4_address_is_refused_like_the_address_it_maps() {
        let port = local().await;
        let url = format!("http://[::ffff:127.0.0.1]:{port}/");
        let refused = web(false).fetch(&url).await.expect_err("the mapped loopback address was fetched");
        assert!(refused.to_string().contains("private address"), "{refused}");
        let (text, _) = web(true).fetch(&url).await.expect("a test server allows private networks");
        assert!(text.ends_with("ok"), "{text}");
        for a in ["::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:169.254.169.254", "::ffff:192.168.1.1", "::ffff:100.64.0.1"] {
            assert!(private_ip(a.parse().unwrap()), "{a}");
        }
        assert!(!private_ip("::ffff:93.184.216.34".parse().unwrap()), "a public IPv4 address, mapped");
    }
}
