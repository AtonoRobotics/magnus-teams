//! Backups, scrubs and restores through the one list of stores that hold a
//! person, against a kernel booted on a fresh database; each restore goes
//! into a database and state directory of its own, as `magnus restore`
//! does.

use super::*;
use crate::admin_app::tests::{server_with, Server, PASSWORD};

fn pg() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

/// A backup restored into an empty database, with a kernel role of its
/// own, and an empty state directory.
struct Restored {
    db: tokio_postgres::Client,
    state: tempfile::TempDir,
    name: String,
    role: String,
}

async fn restore_into(s: &Server, from: &Path) -> Restored {
    let name = format!("r_{}", uuid::Uuid::new_v4().simple());
    let role = format!("k_{}", uuid::Uuid::new_v4().simple());
    let admin = crate::log::connect(&pg()).await.unwrap();
    admin.batch_execute(&format!("CREATE DATABASE {name}")).await.unwrap();
    let mut admin_url = url::Url::parse(&pg()).unwrap();
    admin_url.set_path(&format!("/{name}"));
    let mut kernel_url = admin_url.clone();
    kernel_url.set_username(&role).unwrap();
    kernel_url.set_password(Some("restore-test")).unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut cfg = s.k.cfg.clone();
    cfg.appliance.state_dir = state.path().join("state");
    cfg.database.url = kernel_url.to_string();
    cfg.backup.dir = Some(s.k.cfg.backup_dir());
    cfg.backup.key_file = Some(s.k.cfg.backup_key_file());
    crate::backup::restore(&cfg, admin_url.as_str(), from).await.unwrap();
    let db = crate::log::connect(admin_url.as_str()).await.unwrap();
    Restored { db, state, name, role }
}

impl Restored {
    /// Whose rows each accounts table holds.
    async fn people(&self, table: &str) -> Vec<String> {
        self.db.query(&format!("SELECT person FROM public.{table} ORDER BY person"), &[]).await.unwrap().iter().map(|r| r.get(0)).collect()
    }

    fn vault(&self) -> Vec<String> {
        let all: BTreeMap<String, Value> = serde_json::from_slice(&std::fs::read(self.state.path().join("state/vault/secrets.json")).unwrap()).unwrap();
        all.into_keys().collect()
    }

    fn devices(&self) -> Vec<String> {
        crate::devices::Devices::open(&self.state.path().join("state")).unwrap().list(None).into_iter().map(|d| d.person).collect()
    }

    async fn done(self) {
        drop(self.db);
        let admin = crate::log::connect(&pg()).await.unwrap();
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)).await.unwrap();
        let _ = admin.batch_execute(&format!("DROP ROLE IF EXISTS {}", self.role)).await;
    }
}

fn fingerprint(person: &str) -> String {
    format!("{:0>64}", hex::encode(person))
}

/// A computer paired for `person`, as the registry keeps it.
fn pair(s: &Server, person: &str) {
    let path = crate::devices::file(&s.k.cfg.appliance.state_dir);
    let mut all: BTreeMap<String, Value> = std::fs::read(&path).ok().map(|b| serde_json::from_slice(&b).unwrap()).unwrap_or_default();
    let id = format!("dev-{person}");
    let d = crate::devices::Device {
        id: id.clone(),
        person: person.into(),
        name: format!("{person}'s laptop"),
        cloudflare_id: format!("cf-{person}"),
        fingerprint: fingerprint(person),
        cloudflare_fingerprint: String::new(),
        serial: String::new(),
        created: Utc::now(),
        revoked: None,
    };
    all.insert(id, serde_json::to_value(d).unwrap());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&all).unwrap()).unwrap();
}

async fn back_up(s: &Server) -> PathBuf {
    let made = s.k.back_up("person:sam").await.unwrap();
    assert_eq!(made["restored"], true, "{made}");
    s.k.cfg.backup_dir().join(made["file"].as_str().unwrap())
}

