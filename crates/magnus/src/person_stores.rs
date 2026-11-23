//! The stores that hold something of a person, in one list (#195): the
//! log's content, the accounts tables, the vault and the paired devices.
//! A backup takes each of them; a scrub after an erasure takes the erased
//! person out of each, in every backup kept; a restore puts each back
//! without what was erased after the backup was made. Sessions are never
//! kept: a restore must not bring back a sign-in ended since the backup
//! (signed out everywhere, a password reset, a person turned off).
//!
//! What was erased is recorded beside the state and beside the backups:
//! events by their number and time, people and vault entries by a hash of
//! their id or name, so the record holds no content and names no one.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A store that holds something of a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    /// The content of the log's events: messages, files, memories.
    LogContent,
    /// The accounts tables: name, email, password, authenticator, status,
    /// role and invitations.
    Accounts,
    /// The vault: their sign-ins to apps, their card and Link wallet, the
    /// tokens of apps they built.
    Vault,
    /// The computers paired with the server.
    Devices,
}

/// Every store that holds something of a person. Backup, scrub and restore
/// each go through this list.
pub const STORES: [Store; 4] = [Store::LogContent, Store::Accounts, Store::Vault, Store::Devices];

/// Accounts tables never backed up or restored: a restore must not bring
/// back a sign-in that ended after the backup.
pub const NEVER_KEPT: &[&str] = &["account_sessions"];

/// The record of erasures, in the state directory and beside the backups.
const RECORD: &str = "removals.json";

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// What has been erased: events whose content was removed, by number and
/// time; people and vault entries by the hash of their id or name, each
/// with the time of its latest erasure.
#[derive(Default, Clone, Debug)]
pub struct Erased {
    events: BTreeSet<(i64, DateTime<Utc>)>,
    people: BTreeMap<String, DateTime<Utc>>,
    secrets: BTreeMap<String, DateTime<Utc>>,
}

fn hashed(kind: &str, id: &str) -> String {
    hex::encode(sha2::Sha256::digest(format!("{kind}:{id}").as_bytes()))
}

fn person_hash(id: &str) -> String {
    hashed("person", &id.trim().to_lowercase())
}

/// Keep the later of two erasures of the same id.
fn later(map: &mut BTreeMap<String, DateTime<Utc>>, key: String, at: DateTime<Utc>) {
    let e = map.entry(key).or_insert(at);
    if *e < at {
        *e = at;
    }
}

fn parse_at(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Utc))
}

impl Erased {
    pub fn event(&mut self, seq: i64, at: DateTime<Utc>) {
        self.events.insert((seq, at));
    }

    pub fn person(&mut self, id: &str, at: DateTime<Utc>) {
        later(&mut self.people, person_hash(id), at);
    }

    pub fn secret(&mut self, name: &str, at: DateTime<Utc>) {
        later(&mut self.secrets, hashed("secret", name), at);
    }

    pub fn merge(&mut self, other: Erased) {
        self.events.extend(other.events);
        for (k, at) in other.people {
            later(&mut self.people, k, at);
        }
        for (k, at) in other.secrets {
            later(&mut self.secrets, k, at);
        }
    }

    /// Whether nothing at all is recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.people.is_empty() && self.secrets.is_empty()
    }

    /// Whether a removal named this event; one without a readable time is
    /// taken by its number alone. Matched by number and time, so a removal
    /// recorded before a restore still takes it, and an event the restored
    /// log later gave the same number is not taken with it.
    pub fn names_event(&self, seq: i64, at: &Value) -> bool {
        match parse_at(at) {
            Some(at) => self.events.contains(&(seq, at)),
            None => self.events.iter().any(|(q, _)| *q == seq),
        }
    }

    /// Whether `id` was erased at or after `made` (a backup's time; any
    /// time when it is not known). A person erased before a backup was made
    /// and recreated since keeps what that backup holds of the new one.
    pub fn holds_person(&self, id: &str, made: Option<DateTime<Utc>>) -> bool {
        self.people.get(&person_hash(id)).is_some_and(|at| made.is_none_or(|m| m <= *at))
    }

    /// Whether the vault entry `name` was erased at or after `made`: named
    /// at an erasure, or a sign-in, card or wallet of an erased person.
    pub fn holds_secret(&self, name: &str, made: Option<DateTime<Utc>>) -> bool {
        self.secrets.get(&hashed("secret", name)).is_some_and(|at| made.is_none_or(|m| m <= *at)) || owner_of(name).is_some_and(|p| self.holds_person(p, made))
    }

    fn to_json(&self) -> Value {
        let pairs = |m: &BTreeMap<String, DateTime<Utc>>| m.iter().map(|(h, at)| json!([h, at.to_rfc3339()])).collect::<Vec<_>>();
        json!({
            "removed": self.events.iter().map(|(q, at)| json!([q, at.to_rfc3339()])).collect::<Vec<_>>(),
            "people": pairs(&self.people),
            "secrets": pairs(&self.secrets),
        })
    }

    fn from_json(v: &Value) -> Result<Erased> {
        let mut out = Erased::default();
        for r in v["removed"].as_array().context("no removals")? {
            out.events.insert((r[0].as_i64().context("a removal without its event")?, parse_at(&r[1]).context("a removal without its time")?));
        }
        for (field, map) in [("people", &mut out.people), ("secrets", &mut out.secrets)] {
            for r in v[field].as_array().into_iter().flatten() {
                let h = r[0].as_str().context("an erasure without its hash")?.to_string();
                later(map, h, parse_at(&r[1]).context("an erasure without its time")?);
            }
        }
        Ok(out)
    }
}

