//! Backups of the event log and the vault (K10): one encrypted file each
//! time, made on a schedule and on the owner's request, each one restored
//! in memory as soon as it is written (every event replayed, every secret
//! decrypted) so a backup that would not restore is known at once. Kept no
//! longer than `keep_days` (the record: key-store backups no longer than the
//! legal deletion deadline). `magnus restore` puts one back into an empty
//! database and state directory.
//!
//! The file is XChaCha20-Poly1305 under the backup key (`[backup] key_file`),
//! which is not the vault's key. Whoever holds the file and that key holds
//! the appliance's history and secrets, so the key belongs off the appliance.

use crate::config::Config;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{bail, Context, Result};
use base64::Engine;
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAGIC: &[u8] = b"MAGNUS-BACKUP-1\n";
const FORMAT: &str = "magnus-backup/1";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn key(cfg: &Config, create: bool) -> Result<XChaCha20Poly1305> {
    let path = cfg.backup_key_file();
    if !path.exists() {
        if !create {
            bail!("no backup key at {}: put the key kept with the backups there", path.display());
        }
        let mut k = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut k);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::vault::write_private(&path, &k)?;
    }
    let k = std::fs::read(&path).with_context(|| format!("read the backup key {}", path.display()))?;
    anyhow::ensure!(k.len() == 32, "the backup key is not 32 bytes");
    XChaCha20Poly1305::new_from_slice(&k).map_err(|_| anyhow::anyhow!("backup key"))
}

fn seal(cipher: &XChaCha20Poly1305, plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher.encrypt(XNonce::from_slice(&nonce), plain).map_err(|_| anyhow::anyhow!("encrypt the backup"))?;
    Ok([MAGIC, &nonce[..], &ct].concat())
}

fn open(cipher: &XChaCha20Poly1305, file: &[u8]) -> Result<Value> {
    let body = file.strip_prefix(MAGIC).context("not a Magnus backup")?;
    anyhow::ensure!(body.len() > 24, "the backup is truncated");
    let plain = cipher.decrypt(XNonce::from_slice(&body[..24]), &body[24..]).map_err(|_| anyhow::anyhow!("the backup does not decrypt with this key"))?;
    let v: Value = serde_json::from_slice(&plain)?;
    anyhow::ensure!(v["format"] == FORMAT, "unknown backup format {}", v["format"]);
    Ok(v)
}

/// Restore a backup in memory: every event parses and replays in order,
/// and every vault secret decrypts with the backed-up vault key.
pub fn check(v: &Value) -> Result<(usize, i64)> {
    let mut state = crate::state::State::default();
    let mut last = 0i64;
    let events = v["events"].as_array().context("no events")?;
    for e in events {
        let seq = e["seq"].as_i64().context("an event without seq")?;
        anyhow::ensure!(seq > last, "events out of order at {seq}");
        last = seq;
        // As the log loads it: its content put back, and a kind from a later
        // release skipped.
        let body = crate::log::join(e["body"].clone(), Some(e["content"].clone()).filter(|c| !c.is_null()));
        let Some(ev) = crate::log::parse_event(&body).with_context(|| format!("event {seq} does not parse"))? else { continue };
        let at = chrono::DateTime::parse_from_rfc3339(e["at"].as_str().unwrap_or_default()).with_context(|| format!("event {seq} has no time"))?.with_timezone(&chrono::Utc);
        state.apply(&crate::log::Event { seq, at, actor: e["actor"].as_str().unwrap_or_default().into(), ev });
    }
    anyhow::ensure!(v["through_seq"].as_i64() == Some(last), "the backup ends at {last}, not at {}", v["through_seq"]);
    let vk = B64.decode(v["vault"]["key"].as_str().unwrap_or_default())?;
    crate::vault::check(&vk, v["vault"]["secrets"].as_str().unwrap_or("{}"))?;
    // Each database the appliance depends on reads back whole (#296).
    for d in v["databases"].as_array().into_iter().flatten() {
        let name = d["name"].as_str().unwrap_or_default();
        let dump = B64.decode(d["dump"].as_str().unwrap_or_default()).with_context(|| format!("the dump of {name}"))?;
        pg_tool("pg_restore", &["--list"], &[], &dump).with_context(|| format!("the dump of {name} does not read back"))?;
    }
    // The accounts tables, when the backup holds them (#466).
    if let Some(a) = v.get("accounts") {
        let dump = B64.decode(a["dump"].as_str().unwrap_or_default()).context("the accounts dump")?;
        pg_tool("pg_restore", &["--list"], &[], &dump).context("the accounts dump does not read back")?;
    }
    Ok((events.len(), last))
}