/// Where a backup still holds anything of `person`: their accounts rows,
/// their vault entries, their devices.
fn holds(v: &Value, person: &str) -> Vec<String> {
    let mut out = vec![];
    for (t, rows) in account_tables(&v["accounts"]).unwrap() {
        if rows.iter().any(|r| r["person"] == person) {
            out.push(t);
        }
    }
    let vault: BTreeMap<String, Value> = serde_json::from_str(v["vault"]["secrets"].as_str().unwrap()).unwrap();
    out.extend(vault.into_keys().filter(|n| owner_of(n) == Some(person)));
    if v["devices"].as_object().unwrap().values().any(|d| d["person"] == person) {
        out.push("devices".into());
    }
    out
}

/// Erased, a person leaves every backup kept and every restore: a restore
/// of a backup from before the erasure brings back neither their login
/// (password, authenticator, email, role) nor their card, their sign-ins to
/// apps, their apps' tokens or their paired computers, whether or not the
/// backup could be rewritten.
#[tokio::test(flavor = "multi_thread")]
async fn a_backup_from_before_an_erasure_brings_back_nothing_of_the_erased_person() {
    let s = server_with("", |_| axum::Router::new()).await;
    s.k.accounts.add_person("vi", "Vi", Some("vi@example.com")).await.unwrap();
    s.k.accounts.set_password("vi", PASSWORD).await.unwrap();
    s.k.accounts.enroll_totp("vi").await.unwrap();
    let mut own = crate::catalog::connector(&crate::catalog::manifests()[0]);
    own.name = "vi_tool_1a2b3c4d".into();
    own.owner = Some("vi".into());
    own.credential = Some("connector_vi_tool_1a2b3c4d".into());
    s.k.emit("person:vi", crate::events::Ev::ConnectorAdded { connector: own }).await.unwrap();
    for n in ["payments/card/vi", "payments/link/vi", "connector_mail__vi", "connector_vi_tool_1a2b3c4d", "payments/card/sam"] {
        s.k.vault.put(n, "theirs").unwrap();
    }
    pair(&s, "vi");
    pair(&s, "sam");
    let file = back_up(&s).await;
    let before = crate::backup::read(&s.k.cfg, &file).unwrap();
    assert!(holds(&before, "vi").len() >= 7, "the backup did not hold vi: {:?}", holds(&before, "vi"));
    // A copy where no scrub reaches it: a backup that could not be rewritten.
    let aside = tempfile::tempdir().unwrap();
    let copy = aside.path().join(file.file_name().unwrap());
    std::fs::copy(&file, &copy).unwrap();

    let out = s.k.erase_person("person:sam", "vi").await.unwrap();
    assert_eq!(out["erasure"], "complete", "{out}");
    s.k.remove_local_person("vi").await.unwrap();

    // The backup kept was rewritten without them.
    let after = crate::backup::read(&s.k.cfg, &file).unwrap();
    assert_eq!(holds(&after, "vi"), Vec::<String>::new(), "the scrubbed backup still holds vi");
    assert!(holds(&after, "sam").len() >= 3, "the scrub took sam's too: {:?}", holds(&after, "sam"));

    for from in [&file, &copy] {
        let r = restore_into(&s, from).await;
        for t in ["account_people", "account_passwords", "account_totp", "account_statuses", "account_roles"] {
            assert!(!r.people(t).await.contains(&"vi".to_string()), "{t} brought vi back from {}", from.display());
        }
        assert!(r.people("account_passwords").await.contains(&"sam".to_string()), "sam's password did not come back");
        let vault = r.vault();
        assert!(vault.iter().all(|n| owner_of(n) != Some("vi") && n != "connector_vi_tool_1a2b3c4d"), "vi's vault entries came back: {vault:?}");
        assert!(vault.contains(&"payments/card/sam".to_string()), "sam's card did not come back");
        assert_eq!(r.devices(), vec!["sam".to_string()], "the devices came back wrong");
        r.done().await;
    }
    s.done().await;
}