/// The person a vault entry of theirs alone belongs to, read from its
/// name: a sign-in to an app, their card or their Link wallet.
fn owner_of(name: &str) -> Option<&str> {
    let p = name
        .strip_prefix("payments/card/")
        .or_else(|| name.strip_prefix("payments/link/"))
        .or_else(|| name.strip_prefix("connector_").and_then(|r| r.split_once("__")).map(|(_, p)| p))?;
    crate::forget::their_secret(name, p).then_some(p)
}

/// The erasures recorded in `dir`: none if it has no record, an error if it
/// has one that cannot be read.
fn read_record(dir: &Path) -> Result<Erased> {
    let bytes = match std::fs::read(dir.join(RECORD)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Erased::default()),
        Err(e) => return Err(e.into()),
    };
    Erased::from_json(&serde_json::from_slice(&bytes)?)
}

/// Add `known` to the record in each of `dirs` and return all of them. A
/// record is only added to, never replaced: an erasure recorded once stays
/// recorded through any restore of a backup older than it, whose log does
/// not have it (#195). A record that cannot be read is left as it is.
fn record(dirs: &[PathBuf], mut known: Erased) -> Erased {
    let mut writable = vec![];
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        match read_record(dir) {
            Ok(had) => {
                known.merge(had);
                writable.push(dir);
            }
            Err(e) => tracing::warn!("reading the erasures recorded in {}: {e:#}", dir.display()),
        }
    }
    let doc = known.to_json().to_string();
    for dir in writable {
        if let Err(e) = crate::vault::write_private(&dir.join(RECORD), doc.as_bytes()) {
            tracing::warn!("recording the erasures in {}: {e:#}", dir.display());
        }
    }
    known
}

/// Record `known` in the state directory and beside the backups; all that
/// is recorded there.
pub fn note(cfg: &Config, known: Erased) -> Erased {
    record(&[cfg.appliance.state_dir.clone(), cfg.backup_dir()], known)
}

/// Everything erased, as recorded in the state directory, beside the
/// backups and beside the backup being restored; all of it recorded in the
/// state directory too, for later restores. A record that cannot be read
/// stops the restore, which could otherwise bring back what it names.
pub fn for_restore(cfg: &Config, from: &Path) -> Result<Erased> {
    let dirs = [Some(cfg.appliance.state_dir.clone()), Some(cfg.backup_dir()), from.parent().map(Path::to_path_buf)];
    let mut all = Erased::default();
    for d in dirs.into_iter().flatten() {
        all.merge(read_record(&d).with_context(|| format!("read the erasures recorded in {}", d.display()))?);
    }
    std::fs::create_dir_all(&cfg.appliance.state_dir)?;
    Ok(record(std::slice::from_ref(&cfg.appliance.state_dir), all))
}

/// Take everything erased out of a backup, store by store; whether it
/// changed. A backup made after an erasure keeps what it holds of that id.
pub fn drop_erased(doc: &mut Value, erased: &Erased) -> Result<bool> {
    let made = parse_at(&doc["made_at"]);
    let mut changed = false;
    for store in STORES {
        let dropped = match store {
            Store::LogContent => drop_content(doc, erased),
            Store::Accounts => drop_accounts(doc, erased, made)?,
            Store::Vault => drop_secrets(doc, erased, made)?,
            Store::Devices => drop_devices(doc, erased, made),
        };
        changed |= dropped;
    }
    Ok(changed)
}

fn drop_content(doc: &mut Value, erased: &Erased) -> bool {
    let mut changed = false;
    for e in doc["events"].as_array_mut().into_iter().flatten() {
        if e["content"].is_null() && e["salt"].is_null() {
            continue;
        }
        if e["seq"].as_i64().is_some_and(|q| erased.names_event(q, &e["at"])) {
            e["content"] = Value::Null;
            e["salt"] = Value::Null;
            changed = true;
        }
    }
    changed
}

