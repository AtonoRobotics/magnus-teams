//! A fresh Postgres database per test, on the server named by `MAGNUS_TEST_PG`,
//! dropped with its roles when the test ends.

use tokio_postgres::{Client, NoTls};

pub fn server_url() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

pub struct TestDb {
    pub name: String,
    pub kernel_role: String,
    pub kernel_password: String,
}

impl TestDb {
    pub async fn create() -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!("t_{id}");
        let c = connect(&server_url()).await;
        c.batch_execute(&format!("CREATE DATABASE {name}")).await.expect("create test database");
        TestDb { name, kernel_role: format!("k_{id}"), kernel_password: uuid::Uuid::new_v4().simple().to_string() }
    }

    fn with(&self, user: Option<(&str, &str)>) -> String {
        let mut u = url::Url::parse(&server_url()).expect("MAGNUS_TEST_PG is a URL");
        u.set_path(&format!("/{}", self.name));
        if let Some((name, pw)) = user {
            u.set_username(name).unwrap();
            u.set_password(Some(pw)).unwrap();
        }
        u.to_string()
    }

    /// Superuser on this test database.
    pub fn admin_url(&self) -> String {
        self.with(None)
    }

    /// The role the kernel runs as. `magnus migrate` creates it.
    pub fn kernel_url(&self) -> String {
        self.with(Some((&self.kernel_role, &self.kernel_password)))
    }

    pub async fn admin(&self) -> Client {
        connect(&self.admin_url()).await
    }

    pub async fn as_kernel(&self) -> Client {
        connect(&self.kernel_url()).await
    }

    /// Every user table outside the system schemas, as (schema, table).
    pub async fn tables(&self) -> Vec<(String, String)> {
        let c = self.admin().await;
        c.query(
            "SELECT table_schema, table_name FROM information_schema.tables
             WHERE table_type = 'BASE TABLE'
               AND table_schema NOT IN ('pg_catalog', 'information_schema')
             ORDER BY 1, 2",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
    }

    /// Every row of every user table, as text.
    pub async fn dump(&self) -> String {
        let c = self.admin().await;
        let mut out = String::new();
        for (s, t) in self.tables().await {
            let rows = c.query(&format!("SELECT row_to_json(x)::text FROM \"{s}\".\"{t}\" x"), &[]).await.unwrap();
            for r in rows {
                out.push_str(&format!("{s}.{t}: {}\n", r.get::<_, String>(0)));
            }
        }
        out
    }
}

/// The server is shared and outlives every test, so the test's database and
/// the roles its kernel made go when the test ends, whether it passed or not.
/// The roles are the kernel's own and any named after it (`{kernel_role}_...`).
impl Drop for TestDb {
    fn drop(&mut self) {
        let (name, role) = (self.name.clone(), self.kernel_role.clone());
        // Drop runs inside the test's runtime, which cannot be blocked on; the
        // cleanup gets a thread and runtime of its own.
        let dropped = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let c = connect(&server_url()).await;
                // FORCE: the appliance may still be connected; its process is stopped after this.
                c.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await?;
                let named_after = format!("{}\\_%", role.replace('_', "\\_"));
                let roles = c.query("SELECT rolname FROM pg_roles WHERE rolname = $1 OR rolname LIKE $2", &[&role, &named_after]).await?;
                for r in roles {
                    c.batch_execute(&format!("DROP ROLE IF EXISTS {}", r.get::<_, String>(0))).await?;
                }
                Ok::<_, tokio_postgres::Error>(())
            })
        })
        .join();
        match dropped {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("could not drop test database {}: {e}", self.name),
            Err(_) => eprintln!("could not drop test database {}", self.name),
        }
    }
}

pub async fn connect(url: &str) -> Client {
    let (client, conn) = tokio_postgres::connect(url, NoTls).await.unwrap_or_else(|e| panic!("connect to {url}: {e}"));
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client
}