/// A restore never brings a session back: one ended after the backup (a
/// sign-out everywhere) stays ended, and none of the others return either.
#[tokio::test(flavor = "multi_thread")]
async fn a_restore_brings_back_no_session() {
    let s = server_with("", |_| axum::Router::new()).await;
    s.k.accounts.open_session("sam", "browser", None).await.unwrap();
    s.k.accounts.open_session("lee", "desktop", None).await.unwrap();
    let file = back_up(&s).await;
    assert!(crate::backup::read(&s.k.cfg, &file).unwrap()["accounts"]["tables"].get("account_sessions").is_none(), "the backup kept the sessions");
    s.k.accounts.end_all("sam").await.unwrap();
    let r = restore_into(&s, &file).await;
    assert_eq!(r.people("account_sessions").await, Vec::<String>::new(), "a restore brought sessions back");
    assert!(r.people("account_passwords").await.contains(&"sam".to_string()), "the accounts did not come back");
    r.done().await;
    s.done().await;
}

/// The paired computers come back with a restore, so none is refused until
/// it is paired again.
#[tokio::test(flavor = "multi_thread")]
async fn paired_devices_come_back_from_a_backup() {
    let s = server_with("", |_| axum::Router::new()).await;
    pair(&s, "sam");
    pair(&s, "lee");
    let file = back_up(&s).await;
    let r = restore_into(&s, &file).await;
    let mut people = r.devices();
    people.sort();
    assert_eq!(people, vec!["lee".to_string(), "sam".to_string()]);
    let back = crate::devices::Devices::open(&r.state.path().join("state")).unwrap();
    assert!(back.is_live(&fingerprint("sam")), "a paired computer is refused after the restore");
    r.done().await;
    s.done().await;
}

/// A backup made before this release holds the accounts as a database dump:
/// it reads as rows, escapes and empty values as they were, and an erased
/// person is taken out of it.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_backups_accounts_dump_reads_as_rows_and_loses_the_erased() {
    let s = server_with("", |_| axum::Router::new()).await;
    let odd = "Tab\there, back\\slash,\nnew line";
    s.k.accounts.add_person("vi", odd, None).await.unwrap();
    let mut args: Vec<String> = vec!["--format=custom".into(), "--no-owner".into(), "--no-privileges".into()];
    args.extend(crate::accounts::TABLES.iter().map(|t| format!("--table=public.{t}")));
    args.push(format!("--dbname={}", s.k.cfg.database.url));
    let dump = crate::backup::pg_tool("pg_dump", &args.iter().map(String::as_str).collect::<Vec<_>>(), &[], &[]).unwrap();
    let mut doc = json!({ "made_at": Utc::now() - chrono::Duration::minutes(1), "accounts": { "dump": B64.encode(dump) } });
    let tables = account_tables(&doc["accounts"]).unwrap();
    let vi = tables["account_people"].iter().find(|r| r["person"] == "vi").unwrap();
    assert_eq!(vi["name"], odd);
    assert_eq!(vi["email"], Value::Null);
    assert!(tables["account_passwords"].iter().any(|r| r["person"] == "sam"));
    let mut erased = Erased::default();
    erased.person("vi", Utc::now());
    assert!(drop_erased(&mut doc, &erased).unwrap());
    let tables = account_tables(&doc["accounts"]).unwrap();
    assert!(!tables["account_people"].iter().any(|r| r["person"] == "vi"), "vi stayed in the older backup");
    assert!(tables["account_people"].iter().any(|r| r["person"] == "lee"), "lee went with vi");
    s.done().await;
}

/// A person erased before a backup was made, and made again since, keeps
/// what that backup holds of the new one; one erased after it does not.
#[test]
fn an_erasure_reaches_only_backups_made_before_it() {
    let at = Utc::now();
    let mut erased = Erased::default();
    erased.person("vi", at);
    erased.secret("connector_vi_tool_1", at);
    let before = Some(at - chrono::Duration::minutes(1));
    let after = Some(at + chrono::Duration::minutes(1));
    assert!(erased.holds_person("vi", before) && !erased.holds_person("vi", after));
    assert!(erased.holds_secret("payments/card/vi", before) && !erased.holds_secret("payments/card/vi", after));
    assert!(erased.holds_secret("connector_mail__vi", before));
    assert!(erased.holds_secret("connector_vi_tool_1", before) && !erased.holds_secret("connector_vi_tool_1", after));
    assert!(!erased.holds_secret("payments/card/sam", before));
    // The record names no one.
    assert!(!erased.to_json().to_string().contains("vi"));
}