fn drop_accounts(doc: &mut Value, erased: &Erased, made: Option<DateTime<Utc>>) -> Result<bool> {
    if erased.people.is_empty() || doc.get("accounts").is_none() {
        return Ok(false);
    }
    let mut tables = account_tables(&doc["accounts"])?;
    let mut changed = false;
    for rows in tables.values_mut() {
        let before = rows.len();
        rows.retain(|r| !r["person"].as_str().is_some_and(|p| erased.holds_person(p, made)));
        changed |= rows.len() != before;
    }
    if changed {
        doc["accounts"] = json!({ "tables": tables });
    }
    Ok(changed)
}

fn drop_secrets(doc: &mut Value, erased: &Erased, made: Option<DateTime<Utc>>) -> Result<bool> {
    let Some(stored) = doc["vault"]["secrets"].as_str() else { return Ok(false) };
    let mut all: BTreeMap<String, Value> = serde_json::from_str(stored).context("the backed-up vault")?;
    let before = all.len();
    all.retain(|name, _| !erased.holds_secret(name, made));
    if all.len() == before {
        return Ok(false);
    }
    doc["vault"]["secrets"] = Value::String(serde_json::to_string(&all)?);
    Ok(true)
}

fn drop_devices(doc: &mut Value, erased: &Erased, made: Option<DateTime<Utc>>) -> bool {
    let Some(all) = doc.get_mut("devices").and_then(Value::as_object_mut) else { return false };
    let before = all.len();
    all.retain(|_, d| !d["person"].as_str().is_some_and(|p| erased.holds_person(p, made)));
    all.len() != before
}

/// The accounts tables, as rows, in one snapshot; the sessions left out.
pub async fn take_accounts(cfg: &Config) -> Result<Value> {
    let mut c = crate::log::connect(&cfg.database.url).await?;
    let tx = c.build_transaction().isolation_level(tokio_postgres::IsolationLevel::RepeatableRead).read_only(true).start().await?;
    let mut tables = serde_json::Map::new();
    for t in crate::accounts::TABLES.iter().filter(|t| !NEVER_KEPT.contains(t)) {
        let rows: Value = tx
            .query_one(&format!("SELECT coalesce(jsonb_agg(to_jsonb(x)), '[]'::jsonb) FROM public.{t} x"), &[])
            .await
            .with_context(|| format!("back up the accounts table {t}"))?
            .get(0);
        tables.insert(t.to_string(), rows);
    }
    tx.commit().await?;
    Ok(json!({ "tables": tables }))
}