/// A database the backup holds: its name, its URL without the password (for
/// the tools' command line, which others on the machine can read), and the
/// password (given to them in the environment).
fn database(label: &str, url: &str) -> Result<(String, String, String)> {
    let mut u = url::Url::parse(url).with_context(|| format!("{label} holds a URL that does not parse"))?;
    let name = u.path().trim_start_matches('/').to_string();
    anyhow::ensure!(!name.is_empty() && u.scheme().starts_with("postgres"), "{label}: {} names no Postgres database", u.host_str().unwrap_or_default());
    let password = url::form_urlencoded::parse(format!("p={}", u.password().unwrap_or_default()).as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default();
    u.set_password(None).map_err(|_| anyhow::anyhow!("{label}: {name} has no password to take out"))?;
    Ok((name, u.to_string(), password))
}

/// Run `pg_dump` or `pg_restore`, with `input` on its stdin; its stdout.
fn pg_tool(tool: &str, args: &[&str], env: &[(&str, &str)], input: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut child = std::process::Command::new(tool)
        .args(args)
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("run {tool}"))?;
    let mut stdin = child.stdin.take().context("stdin")?;
    let input = input.to_vec();
    let feed = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output()?;
    let _ = feed.join();
    anyhow::ensure!(out.status.success(), "{tool}: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(out.stdout)
}

/// Each configured database, dumped (custom format, no owners or grants:
/// a restore makes them the restoring role's).
fn dump_databases(cfg: &Config) -> Result<Vec<Value>> {
    cfg.backup
        .databases
        .iter()
        .map(|url| {
            let (name, dsn, password) = database("[backup] databases", url)?;
            let dump = pg_tool("pg_dump", &["--format=custom", "--no-owner", "--no-privileges", "--dbname", &dsn], &[("PGPASSWORD", &password)], &[])
                .with_context(|| format!("back up the database {name}"))?;
            Ok(json!({ "name": name, "dump": B64.encode(dump) }))
        })
        .collect()
}

/// The accounts tables, dumped from the appliance's own database,
/// table-scoped: the log rides the events, not a dump, and the accounts
/// tables ride neither — they are no event's content — so they get their
/// own (#466).
fn dump_accounts(cfg: &Config) -> Result<Value> {
    let (_, dsn, password) = database("[database] url", &cfg.database.url)?;
    let mut owned: Vec<String> = vec!["--format=custom".into(), "--no-owner".into(), "--no-privileges".into()];
    for t in crate::accounts::TABLES {
        owned.push(format!("--table=public.{t}"));
    }
    owned.push("--dbname".into());
    owned.push(dsn);
    let args: Vec<&str> = owned.iter().map(String::as_str).collect();
    let dump = pg_tool("pg_dump", &args, &[("PGPASSWORD", &password)], &[]).context("back up the accounts tables")?;
    Ok(json!({ "dump": B64.encode(dump) }))
}

/// The accounts tables back into the appliance's own database: data only,
/// into the migrated tables, so grants stay as `migrate` left them. The
/// caller empties the tables first, so a rerun loads clean (#466).
fn restore_accounts(cfg: &Config, v: &Value) -> Result<bool> {
    let Some(a) = v.get("accounts") else { return Ok(false) };
    let (_, dsn, password) = database("[database] url", &cfg.database.url)?;
    let dump = B64.decode(a["dump"].as_str().unwrap_or_default()).context("the accounts dump")?;
    pg_tool("pg_restore", &["--data-only", "--single-transaction", "--no-owner", "--no-privileges", "--dbname", &dsn], &[("PGPASSWORD", &password)], &dump)
        .context("restore the accounts tables")?;
    Ok(true)
}

/// Put each database the backup holds back, into the one `[backup]
/// databases` names with that name; all are found before any is written.
fn restore_databases(cfg: &Config, v: &Value) -> Result<usize> {
    let mut plan = Vec::new();
    for d in v["databases"].as_array().into_iter().flatten() {
        let name = d["name"].as_str().unwrap_or_default();
        let target = cfg.backup.databases.iter().map(|u| database("[backup] databases", u)).collect::<Result<Vec<_>>>()?.into_iter().find(|(n, ..)| n == name);
        let Some((_, dsn, password)) = target else { bail!("the backup holds the database {name}, which [backup] databases does not name") };
        plan.push((name.to_string(), dsn, password, B64.decode(d["dump"].as_str().unwrap_or_default())?));
    }
    for (name, dsn, password, dump) in &plan {
        pg_tool("pg_restore", &["--clean", "--if-exists", "--no-owner", "--no-privileges", "--single-transaction", "--dbname", dsn], &[("PGPASSWORD", password)], dump)
            .with_context(|| format!("restore the database {name}"))?;
    }
    Ok(plan.len())
}

impl Kernel {
    /// Back up now: write the file, restore it in memory, record both, and
    /// drop backups older than `keep_days`.
    pub async fn back_up(&self, actor: &str) -> Result<Value> {
        let _one = backing_up().lock().await;
        let mut rows = self.log.dump().await?;
        let through = rows.last().map(|r| r["seq"].as_i64().unwrap_or(0)).unwrap_or(0);
        // Every removal in the dump applies to it: content whose deletion
        // was recorded but had not happened yet is left out, and the backup
        // says it has had them all (`scrub_backups`).
        let removed: std::collections::HashSet<i64> =
            rows.iter().filter(|r| r["kind"] == "removed").flat_map(|r| r["body"]["seqs"].as_array().cloned().unwrap_or_default()).filter_map(|q| q.as_i64()).collect();
        let scrubbed = rows.iter().filter(|r| r["kind"] == "removed").filter_map(|r| r["seq"].as_i64()).max().unwrap_or(0);
        for r in rows.iter_mut().filter(|r| r["seq"].as_i64().is_some_and(|q| removed.contains(&q))) {
            r["content"] = Value::Null;
            r["salt"] = Value::Null;
        }
        let (vault_key, secrets) = self.vault.raw()?;
        let cfg = self.cfg.clone();
        let databases = tokio::task::spawn_blocking(move || dump_databases(&cfg)).await??;
        let cfg = self.cfg.clone();
        let accounts = tokio::task::spawn_blocking(move || dump_accounts(&cfg)).await??;
        let cfg = self.cfg.clone();
        let team_files = tokio::task::spawn_blocking(move || crate::team::pack_files(&cfg)).await??;
        let made = chrono::Utc::now();
        let doc = json!({
            "format": FORMAT, "made_at": made, "through_seq": through, "scrubbed_through": scrubbed, "events": rows,
            "vault": { "key": B64.encode(vault_key), "secrets": secrets },
            "databases": databases, "accounts": accounts, "team_files": team_files,
        });
        let plain = serde_json::to_vec(&doc)?;
        let cipher = key(&self.cfg, true)?;
        let bytes = seal(&cipher, &plain)?;
        let dir = self.cfg.backup_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("magnus-{}.backup", made.format("%Y%m%dT%H%M%S%.3fZ")));
        crate::vault::write_private(&path, &bytes)?;
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        self.emit(actor, Ev::BackupMade { file: file.clone(), through_seq: through, events: rows.len(), bytes: bytes.len() }).await?;
        // The restore test, on the file as written.
        let checked = std::fs::read(&path).map_err(anyhow::Error::from).and_then(|b| open(&cipher, &b)).and_then(|v| check(&v));
        let (ok, detail) = match &checked {
            Ok((n, last)) => {
                let dbs = doc["databases"].as_array().map(|d| d.len()).unwrap_or(0);
                let accounts = if doc.get("accounts").is_some() { ", the accounts tables read back" } else { "" };
                (true, format!("restored in memory: {n} events through {last}, every secret decrypts, {dbs} databases read back{accounts}"))
            }
            Err(e) => (false, format!("{e:#}")),
        };
        self.emit("kernel", Ev::BackupChecked { file: file.clone(), ok, detail: detail.clone() }).await?;
        let removed = prune(&dir, self.backup_keep_days())?;
        // Off host, when a destination is configured: the directory as it
        // is now, pruned; a failed copy is recorded, not a failed backup.
        let (copied, copy_detail) = match self.cfg.backup.copy_to.clone().filter(|d| !d.trim().is_empty()) {
            Some(dest) => {
                let (ok, detail) = mirror(&dir, &dest, self.cfg.backup.copy_timeout_secs).await;
                self.emit("kernel", Ev::BackupCopied { file: file.clone(), ok, detail: detail.clone() }).await?;
                (Some(ok), Some(detail))
            }
            None => (None, None),
        };
        Ok(json!({
            "file": file, "through_seq": through, "events": rows.len(), "bytes": bytes.len(),
            "restored": ok, "detail": detail, "removed": removed, "copied": copied, "copy_detail": copy_detail,
        }))
    }
}

