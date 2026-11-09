//! The egress audit table: one row per request, keyed by computer, beside the
//! log. An allowed request has its row before it has a packet.

use anyhow::{Context, Result};
use tokio_postgres::Client;

pub fn migrate_sql(role: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS public.computer_egress (
           id bigserial PRIMARY KEY,
           computer text NOT NULL,
           at timestamptz NOT NULL DEFAULT now(),
           phase text NOT NULL,
           host text NOT NULL,
           port integer NOT NULL,
           method text NOT NULL,
           path text NOT NULL,
           address text,
           detail text
         );
         CREATE INDEX IF NOT EXISTS computer_egress_computer ON public.computer_egress (computer, id);
         GRANT SELECT, INSERT ON public.computer_egress TO \"{role}\";
         GRANT USAGE ON SEQUENCE public.computer_egress_id_seq TO \"{role}\";"
    )
}

pub struct Row<'a> {
    pub computer: &'a str,
    pub phase: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub method: &'a str,
    pub path: &'a str,
    pub address: Option<&'a str>,
    pub detail: &'a str,
}

pub struct Audit {
    url: String,
    client: tokio::sync::Mutex<Option<Client>>,
}

impl Audit {
    pub fn new(url: &str) -> Audit {
        Audit { url: url.to_string(), client: tokio::sync::Mutex::new(None) }
    }

    /// The row's id, or an error, in which case the caller sends nothing.
    pub async fn write(&self, r: Row<'_>) -> Result<i64> {
        let mut slot = self.client.lock().await;
        if slot.as_ref().is_none_or(|c| c.is_closed()) {
            *slot = Some(crate::log::connect(&self.url).await?);
        }
        let client = slot.as_ref().context("the audit connection")?;
        let row = client
            .query_one(
                "INSERT INTO public.computer_egress (computer, phase, host, port, method, path, address, detail) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
                &[&r.computer, &r.phase, &r.host, &i32::from(r.port), &r.method, &r.path, &r.address, &r.detail],
            )
            .await
            .context("write the egress audit row")?;
        Ok(row.get(0))
    }
}
