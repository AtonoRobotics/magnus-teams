//! The accounts tables: their schema, and the one-time import of the
//! JSON files they replaced.

use super::{key, Invite, LocalPerson, Role, Session, Status};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use tokio_postgres::Client;

/// The accounts tables, for the backup's table-scoped dump.
pub const TABLES: &[&str] = &[
    "account_people",
    "account_passwords",
    "account_totp",
    "account_totp_steps",
    "account_statuses",
    "account_roles",
    "account_invites",
    "account_reset_tokens",
    "account_sessions",
    "account_machine_codes",
    "account_sign_in_policy",
    "account_json_import",
];

/// The accounts tables, beside the log. The kernel reads and writes them;
/// no other role reaches them.
pub fn migrate_sql(role: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS public.account_people (
             person text PRIMARY KEY,
             name   text NOT NULL,
             email  text
         );
         CREATE TABLE IF NOT EXISTS public.account_passwords (
             person text PRIMARY KEY,
             phc    text NOT NULL
         );
         ALTER TABLE public.account_passwords ADD COLUMN IF NOT EXISTS temporary bool NOT NULL DEFAULT false;
         CREATE TABLE IF NOT EXISTS public.account_totp (
             person text PRIMARY KEY,
             secret text NOT NULL
         );
         CREATE TABLE IF NOT EXISTS public.account_totp_steps (
             person text PRIMARY KEY,
             step   bigint NOT NULL
         );
         CREATE TABLE IF NOT EXISTS public.account_statuses (
             person text PRIMARY KEY,
             status text NOT NULL CHECK (status IN ('invited', 'active', 'disabled'))
         );
         CREATE TABLE IF NOT EXISTS public.account_roles (
             person text PRIMARY KEY,
             role   text NOT NULL CHECK (role IN ('admin', 'member'))
         );
         CREATE TABLE IF NOT EXISTS public.account_invites (
             hash    text PRIMARY KEY,
             person  text NOT NULL,
             email   text,
             expires timestamptz NOT NULL
         );
         CREATE INDEX IF NOT EXISTS account_invites_person ON public.account_invites (person);
         CREATE TABLE IF NOT EXISTS public.account_reset_tokens (
             person    text PRIMARY KEY,
             hash      text NOT NULL,
             expires   timestamptz NOT NULL,
             requested timestamptz NOT NULL
         );
         CREATE TABLE IF NOT EXISTS public.account_sessions (
             hash    text PRIMARY KEY,
             id      text NOT NULL,
             person  text NOT NULL,
             created timestamptz NOT NULL,
             expires timestamptz NOT NULL,
             client  text NOT NULL,
             device  text
         );
         ALTER TABLE public.account_sessions ADD COLUMN IF NOT EXISTS seen timestamptz;
         CREATE INDEX IF NOT EXISTS account_sessions_person ON public.account_sessions (person);
         CREATE TABLE IF NOT EXISTS public.account_machine_codes (
             hash  text PRIMARY KEY,
             until timestamptz NOT NULL
         );
         CREATE TABLE IF NOT EXISTS public.account_sign_in_policy (
             id                     int PRIMARY KEY CHECK (id = 1),
             code_required          bool NOT NULL,
             idle_minutes           bigint NOT NULL,
             max_hours              bigint NOT NULL,
             lockout_enabled        bool NOT NULL,
             lockout_failures       bigint NOT NULL,
             lockout_wait_minutes   bigint NOT NULL,
             lockout_max_wait_minutes bigint NOT NULL,
             lockout_permanent      bool NOT NULL
         );
         CREATE TABLE IF NOT EXISTS public.account_json_import (
             imported_at timestamptz PRIMARY KEY
         );
         REVOKE ALL ON public.account_people, public.account_passwords, public.account_totp, public.account_totp_steps, public.account_statuses, public.account_roles, public.account_invites, public.account_reset_tokens, public.account_sessions, public.account_machine_codes, public.account_sign_in_policy, public.account_json_import FROM PUBLIC;
         GRANT SELECT, INSERT, UPDATE, DELETE ON public.account_people, public.account_passwords, public.account_totp, public.account_totp_steps, public.account_statuses, public.account_roles, public.account_invites, public.account_reset_tokens, public.account_sessions, public.account_machine_codes, public.account_sign_in_policy, public.account_json_import TO {role};"
    )
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).with_context(|| format!("{} does not parse", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

/// Bring the JSON files' contents into the tables, once: `magnus migrate`
/// runs this after creating them, so the cutover keeps everyone signing
/// in. Completion is recorded in the database itself, so later migrates
/// (every deploy runs one) never resurrect a removed person or an ended
/// session from the untouched files; expired sessions, invites and codes
/// are left behind. Returns how many rows it wrote.
pub async fn import_json(client: &mut Client, state_dir: &Path) -> Result<usize> {
    let tx = client.transaction().await?;
    tx.batch_execute("SELECT pg_advisory_xact_lock(hashtext('magnus-accounts-import'))").await?;
    if tx.query_opt("SELECT 1 FROM public.account_json_import LIMIT 1", &[]).await?.is_some() {
        tx.commit().await?;
        return Ok(0);
    }
    let d = state_dir.join("accounts");
    let mut n = 0usize;
    let now = Utc::now();
    let people: BTreeMap<String, LocalPerson> = read_json(&d.join("people.json"))?;
    for (person, p) in &people {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_people (person, name, email) VALUES ($1, $2, $3) ON CONFLICT (person) DO NOTHING", &[&person, &p.name, &p.email]).await?
            as usize;
    }
    let passwords: BTreeMap<String, String> = read_json(&d.join("passwords.json"))?;
    for (person, phc) in &passwords {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_passwords (person, phc) VALUES ($1, $2) ON CONFLICT (person) DO NOTHING", &[&person, phc]).await? as usize;
    }
    let totp: BTreeMap<String, String> = read_json(&d.join("totp.json"))?;
    for (person, secret) in &totp {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_totp (person, secret) VALUES ($1, $2) ON CONFLICT (person) DO NOTHING", &[&person, secret]).await? as usize;
    }
    let steps: BTreeMap<String, i64> = read_json(&d.join("totp-steps.json"))?;
    for (person, step) in &steps {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_totp_steps (person, step) VALUES ($1, $2) ON CONFLICT (person) DO NOTHING", &[&person, step]).await? as usize;
    }
    let statuses: BTreeMap<String, Status> = read_json(&d.join("status.json"))?;
    for (person, status) in &statuses {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_statuses (person, status) VALUES ($1, $2) ON CONFLICT (person) DO NOTHING", &[&person, &status.as_str()]).await? as usize;
    }
    let roles: BTreeMap<String, Role> = read_json(&d.join("roles.json"))?;
    for (person, role) in &roles {
        let person = key(person);
        n += tx.execute("INSERT INTO public.account_roles (person, role) VALUES ($1, $2) ON CONFLICT (person) DO NOTHING", &[&person, &role.as_str()]).await? as usize;
    }
    let invites: BTreeMap<String, Invite> = read_json(&d.join("invites.json"))?;
    for (hash, inv) in invites.iter().filter(|(_, inv)| inv.expires > now) {
        let who = key(&inv.person);
        n += tx
            .execute(
                "INSERT INTO public.account_invites (hash, person, email, expires) VALUES ($1, $2, $3, $4) ON CONFLICT (hash) DO NOTHING",
                &[hash, &who, &inv.email, &inv.expires],
            )
            .await? as usize;
    }
    let sessions: BTreeMap<String, Session> = read_json(&d.join("sessions.json"))?;
    for (hash, s) in sessions.iter().filter(|(_, s)| s.expires > now) {
        let who = key(&s.person);
        n += tx
            .execute(
                "INSERT INTO public.account_sessions (hash, id, person, created, expires, client, device) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (hash) DO NOTHING",
                &[hash, &s.id, &who, &s.created, &s.expires, &s.client, &s.device],
            )
            .await? as usize;
    }
    let codes: BTreeMap<String, DateTime<Utc>> = read_json(&d.join("machine-codes.json"))?;
    for (hash, until) in codes.iter().filter(|(_, until)| **until > now) {
        n += tx.execute("INSERT INTO public.account_machine_codes (hash, until) VALUES ($1, $2) ON CONFLICT (hash) DO NOTHING", &[hash, until]).await? as usize;
    }
    tx.execute("INSERT INTO public.account_json_import (imported_at) VALUES ($1)", &[&now]).await?;
    tx.commit().await?;
    Ok(n)
}