/// Mirror the backups directory to the configured off-host destination
/// (#297): rsync, deletions included so a pruned backup leaves there too.
/// Whether it held, and why not.
async fn mirror(dir: &Path, dest: &str, timeout_secs: u32) -> (bool, String) {
    let from = format!("{}/", dir.display());
    let run = tokio::process::Command::new("rsync").args(["-a", "--delete", &from, dest]).output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs.max(1) as u64), run).await {
        Err(_) => return (false, format!("the copy to {dest} took longer than {timeout_secs} seconds")),
        Ok(Err(e)) => return (false, format!("the copy to {dest} would not start ({e:#})")),
        Ok(Ok(out)) => out,
    };
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect::<String>();
        return (false, format!("the copy to {dest} failed ({})", if why.is_empty() { out.status.to_string() } else { why }));
    }
    let n = std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0);
    (true, format!("mirrored {n} files to {dest}"))
}

impl Kernel {
    /// Take removed content out of every backup kept, so a removed item is
    /// gone from the backups when it is gone from the appliance, not a
    /// month later. Each backup records the last removal it has had applied
    /// (`scrubbed_through`); one made before that removal is rewritten, its
    /// removed events' content and salt dropped, and encrypted again. Run
    /// after each removal, at start (for a removal cut short) and hourly
    /// while a backup could not be rewritten.
    ///
    /// A backup that cannot be read or rewritten does not stop the others;
    /// it is recorded (`BackupScrubPending`) and the scrub fails, so an
    /// erasure is not called complete while a backup still holds its
    /// content, until a later scrub rewrites it (`BackupsScrubbed`) (#195).
    pub async fn scrub_backups(&self) -> Result<usize> {
        let removals = self.log.removals().await?;
        let Some(&(latest, _)) = removals.last() else { return Ok(0) };
        // With no scrub left pending, every backup has had the removals up
        // to the one before the latest.
        let before = removals.iter().rev().nth(1).map_or(0, |r| r.0);
        let removed = self.log.removed_events().await?;
        let dir = self.cfg.backup_dir();
        // A directory backups were made in that is gone now (a disk not
        // mounted) is not one without backups: they are only out of reach.
        let made = self.read(|s| s.last_backup.is_some() || s.scrub_pending.is_some());
        let shown = dir.display().to_string();
        let _one = backing_up().lock().await;
        let cfg = self.cfg.clone();
        let found = tokio::task::spawn_blocking(move || -> std::result::Result<(usize, Vec<(String, i64, String)>), String> {
            // What has been removed, where a restore of a backup that
            // could not be rewritten finds it (`restore`), and every
            // removal recorded before, in this log's history or another's.
            let gone = note_removals(&cfg, &removed);
            let files = match try_list(&dir) {
                Ok(files) => files,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !made => vec![],
                Err(e) => return Err(format!("the backups directory {} cannot be read: {e}", dir.display())),
            };
            let cipher = match key(&cfg, false) {
                Ok(c) => c,
                Err(e) => return Ok((0, files.into_iter().map(|(name, _)| (name, 0, format!("{e:#}"))).collect())),
            };
            let mut rewritten = 0;
            let mut left: Vec<(String, i64, String)> = vec![];
            for (name, _) in files {
                let mut done = 0;
                match scrub_one(&cipher, &dir.join(&name), &gone, latest, &mut done) {
                    Ok(true) => rewritten += 1,
                    Ok(false) => {}
                    Err(e) => left.push((name, done, format!("{e:#}"))),
                }
            }
            Ok((rewritten, left))
        })
        .await?;
        let (rewritten, left) = match found {
            Ok(found) => found,
            // Not looked at, so not rewritten: whatever they held, they
            // still hold. The backups already pending stay so; with none,
            // the directory is, for the latest removal.
            Err(detail) => {
                let (from, files) = self.read(|s| s.scrub_pending.as_ref().map(|p| (p.from, p.files.clone()))).filter(|(_, f)| !f.is_empty()).unwrap_or((before, vec![shown]));
                (0, files.into_iter().map(|f| (f, from, detail.clone())).collect())
            }
        };
        if left.is_empty() {
            if self.read(|s| s.scrub_pending.is_some()) {
                self.emit("kernel", Ev::BackupsScrubbed { through: latest }).await?;
            }
            return Ok(rewritten);
        }
        let from = left.iter().map(|l| l.1).min().unwrap_or(0);
        let files: Vec<String> = left.iter().map(|l| l.0.clone()).collect();
        let detail = left.iter().map(|(f, _, e)| format!("{f}: {e}")).collect::<Vec<_>>().join("; ");
        // Once per change, not at every retry.
        let same = self.read(|s| s.scrub_pending.as_ref().is_some_and(|p| (p.from, p.through, &p.files) == (from, latest, &files)));
        if !same {
            self.emit("kernel", Ev::BackupScrubPending { from, through: latest, files: files.clone(), detail: detail.clone() }).await?;
        }
        bail!("{} backups still hold removed content: {detail}", files.len())
    }
}

