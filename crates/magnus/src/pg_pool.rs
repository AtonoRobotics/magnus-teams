//! Lightweight bounded Postgres connection pool (zero extra crates).
//!
//! Replaces single-connection `tokio::sync::Mutex<Option<Client>>` bottlenecks
//! in `Recall`, `Accounts`, and `knowledge_index::Index` so concurrent users
//! never head-of-line block behind a single connection.

use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_postgres::Client;

/// A bounded pool of `tokio_postgres::Client` connections to one database URL.
pub(crate) struct PgPool {
    url: String,
    idle: Arc<Mutex<Vec<Client>>>,
    permits: Arc<Semaphore>,
}

/// A checked-out connection that returns to the pool's idle list on drop
/// unless it has closed.
pub(crate) struct PooledClient {
    client: ManuallyDrop<Client>,
    idle: Arc<Mutex<Vec<Client>>>,
    _permit: OwnedSemaphorePermit,
}

impl Deref for PooledClient {
    type Target = Client;
    fn deref(&self) -> &Client {
        &self.client
    }
}

impl PooledClient {
    pub(crate) fn as_mut(&mut self) -> Option<&mut Client> {
        Some(&mut self.client)
    }
}

impl DerefMut for PooledClient {
    fn deref_mut(&mut self) -> &mut Client {
        &mut self.client
    }
}

impl Drop for PooledClient {
    fn drop(&mut self) {
        // SAFETY: `self.client` is only taken once, here in `Drop`.
        let client = unsafe { ManuallyDrop::take(&mut self.client) };
        if !client.is_closed() {
            self.idle.lock().unwrap_or_else(|e| e.into_inner()).push(client);
        }
    }
}

impl PgPool {
    pub(crate) fn new(url: impl Into<String>, max_conns: usize) -> PgPool {
        let cap = max_conns.max(1);
        PgPool { url: url.into(), idle: Arc::new(Mutex::new(Vec::with_capacity(cap))), permits: Arc::new(Semaphore::new(cap)) }
    }

    pub(crate) fn with_initial(url: impl Into<String>, max_conns: usize, initial: Client) -> PgPool {
        let pool = PgPool::new(url, max_conns);
        pool.idle.lock().unwrap_or_else(|e| e.into_inner()).push(initial);
        pool
    }

    /// Borrow an open connection from the pool, opening a new one when none is idle.
    pub(crate) async fn get(&self) -> Result<PooledClient> {
        let permit = self.permits.clone().acquire_owned().await.context("pg pool closed")?;
        let existing = {
            let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
            let mut found = None;
            while let Some(c) = idle.pop() {
                if !c.is_closed() {
                    found = Some(c);
                    break;
                }
            }
            found
        };
        let client = match existing {
            Some(c) => c,
            None => crate::log::connect(&self.url).await?,
        };
        Ok(PooledClient { client: ManuallyDrop::new(client), idle: self.idle.clone(), _permit: permit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_clamps_minimum_capacity_to_one() {
        let p = PgPool::new("postgres://localhost/db", 0);
        assert_eq!(p.permits.available_permits(), 1);
        let p8 = PgPool::new("postgres://localhost/db", 8);
        assert_eq!(p8.permits.available_permits(), 8);
    }
}
