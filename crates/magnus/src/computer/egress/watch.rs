//! The egress audit table read back for the developer view: what a computer
//! reached, allowed or blocked, newest last, polled by the last id seen.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio_postgres::Client;

/// The most rows one read returns.
pub const MAX_ROWS: i64 = 500;

/// One request through the proxy, as a developer sees it.
#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    pub id: i64,
    pub at: DateTime<Utc>,
    /// `intent` (allowed, about to be sent), `result` (what came back) or
    /// `refused` (blocked, or waiting on an ask).
    pub phase: String,
    /// `allowed`, `blocked` or `result`, so the view need not know the phases.
    pub verdict: &'static str,
    pub host: String,
    pub port: u16,
    pub method: String,
    pub path: String,
    pub address: Option<String>,
    pub detail: Option<String>,
}

fn verdict(phase: &str) -> &'static str {
    match phase {
        "intent" => "allowed",
        "refused" => "blocked",
        _ => "result",
    }
}

/// A query string can carry a secret: someone else's computer shows the path only.
pub fn shown_path(path: &str, own: bool) -> String {
    if own {
        path.to_string()
    } else {
        path.split('?').next().unwrap_or_default().to_string()
    }
}

/// One connection, kept, for a view that polls every few seconds.
static READER: tokio::sync::Mutex<Option<(String, Client)>> = tokio::sync::Mutex::const_new(None);

/// `computer`'s rows after `after`, oldest first, at most `limit`
/// (capped at [`MAX_ROWS`]).
pub async fn since(url: &str, computer: &str, after: i64, limit: i64, own: bool) -> Result<Vec<Entry>> {
    let mut slot = READER.lock().await;
    if slot.as_ref().is_none_or(|(u, c)| u != url || c.is_closed()) {
        *slot = Some((url.to_string(), crate::log::connect(url).await?));
    }
    let (_, client) = slot.as_ref().context("the egress reader's connection")?;
    let rows = client
        .query(
            "SELECT id, at, phase, host, port, method, path, address, detail FROM public.computer_egress WHERE computer = $1 AND id > $2 ORDER BY id LIMIT $3",
            &[&computer, &after, &limit.clamp(1, MAX_ROWS)],
        )
        .await
        .context("read the egress audit")?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let id: i64 = r.get(0);
            let phase: String = r.get(2);
            let port: i32 = r.get(4);
            let path: String = r.get(6);
            // A port no proxy could have used is a damaged row: shown as
            // port 0 it would be a request that never happened.
            let Ok(port) = u16::try_from(port) else {
                tracing::warn!("egress audit row {id} of {computer} has port {port}, out of range; not shown");
                return None;
            };
            Some(Entry {
                id,
                at: r.get(1),
                verdict: verdict(&phase),
                phase,
                host: r.get(3),
                port,
                method: r.get(5),
                path: shown_path(&path, own),
                address: r.get(7),
                detail: r.get(8),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::audit::{migrate_sql, Audit, Row};
    use super::*;

    #[test]
    fn someone_elses_query_string_is_not_shown() {
        assert_eq!(shown_path("/a/b?token=x", false), "/a/b");
        assert_eq!(shown_path("/a/b?token=x", true), "/a/b?token=x");
        assert_eq!(shown_path("/a", false), "/a");
    }

    #[tokio::test]
    async fn a_developer_reads_one_computers_egress_in_order_after_the_last_seen() {
        let server = std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into());
        let name = format!("eg_{}", uuid::Uuid::new_v4().simple());
        let admin = crate::log::connect(&server).await.unwrap();
        admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
        let mut u = url::Url::parse(&server).unwrap();
        u.set_path(&format!("/{name}"));
        let url = u.to_string();
        crate::log::connect(&url).await.unwrap().batch_execute(&migrate_sql("postgres")).await.unwrap();
        let audit = Audit::new(&url);
        let row = |computer, phase, host, path| Row { computer, phase, host, port: 443, method: "GET", path, address: Some("1.2.3.4:443"), detail: "why" };
        let first = audit.write(row("a", "intent", "pypi.org", "/simple?k=1")).await.unwrap();
        audit.write(row("b", "intent", "other.example", "/")).await.unwrap();
        audit.write(row("a", "result", "pypi.org", "/simple?k=1")).await.unwrap();
        audit.write(row("a", "refused", "evil.example", "/x")).await.unwrap();

        let all = since(&url, "a", 0, 100, false).await.unwrap();
        let seen: Vec<(&str, &str, &str)> = all.iter().map(|e| (e.host.as_str(), e.verdict, e.path.as_str())).collect();
        assert_eq!(seen, [("pypi.org", "allowed", "/simple"), ("pypi.org", "result", "/simple"), ("evil.example", "blocked", "/x")]);
        let newer = since(&url, "a", first, 100, true).await.unwrap();
        assert_eq!(newer.len(), 2, "rows at or before the last seen came back");
        assert_eq!(newer[0].path, "/simple?k=1");
        assert_eq!(since(&url, "a", 0, 1, true).await.unwrap().len(), 1, "the limit was ignored");
        // A damaged row with a port no proxy could use is left out, not shown as port 0.
        let db = crate::log::connect(&url).await.unwrap();
        db.batch_execute(
            "INSERT INTO public.computer_egress (computer, phase, host, port, method, path) VALUES ('c', 'intent', 'bad.example', 70000, 'GET', '/'), ('c', 'intent', 'good.example', 443, 'GET', '/')",
        )
        .await
        .unwrap();
        let c = since(&url, "c", 0, 100, true).await.unwrap();
        let hosts: Vec<(&str, u16)> = c.iter().map(|e| (e.host.as_str(), e.port)).collect();
        assert_eq!(hosts, [("good.example", 443)], "a row with an impossible port was shown");
        drop(db);
        drop(audit);
        *READER.lock().await = None;
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await.unwrap();
    }
}