/// The paired devices, as their registry holds them.
pub fn take_devices(cfg: &Config) -> Result<Value> {
    let path = crate::devices::file(&cfg.appliance.state_dir);
    match std::fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b).with_context(|| format!("{} does not parse", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

/// The accounts tables a backup holds, by table: rows as they are now
/// backed up, or read out of the database dump older backups hold.
pub fn account_tables(a: &Value) -> Result<BTreeMap<String, Vec<Value>>> {
    if let Some(tables) = a.get("tables") {
        return serde_json::from_value(tables.clone()).context("the backed-up accounts tables");
    }
    let dump = B64.decode(a["dump"].as_str().unwrap_or_default()).context("the accounts dump")?;
    let text = crate::backup::pg_tool("pg_restore", &["--data-only", "--file=-"], &[], &dump).context("the accounts dump does not read back")?;
    rows_from_copy(&String::from_utf8_lossy(&text))
}

/// That a backup's accounts tables and devices read back.
pub fn check(v: &Value) -> Result<()> {
    if let Some(a) = v.get("accounts") {
        if a.get("tables").is_some() {
            for (t, rows) in account_tables(a)? {
                anyhow::ensure!(crate::accounts::TABLES.contains(&t.as_str()), "the backup holds the accounts table {t}, which this release does not know");
                anyhow::ensure!(rows.iter().all(Value::is_object), "the accounts table {t} holds a row that is not one");
            }
        } else {
            let dump = B64.decode(a["dump"].as_str().unwrap_or_default()).context("the accounts dump")?;
            crate::backup::pg_tool("pg_restore", &["--list"], &[], &dump).context("the accounts dump does not read back")?;
        }
    }
    devices_file(v)?;
    Ok(())
}

/// The accounts tables back into the server's own database, emptied first,
/// in one transaction on their own connection: the log's restore holds its
/// own open. Sessions are never put back.
pub async fn put_back_accounts(cfg: &Config, v: &Value) -> Result<bool> {
    let Some(a) = v.get("accounts").cloned() else { return Ok(false) };
    let tables = tokio::task::spawn_blocking(move || account_tables(&a)).await??;
    let mut c = crate::log::connect(&cfg.database.url).await?;
    let tx = c.transaction().await?;
    for t in crate::accounts::TABLES {
        tx.execute(&format!("DELETE FROM public.{t}"), &[]).await?;
    }
    for (t, rows) in &tables {
        let Some(t) = crate::accounts::TABLES.iter().find(|k| **k == t.as_str()) else { bail!("the backup holds the accounts table {t}, which this release does not know") };
        if NEVER_KEPT.contains(t) || rows.is_empty() {
            continue;
        }
        let cols: Vec<String> = rows[0].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for col in &cols {
            anyhow::ensure!(!col.is_empty() && col.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'), "the accounts table {t} has a column {col:?}");
        }
        let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        tx.execute(&format!("INSERT INTO public.{t} ({list}) SELECT {list} FROM jsonb_populate_recordset(NULL::public.{t}, $1)"), &[&Value::Array(rows.clone())])
            .await
            .with_context(|| format!("restore the accounts table {t}"))?;
    }
    tx.commit().await.context("restore the accounts tables")?;
    Ok(true)
}

/// The paired devices a backup holds, as their registry file, checked to
/// read as one; none for a backup from before they were kept.
pub fn devices_file(v: &Value) -> Result<Option<Vec<u8>>> {
    let Some(d) = v.get("devices") else { return Ok(None) };
    let devices: BTreeMap<String, crate::devices::Device> = serde_json::from_value(d.clone()).context("the backed-up paired devices")?;
    Ok(Some(serde_json::to_vec_pretty(&devices)?))
}

/// The rows of each table in `pg_restore --data-only` output: its `COPY`
/// blocks, in Postgres's text format.
pub fn rows_from_copy(text: &str) -> Result<BTreeMap<String, Vec<Value>>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(head) = line.strip_prefix("COPY ").and_then(|l| l.strip_suffix(" FROM stdin;")) else { continue };
        let (name, cols) = head.split_once(" (").context("a COPY without its columns")?;
        let table = unquote(name.rsplit('.').next().unwrap_or(name));
        let cols: Vec<String> = cols.strip_suffix(')').context("a COPY without its columns")?.split(", ").map(unquote).collect();
        let rows = out.entry(table.clone()).or_default();
        loop {
            let row = lines.next().with_context(|| format!("the rows of {table} do not end"))?;
            if row == "\\." {
                break;
            }
            let fields: Vec<&str> = row.split('\t').collect();
            anyhow::ensure!(fields.len() == cols.len(), "a row of {table} has {} fields, not {}", fields.len(), cols.len());
            let obj: serde_json::Map<String, Value> =
                cols.iter().zip(fields).map(|(c, f)| (c.clone(), if f == "\\N" { Value::Null } else { Value::String(unescape(f)) })).collect();
            rows.push(Value::Object(obj));
        }
    }
    Ok(out)
}

fn unquote(s: &str) -> String {
    match s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => s.to_string(),
    }
}

/// A field of Postgres's COPY text format, its backslash escapes undone.
fn unescape(f: &str) -> String {
    let b = f.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        i += 2;
        match c {
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'0'..=b'7' => {
                let mut n = u32::from(c - b'0');
                for _ in 0..2 {
                    match b.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            n = n * 8 + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(n as u8);
            }
            b'x' if b.get(i).is_some_and(u8::is_ascii_hexdigit) => {
                let mut n = 0u32;
                for _ in 0..2 {
                    match b.get(i).and_then(|d| (*d as char).to_digit(16)) {
                        Some(d) => {
                            n = n * 16 + d;
                            i += 1;
                        }
                        None => break,
                    }
                }
                out.push(n as u8);
            }
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl crate::kernel::Kernel {
    /// Everything this log says was erased: the events removals named, and
    /// the people erased.
    pub async fn erased(&self) -> Result<Erased> {
        let mut out = Erased::default();
        for (seq, at) in self.log.removed_events().await? {
            out.event(seq, at);
        }
        for (_, at, _, body) in self.log.of_kinds(&["person_erased"], i64::MAX).await? {
            if let Some(p) = body["person"].as_str() {
                out.person(p, at);
            }
        }
        Ok(out)
    }

    /// A person is being erased: they, and these vault entries of theirs,
    /// are recorded as erased now, so every backup made before now loses
    /// them at the next scrub and at any restore.
    pub fn note_erased(&self, person: &str, secrets: &[String]) {
        let now = Utc::now();
        let mut known = Erased::default();
        known.person(person, now);
        for s in secrets {
            known.secret(s, now);
        }
        note(&self.cfg, known);
    }
}

#[cfg(test)]
#[path = "person_stores_tests.rs"]
mod tests;