/// Every event whose content was removed, by its number and time (#195).
type Gone = std::collections::BTreeSet<(i64, chrono::DateTime<chrono::Utc>)>;

/// Take every removed event's content out of one backup, and mark it as
/// having had the removals through `latest`; whether it was rewritten.
/// `done` is the last removal it had, once it is read. An event is matched
/// by its number and its time, so a removal recorded before a restore
/// still takes it, and an event the restored log later gave the same
/// number is not taken with it.
fn scrub_one(cipher: &XChaCha20Poly1305, path: &Path, gone: &Gone, latest: i64, done: &mut i64) -> Result<bool> {
    let mut v = open(cipher, &std::fs::read(path)?)?;
    *done = v["scrubbed_through"].as_i64().unwrap_or(0);
    let mut changed = *done < latest;
    for e in v["events"].as_array_mut().into_iter().flatten() {
        if e["content"].is_null() && e["salt"].is_null() {
            continue;
        }
        if e["seq"].as_i64().is_some_and(|q| named(gone, q, &e["at"])) {
            e["content"] = Value::Null;
            e["salt"] = Value::Null;
            changed = true;
        }
    }
    if !changed {
        return Ok(false);
    }
    v["scrubbed_through"] = json!(latest.max(*done));
    crate::vault::write_private(path, &seal(cipher, &serde_json::to_vec(&v)?)?)?;
    Ok(true)
}

