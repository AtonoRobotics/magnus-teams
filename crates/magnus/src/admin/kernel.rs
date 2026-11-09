//! The console's way to the kernel: HTTP over the kernel's control socket
//! (`crate::control`), a local Unix socket, never the network.

use anyhow::{anyhow, Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use serde_json::Value;
use std::path::PathBuf;

pub struct Kernel {
    pub path: PathBuf,
}

/// The kernel refused a call: its status and why, for the console to pass on.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub message: String,
    /// What the kernel said beyond the error's name, when it said more.
    pub detail: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

impl Kernel {
    pub async fn call(&self, method: hyper::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let stream = tokio::net::UnixStream::connect(&self.path).await.with_context(|| format!("reach the kernel at {}", self.path.display()))?;
        let (mut send, conn) = hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let req = hyper::Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, "kernel")
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body.map(|b| b.to_string()).unwrap_or_default())))?;
        let res = send.send_request(req).await?;
        let status = res.status();
        let bytes = res.into_body().collect().await?.to_bytes();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status.is_client_error() {
            return Err(Refused {
                status: status.as_u16(),
                message: v["error"].as_str().unwrap_or("refused").to_string(),
                detail: v["message"].as_str().unwrap_or_default().to_string(),
            }
            .into());
        }
        if !status.is_success() {
            return Err(anyhow!("the kernel answered {status}: {}", v["error"].as_str().unwrap_or_default()));
        }
        Ok(v)
    }

    /// Write one console action to the log, through the kernel.
    pub async fn audit(&self, entry: &Value) -> Result<i64> {
        let v = self.call(hyper::Method::POST, "/v1/audit", Some(entry)).await?;
        v["seq"].as_i64().ok_or_else(|| anyhow!("the kernel did not say where it wrote the entry"))
    }
}
