//! A fresh Postgres database and kernel role per live memory test, on the
//! server `MAGNUS_TEST_PG` names (`ci/on-strix-beta.sh` sets it). The kernel
//! role is NOSUPERUSER NOBYPASSRLS, so row security is exercised as in
//! production. Without a reachable server the test fails rather than passing
//! silently. Both are dropped when the test ends.

use crate::memory::schema::migrate_if_enabled;
use crate::memory::store::MemoryStore;
use crate::memory_index::{HnswIterativeScan, MemoryConfig, MemoryMode};

pub(crate) const TEST_EMBEDDING_DIM: usize = 4;
const TEST_KERNEL_PASSWORD: &str = "memtestpw";

pub(crate) fn test_config() -> MemoryConfig {
    MemoryConfig { mode: MemoryMode::V2, embedding_dim: TEST_EMBEDDING_DIM, hnsw_iterative_scan: HnswIterativeScan::RelaxedOrder, ..Default::default() }
}

fn server_url() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

pub(crate) struct LiveDb {
    pub admin_url: String,
    pub kernel_url: String,
    name: String,
}

impl LiveDb {
    /// Create the database and kernel role, and migrate with `test_config()`.
    pub(crate) async fn create() -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("mem_{id}");
        let server = crate::log::connect(&server_url()).await.expect("connect to the test Postgres (MAGNUS_TEST_PG)");
        server.batch_execute(&format!("CREATE DATABASE {name}")).await.expect("create test database");
        server.batch_execute(&format!("CREATE ROLE {name} LOGIN PASSWORD '{TEST_KERNEL_PASSWORD}' NOSUPERUSER NOBYPASSRLS")).await.expect("create test kernel role");

        let mut admin = url::Url::parse(&server_url()).expect("valid MAGNUS_TEST_PG");
        admin.set_path(&format!("/{name}"));
        let mut kernel = admin.clone();
        kernel.set_username(&name).expect("set kernel user");
        kernel.set_password(Some(TEST_KERNEL_PASSWORD)).expect("set kernel password");

        let db = Self { admin_url: admin.to_string(), kernel_url: kernel.to_string(), name };
        migrate_if_enabled(&db.admin_url, &db.kernel_url, &test_config()).await.expect("migrate unified memory schema");
        db
    }

    pub(crate) async fn store(&self) -> MemoryStore {
        self.store_with(&test_config()).await
    }

    pub(crate) async fn store_with(&self, cfg: &MemoryConfig) -> MemoryStore {
        MemoryStore::connect(&self.kernel_url, cfg).await.expect("connect MemoryStore as the kernel role")
    }
}

impl Drop for LiveDb {
    fn drop(&mut self) {
        let name = self.name.clone();
        // Drop runs inside the test's runtime, which cannot be blocked on;
        // the cleanup gets a thread and runtime of its own.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("cleanup runtime");
            rt.block_on(async move {
                let server = crate::log::connect(&server_url()).await.expect("connect to the test Postgres");
                server.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await.expect("drop test database");
                server.batch_execute(&format!("DROP ROLE IF EXISTS {name}_reader; DROP ROLE IF EXISTS {name}")).await.expect("drop test roles");
            })
        })
        .join();
    }
}