/// Whether a removal named this event; one without a readable time is
/// taken by its number alone.
fn named(gone: &Gone, seq: i64, at: &Value) -> bool {
    match at.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
        Some(at) => gone.contains(&(seq, at.with_timezone(&chrono::Utc))),
        None => gone.iter().any(|(q, _)| *q == seq),
    }
}

/// The record of removals a restore reads: the events whose content was
/// removed, by number and time only (no content, no names). Kept in the
/// state directory, which the kernel can always write, and beside the
/// backups, which go with them to another machine.
const REMOVALS: &str = "removals.json";

/// The removals recorded in `dir`: none if it has no record, an error if
/// it has one that cannot be read.
fn read_removals(dir: &Path) -> Result<Gone> {
    let bytes = match std::fs::read(dir.join(REMOVALS)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Gone::new()),
        Err(e) => return Err(e.into()),
    };
    let v: Value = serde_json::from_slice(&bytes)?;
    v["removed"]
        .as_array()
        .context("no removals")?
        .iter()
        .map(|r| {
            let at = chrono::DateTime::parse_from_rfc3339(r[1].as_str().unwrap_or_default())?.with_timezone(&chrono::Utc);
            Ok((r[0].as_i64().context("a removal without its event")?, at))
        })
        .collect()
}

/// Add `known` to the record in each of `dirs` and return all of them. A
/// record is only added to, never replaced: a removal recorded once stays
/// recorded through any restore of a backup older than it, whose log does
/// not have it (#195). A record that cannot be read is left as it is.
fn record_removals(dirs: &[PathBuf], mut known: Gone) -> Gone {
    let mut writable = vec![];
    for dir in dirs.iter().filter(|d| d.is_dir()) {
        match read_removals(dir) {
            Ok(had) => {
                known.extend(had);
                writable.push(dir);
            }
            Err(e) => tracing::warn!("reading the removals recorded in {}: {e:#}", dir.display()),
        }
    }
    let doc = json!({ "removed": known.iter().map(|(q, at)| json!([q, at.to_rfc3339()])).collect::<Vec<_>>() }).to_string();
    for dir in writable {
        if let Err(e) = crate::vault::write_private(&dir.join(REMOVALS), doc.as_bytes()) {
            tracing::warn!("recording the removals in {}: {e:#}", dir.display());
        }
    }
    known
}

fn note_removals(cfg: &Config, removed: &[(i64, chrono::DateTime<chrono::Utc>)]) -> Gone {
    record_removals(&[cfg.appliance.state_dir.clone(), cfg.backup_dir()], removed.iter().copied().collect())
}

