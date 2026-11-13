//! Web access for the agent: fetch and search, run by the kernel behind the
//! boundary. Everything returned is labeled untrusted.

use anyhow::{anyhow, Result};
use std::net::IpAddr;
use std::time::Duration;

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_TEXT: usize = 60_000;
/// A fetch: the connection, then the whole request.
const CONNECT_WAIT: Duration = Duration::from_secs(10);
const FETCH_WAIT: Duration = Duration::from_secs(45);

pub struct Web {
    http: reqwest::Client,
    allow_private: bool,
    search_url: Option<String>,
}

pub(crate) fn private_ip(ip: IpAddr) -> bool {
    // An IPv4 address written as IPv6 (`::ffff:a.b.c.d`) is reached as that
    // IPv4 address, so it is judged as one (#196).
    if let IpAddr::V6(v) = ip {
        if let Some(v4) = v.to_ipv4_mapped() {
            return private_ip(IpAddr::V4(v4));
        }
    }
    match ip {
        IpAddr::V4(v) => {
            v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.octets()[0] == 100 && (64..128).contains(&v.octets()[1])
        }
        IpAddr::V6(v) => v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
    }
}

impl Web {
    pub fn new(cfg: &crate::config::Web) -> Result<Web> {
        Ok(Web {
            http: reqwest::Client::builder()
                .connect_timeout(CONNECT_WAIT)
                .timeout(FETCH_WAIT)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Mozilla/5.0 (compatible; Magnus/0.1)")
                .build()?,
            allow_private: cfg.allow_private_networks,
            search_url: cfg.search_url.clone(),
        })
    }

    pub fn can_search(&self) -> bool {
        self.search_url.is_some()
    }

    /// Check a URL's host and return the address to connect to, so the
    /// connection goes to the address that was checked (no DNS rebinding).
    async fn check_host(&self, url: &url::Url) -> Result<Option<(String, std::net::SocketAddr)>> {
        anyhow::ensure!(matches!(url.scheme(), "http" | "https"), "only http and https URLs can be fetched");
        if self.allow_private {
            return Ok(None);
        }
        let host = url.host_str().ok_or_else(|| anyhow!("URL has no host"))?.to_string();
        let port = url.port_or_known_default().unwrap_or(443);
        // An address written in the URL is that address (`host_str` keeps an
        // IPv6 one's brackets, which no resolver takes); a name is resolved.
        let addrs: Vec<std::net::SocketAddr> = match url.host() {
            Some(url::Host::Ipv4(ip)) => vec![(IpAddr::V4(ip), port).into()],
            Some(url::Host::Ipv6(ip)) => vec![(IpAddr::V6(ip), port).into()],
            _ => tokio::net::lookup_host((host.as_str(), port)).await?.collect(),
        };
        anyhow::ensure!(!addrs.is_empty(), "{host} does not resolve");
        for a in &addrs {
            anyhow::ensure!(!private_ip(a.ip()), "{host} resolves to a private address; fetching it is not allowed");
        }
        Ok(Some((host, addrs[0])))
    }

    /// GET with every redirect hop checked like the first request.
    async fn get(&self, start: url::Url) -> Result<reqwest::Response> {
        let mut url = start;
        for _ in 0..6 {
            let pinned = self.check_host(&url).await?;
            let client = match &pinned {
                Some((host, addr)) => reqwest::Client::builder()
                    .connect_timeout(CONNECT_WAIT)
                    .timeout(FETCH_WAIT)
                    .redirect(reqwest::redirect::Policy::none())
                    .user_agent("Mozilla/5.0 (compatible; Magnus/0.1)")
                    .resolve(host, *addr)
                    .build()?,
                None => self.http.clone(),
            };
            let resp = client.get(url.clone()).send().await.map_err(|e| anyhow!("fetch failed: {e}"))?;
            if resp.status().is_redirection() {
                let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).ok_or_else(|| anyhow!("redirect without a location"))?;
                url = url.join(loc).map_err(|e| anyhow!("bad redirect: {e}"))?;
                continue;
            }
            return Ok(resp);
        }
        Err(anyhow!("too many redirects"))
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
        let v: serde_json::Value = self.http.get(url).send().await?.json().await?; // the owner's own search service
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