/// Every event whose content was removed, as recorded in the state
/// directory, beside the backups and beside the backup being restored;
/// all of them recorded in the state directory too, for later restores. A
/// record that cannot be read stops the restore, which could otherwise
/// bring back what it names.
fn removed_events(cfg: &Config, from: &Path) -> Result<Gone> {
    let dirs = [Some(cfg.appliance.state_dir.clone()), Some(cfg.backup_dir()), from.parent().map(Path::to_path_buf)];
    let mut all = Gone::new();
    for d in dirs.into_iter().flatten() {
        all.extend(read_removals(&d).with_context(|| format!("read the removals recorded in {}", d.display()))?);
    }
    std::fs::create_dir_all(&cfg.appliance.state_dir)?;
    Ok(record_removals(&[cfg.appliance.state_dir.clone()], all))
}

fn backing_up() -> &'static tokio::sync::Mutex<()> {
    static M: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Backups past their keeping time, by the time in their name.
pub(crate) fn prune(dir: &Path, keep_days: u32) -> Result<Vec<String>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(keep_days as i64);
    let mut removed = vec![];
    if !dir.exists() {
        return Ok(removed);
    }
    for e in std::fs::read_dir(dir)? {
        let name = e?.file_name().to_string_lossy().to_string();
        let Some(t) = made_at(&name) else { continue };
        if t < cutoff {
            std::fs::remove_file(dir.join(&name))?;
            removed.push(name);
        }
    }
    Ok(removed)
}

/// When the newest backup in `dir` was made, by its name.
fn newest(dir: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    list(&dir.to_path_buf()).iter().filter_map(|(n, _)| made_at(n)).max()
}

fn made_at(name: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let ts = name.strip_prefix("magnus-")?.strip_suffix(".backup")?;
    chrono::NaiveDateTime::parse_from_str(ts, "%Y%m%dT%H%M%S%.3fZ").ok().map(|t| t.and_utc())
}

/// Back up every `every_hours`, counted from the last backup, so a restart
/// (an update, a crash) does not put the next one off; a first backup soon
/// after the first start. Old backups are dropped on every round, whether
/// or not a new one could be made.
pub fn schedule(k: Arc<Kernel>) {
    let every = k.cfg.backup.every_hours;
    let dir = k.cfg.backup_dir();
    let fallback_secs = k.cfg.backup.fallback_wait_secs;
    let retry_secs = k.cfg.backup.retry_wait_secs;
    // The window is read each round: an admin may change it (`retention.rs`).
    let drop_old = |k: &Kernel, dir: &Path| {
        if let Err(e) = prune(dir, k.backup_keep_days()) {
            tracing::error!("dropping old backups: {e:#}");
        }
    };
    drop_old(&k, &dir);
    if every == 0 {
        return;
    }
    tokio::spawn(async move {
        let period = chrono::Duration::hours(every as i64);
        loop {
            let wait = next_wait(newest(&dir), chrono::Utc::now(), period, fallback_secs);
            tokio::time::sleep(wait).await;
            if let Err(e) = k.back_up("kernel:backup").await {
                tracing::error!("scheduled backup: {e:#}");
            }
            drop_old(&k, &dir);
            // A failing backup is tried again after the configured wait,
            // not at once.
            if let Some(wait) = retry_wait(newest(&dir), chrono::Utc::now(), period, retry_secs) {
                tokio::time::sleep(wait).await;
            }
        }
    });
}

/// How long before the next scheduled backup: from the last backup, or soon
/// after the first start when there is none yet.
fn next_wait(newest: Option<chrono::DateTime<chrono::Utc>>, now: chrono::DateTime<chrono::Utc>, period: chrono::Duration, fallback_secs: u32) -> std::time::Duration {
    let wait = match newest {
        Some(at) => (at + period - now).max(chrono::Duration::seconds(60)),
        None => chrono::Duration::minutes(10).min(period),
    };
    to_std_or(wait, fallback_secs)
}

/// How long after this round before the next, when the round left no fresh
/// backup behind: the configured retry wait, else nothing.
fn retry_wait(newest: Option<chrono::DateTime<chrono::Utc>>, now: chrono::DateTime<chrono::Utc>, period: chrono::Duration, retry_secs: u32) -> Option<std::time::Duration> {
    let stale = newest.is_none_or(|at| now - at > period);
    stale.then(|| std::time::Duration::from_secs(retry_secs as u64))
}

/// A chrono wait as the sleeper takes it, or the configured fallback when it
/// cannot be converted.
fn to_std_or(wait: chrono::Duration, fallback_secs: u32) -> std::time::Duration {
    wait.to_std().unwrap_or(std::time::Duration::from_secs(fallback_secs as u64))
}

/// `magnus restore`: a backup into an empty event log and an empty vault.
pub async fn restore(cfg: &Config, admin_url: &str, from: &Path) -> Result<(usize, i64)> {
    let cipher = key(cfg, false)?;
    let v = open(&cipher, &std::fs::read(from).with_context(|| format!("read {}", from.display()))?)?;
    let (n, last) = check(&v)?;
    // A backup that could not be rewritten after a removal (an erasure)
    // still holds what was removed: it is left out here too (#195).
    let removed = removed_events(cfg, from)?;
    let vault_dir = cfg.appliance.state_dir.join("vault");
    if vault_dir.join("secrets.json").exists() {
        bail!("{} already holds secrets; restore into an empty state directory", vault_dir.display());
    }
    crate::log::migrate(admin_url, &cfg.database.url, None).await?;
    let admin = crate::log::connect(admin_url).await?;
    let mut db = admin;
    // No kernel may run on the log while it is restored: it would keep its
    // empty state and its vault key, and write over the restored ones.
    crate::log::hold(&db).await.context("a kernel is running on this event log; stop it before restoring")?;
    let tx = db.transaction().await?;
    let have: i64 = tx.query_one("SELECT (SELECT count(*) FROM public.events) + (SELECT count(*) FROM public.content)", &[]).await?.get(0);
    if have > 0 {
        bail!("the event log and its content already hold {have} rows; restore into empty ones");
    }
    for e in v["events"].as_array().into_iter().flatten() {
        let at = chrono::DateTime::parse_from_rfc3339(e["at"].as_str().unwrap_or_default())?.with_timezone(&chrono::Utc);
        tx.execute(
            "INSERT INTO public.events (seq, at, kind, actor, run_id, body) OVERRIDING SYSTEM VALUE VALUES ($1, $2, $3, $4, $5, $6)",
            &[&e["seq"].as_i64(), &at, &e["kind"].as_str(), &e["actor"].as_str(), &e["run_id"].as_str(), &e["body"]],
        )
        .await?;
        if !e["content"].is_null() && !e["seq"].as_i64().is_some_and(|q| removed.contains(&(q, at))) {
            let salt = hex::decode(e["salt"].as_str().unwrap_or_default()).with_context(|| format!("event {} has no salt", e["seq"]))?;
            tx.execute("INSERT INTO public.content (seq, salt, body) VALUES ($1, $2, $3)", &[&e["seq"].as_i64(), &salt, &e["content"]]).await?;
        }
    }
    if last > 0 {
        tx.execute("SELECT setval(pg_get_serial_sequence('public.events', 'seq'), $1)", &[&last]).await?;
    }
    // The backup does not say whose each row is; the events do.
    tx.batch_execute(crate::log::CONTENT_OWNERS).await?;
    // The vault first, then the log's commit: a vault that cannot be
    // written leaves the log empty, so the restore can be run again; a
    // commit that fails takes the written vault back out.
    let write_vault = || -> Result<()> {
        std::fs::create_dir_all(&vault_dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&vault_dir, std::fs::Permissions::from_mode(0o700))?;
        crate::vault::write_private(&vault_dir.join("key"), &B64.decode(v["vault"]["key"].as_str().unwrap_or_default())?)?;
        crate::vault::write_private(&vault_dir.join("secrets.json"), v["vault"]["secrets"].as_str().unwrap_or("{}").as_bytes())?;
        Ok(())
    };
    // The databases the appliance depends on, while the log's transaction is
    // still open: one that cannot be put back leaves the log empty, so the
    // restore can be run again (#296).
    let (c, w) = (cfg.clone(), v.clone());
    tokio::task::spawn_blocking(move || restore_databases(&c, &w)).await??;
    let (c, w) = (cfg.clone(), v.clone());
    tokio::task::spawn_blocking(move || crate::team::unpack_files(&c, &w)).await??;
    // The accounts tables likewise (#466): emptied first on their own
    // committed connection (a data load cannot run inside the log's
    // transaction, and must not meet rows a run before it left), then
    // loaded while the log's transaction is still open.
    if v.get("accounts").is_some() {
        let wipe = crate::log::connect(&cfg.database.url).await?;
        for t in crate::accounts::TABLES {
            wipe.execute(&format!("DELETE FROM public.{t}"), &[]).await?;
        }
        let (c, w) = (cfg.clone(), v.clone());
        tokio::task::spawn_blocking(move || restore_accounts(&c, &w)).await??;
    }
    let old_key = std::fs::read(vault_dir.join("key")).ok();
    let undo = || {
        let _ = std::fs::remove_file(vault_dir.join("secrets.json"));
        match &old_key {
            Some(k) => {
                let _ = crate::vault::write_private(&vault_dir.join("key"), k);
            }
            None => {
                let _ = std::fs::remove_file(vault_dir.join("key"));
            }
        }
    };
    if let Err(e) = write_vault() {
        undo();
        return Err(e.context("write the restored vault"));
    }
    if let Err(e) = tx.commit().await {
        undo();
        return Err(anyhow::Error::from(e).context("commit the restored log"));
    }
    Ok((n, last))
}

/// The backups as the owner's Settings and the admin's Operations show
/// them: where, how often, how long kept, the latest and whether it
/// restored, the off-host copy, and backups still holding removed content.
pub fn status(k: &Kernel) -> Value {
    let dir = k.cfg.backup_dir();
    let files: Vec<Value> = list(&dir).into_iter().map(|(f, b)| json!({ "file": f, "bytes": b })).collect();
    let last = k.read(|s| s.last_backup.clone()).map(|(file, ok, detail, at)| json!({ "file": file, "restored": ok, "detail": detail, "at": at }));
    let copy = k.read(|s| s.last_copy.clone()).map(|(file, ok, detail, at)| json!({ "file": file, "copied": ok, "detail": detail, "at": at }));
    let c = &k.cfg.backup;
    // Backups that still hold removed content, erased people's among it (#195).
    let pending =
        k.read(|s| s.scrub_pending.clone().map(|p| json!({ "files": p.files, "detail": p.detail, "since": p.at, "erasures": s.erasures.values().filter(|e| e.pending).count() })));
    json!({ "dir": dir, "every_hours": c.every_hours, "keep_days": k.backup_keep_days(), "copy_to": c.copy_to, "on_appliance_disk": c.dir.is_none(), "latest": last, "copy": copy, "files": files, "scrub_pending": pending })
}

pub fn list(dir: &PathBuf) -> Vec<(String, u64)> {
    try_list(dir).unwrap_or_default()
}

/// The backups in `dir`, or why it could not be read: a directory that is
/// missing or unreadable is not one without backups (#195).
fn try_list(dir: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with("magnus-") && n.ends_with(".backup") {
            out.push((n, e.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_fallback_wait_is_honoured() {
        // A wait that cannot be converted sleeps the configured fallback.
        assert_eq!(to_std_or(chrono::Duration::seconds(-1), 7), std::time::Duration::from_secs(7));
        // A wait that converts is untouched by it.
        assert_eq!(to_std_or(chrono::Duration::seconds(30), 7), std::time::Duration::from_secs(30));
    }

    #[test]
    fn configured_retry_wait_is_honoured() {
        let now = chrono::Utc::now();
        let period = chrono::Duration::hours(24);
        // No backup, or none within the period: the configured retry wait.
        assert_eq!(retry_wait(None, now, period, 120), Some(std::time::Duration::from_secs(120)));
        assert_eq!(retry_wait(Some(now - chrono::Duration::hours(25)), now, period, 120), Some(std::time::Duration::from_secs(120)));
        // A fresh backup: no retry wait at all.
        assert_eq!(retry_wait(Some(now - chrono::Duration::hours(1)), now, period, 120), None);
        // The defaults keep the old behaviour: a minute, an hour.
        let cfg = crate::config::Backup::default();
        assert_eq!(cfg.fallback_wait_secs, 60);
        assert_eq!(cfg.retry_wait_secs, 3600);
    }

    #[test]
    fn next_wait_counts_from_the_last_backup() {
        let now = chrono::Utc::now();
        let period = chrono::Duration::hours(24);
        // A backup an hour ago: about 23 hours hence.
        let wait = next_wait(Some(now - chrono::Duration::hours(1)), now, period, 60);
        assert!(wait > std::time::Duration::from_secs(22 * 3600));
        assert!(wait <= std::time::Duration::from_secs(23 * 3600));
        // None yet: soon after the first start.
        let wait = next_wait(None, now, period, 60);
        assert_eq!(wait, std::time::Duration::from_secs(600));
    }
}
