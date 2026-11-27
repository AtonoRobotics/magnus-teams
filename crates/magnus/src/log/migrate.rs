//! Event log and content store schema migrations.

use super::{connect, ident, split};
use anyhow::{Context, Result};
use tokio_postgres::Client;

/// A URL's password as Postgres is to store it: percent-decoded, as the
/// kernel sends it when it connects, and quoted for a SQL string.
fn sql_password(in_url: &str) -> Result<String> {
    let pw = percent_encoding::percent_decode_str(in_url).decode_utf8().context("the database password is not UTF-8")?;
    Ok(pw.replace('\'', "''"))
}

/// Create the log table and the kernel role named in `kernel_url`, and,
/// when the admin console is set up, its root role named in `root_url`.
pub async fn migrate(admin_url: &str, kernel_url: &str, root_url: Option<&str>) -> Result<()> {
    let k = url::Url::parse(kernel_url).context("database.url")?;
    let role = ident(k.username())?.to_string();
    let password = sql_password(k.password().context("database.url has no password")?)?;
    let db = ident(k.path().trim_start_matches('/'))?.to_string();
    let admin = connect(admin_url).await?;
    let exists = admin.query_opt("SELECT 1 FROM pg_roles WHERE rolname = $1", &[&role]).await?.is_some();
    // A role made beforehand keeps whatever it was given, so its attributes
    // are set either way; what it may still do to the log is checked below.
    let verb = if exists { "ALTER" } else { "CREATE" };
    admin.batch_execute(&format!("{verb} ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS")).await?;
    admin
        .batch_execute(&format!(
            "CREATE TABLE IF NOT EXISTS public.events (
                seq    bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                at     timestamptz NOT NULL DEFAULT now(),
                kind   text NOT NULL,
                actor  text NOT NULL,
                run_id text,
                body   jsonb NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_run ON public.events (run_id, seq);
             REVOKE ALL ON public.events FROM PUBLIC;
             REVOKE ALL ON public.events FROM {role};
             GRANT CONNECT ON DATABASE {db} TO {role};
             GRANT USAGE ON SCHEMA public TO {role};
             REVOKE CREATE ON SCHEMA public FROM PUBLIC;
             GRANT SELECT, INSERT ON public.events TO {role};
             CREATE TABLE IF NOT EXISTS public.content (
                seq  bigint PRIMARY KEY REFERENCES public.events (seq),
                salt bytea NOT NULL,
                body jsonb NOT NULL
             );
             REVOKE ALL ON public.content FROM PUBLIC;
             REVOKE ALL ON public.content FROM {role};
             GRANT SELECT, INSERT, DELETE ON public.content TO {role};"
        ))
        .await?;
    move_inline_content(&admin).await.context("move content written into the log to the content store")?;
    admin.batch_execute(&content_walls_sql(&role)).await.context("wall the content store by person")?;
    // The knowledge base's index, filtered for each reader by row security.
    admin.batch_execute(&crate::knowledge::migrate_sql(&role)).await.context("create the knowledge index (pgvector must be installed)")?;
    admin.batch_execute(&crate::computer::migrate_sql(&role)).await.context("create the computers' egress audit table")?;
    admin.batch_execute(&crate::accounts::migrate_sql(&role)).await.context("create the accounts tables")?;
    // Append-only is the database's to enforce: whatever grants or
    // memberships the role holds (pg_write_all_data, owning the table, a
    // role that does), it must not be able to change or delete the log.
    // A membership counts whether or not the role inherits it: a NOINHERIT
    // role holds nothing through it until it switches to it with SET ROLE
    // (#186). So every role it is a member of, directly or through others,
    // itself included, is checked as if the kernel were that role: none may
    // change the log, hold what the kernel was stripped of above, or reach
    // the server's files and programs (Postgres documents those three roles
    // as a way to superuser). Memberships are refused, not revoked, as an
    // inherited one always was: they were given on purpose by someone, and
    // taking them away is theirs to decide.
    let can: Vec<String> = admin
        .query(
            "SELECT rolname || ': ' || array_to_string(rights, ', ') FROM (
                SELECT r.rolname, array_remove(ARRAY[
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'UPDATE') THEN 'UPDATE' END,
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'DELETE') THEN 'DELETE' END,
                    CASE WHEN has_table_privilege(r.oid, 'public.events', 'TRUNCATE') THEN 'TRUNCATE' END,
                    CASE WHEN r.rolsuper THEN 'SUPERUSER' END,
                    CASE WHEN r.rolcreatedb THEN 'CREATEDB' END,
                    CASE WHEN r.rolcreaterole THEN 'CREATEROLE' END,
                    CASE WHEN r.rolreplication THEN 'REPLICATION' END,
                    CASE WHEN r.rolbypassrls THEN 'BYPASSRLS' END,
                    CASE WHEN r.rolname IN ('pg_read_server_files', 'pg_write_server_files', 'pg_execute_server_program') THEN 'server files' END
                ], NULL) AS rights
                FROM pg_roles r WHERE pg_has_role($1, r.oid, 'MEMBER')
             ) m WHERE cardinality(rights) > 0 ORDER BY rolname",
            &[&role],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    anyhow::ensure!(
        can.is_empty(),
        "the kernel's role {role} can change the log or act past its grants ({}), through a grant or a role it is a member of, inherited or by SET ROLE; take that away and run migrate again",
        can.join("; ")
    );
    if let Some(root) = root_url {
        migrate_root(&admin, root, &db).await.context("the admin console's database role")?;
    }
    Ok(())
}

/// Move the content of events written whole, before the content store
/// existed, out of the log and into the store, where trash and erasure can
/// delete it, leaving the record with the content's hash as `split` does for
/// every event now. Each event moves in one statement, so a crash leaves it
/// either whole or moved; events already moved, or without content, are left.
pub(crate) async fn move_inline_content(admin: &Client) -> Result<()> {
    // Only kinds that carry content, and only records without the hash a
    // split leaves: a record with it was split when written or moved
    // before, and its content, if gone, was deleted on purpose.
    let kinds: Vec<&str> = crate::events::CONTENT.iter().map(|(k, _)| *k).chain(crate::events::OLD_LANE_KINDS).collect();
    let mut after = 0i64;
    loop {
        let rows = admin
            .query(
                "SELECT e.seq, e.body FROM public.events e
                 WHERE e.seq > $1 AND e.kind = ANY($2) AND NOT (e.body ? 'content')
                 ORDER BY e.seq LIMIT 1000",
                &[&after, &kinds],
            )
            .await?;
        let Some(last) = rows.last() else { return Ok(()) };
        after = last.get(0);
        for r in &rows {
            let (record, Some(content)) = split(r.get(1)) else { continue };
            admin
                .execute(
                    "WITH moved AS (INSERT INTO public.content (seq, salt, body) VALUES ($1, $2, $3) RETURNING seq)
                     UPDATE public.events SET body = $4 WHERE seq = (SELECT seq FROM moved)",
                    &[&r.get::<_, i64>(0), &content.salt, &content.body, &record],
                )
                .await?;
        }
    }
}

/// The admin console's root role (admin console spec, Authority and audit):
/// it reads every person's content, past the walls between people
/// (BYPASSRLS), and the log, which it can add to but never change or
/// delete from. No other process holds it.
async fn migrate_root(admin: &Client, root_url: &str, db: &str) -> Result<()> {
    let r = url::Url::parse(root_url).context("admin.database_url")?;
    let role = ident(r.username())?.to_string();
    let password = sql_password(r.password().context("admin.database_url has no password")?)?;
    let exists = admin.query_opt("SELECT 1 FROM pg_roles WHERE rolname = $1", &[&role]).await?.is_some();
    let verb = if exists { "ALTER" } else { "CREATE" };
    admin.batch_execute(&format!("{verb} ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE BYPASSRLS")).await?;
    admin
        .batch_execute(&format!(
            "GRANT CONNECT ON DATABASE {db} TO {role};
             GRANT USAGE ON SCHEMA public TO {role};
             REVOKE ALL ON public.events, public.content, public.knowledge FROM {role};
             GRANT SELECT, INSERT ON public.events TO {role};
             GRANT SELECT ON public.content, public.knowledge TO {role};"
        ))
        .await?;
    Ok(())
}

/// The walls between people in the content store (owner's ruling 9 and
/// "recall only", 2026-11-01; `docs/proposals/recall-only.md`, step 1):
/// each row records the person it belongs to, and `{role}_person` reads
/// only the rows of the person in `magnus.person`, under a row policy,
/// whatever query it is given. The kernel's own role still reads every
/// row. The policy is not forced on the table's owner, the administrator
/// `migrate` runs as, which fills the column below. Rows written before the column existed are filled from their
/// events, by the same rule as `State::content_owner`.
pub fn content_walls_sql(role: &str) -> String {
    let reader = format!("{role}_person");
    let (owners, open_web, knowledge) = (CONTENT_OWNERS, crate::team::lanes::OPEN_WEB_OWNERS, crate::knowledge_owner::owners_sql());
    format!(
        "ALTER TABLE public.content ADD COLUMN IF NOT EXISTS person text;
         CREATE INDEX IF NOT EXISTS content_person ON public.content (person);
         DO $$ BEGIN CREATE ROLE {reader} NOLOGIN; EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$;
         DO $$ BEGIN IF NOT pg_has_role('{role}', '{reader}', 'MEMBER') THEN GRANT {reader} TO {role}; END IF; END $$;
         REVOKE ALL ON public.content FROM {reader};
         REVOKE ALL ON public.events FROM {reader};
         GRANT USAGE ON SCHEMA public TO {reader};
         GRANT SELECT ON public.content TO {reader};
         ALTER TABLE public.content ENABLE ROW LEVEL SECURITY;
         DROP POLICY IF EXISTS content_kernel ON public.content;
         CREATE POLICY content_kernel ON public.content FOR ALL TO {role} USING (true) WITH CHECK (true);
         DROP POLICY IF EXISTS content_person ON public.content;
         CREATE POLICY content_person ON public.content FOR SELECT TO {reader}
             USING (person IS NOT NULL AND person = NULLIF(current_setting('magnus.person', true), ''));
         {owners} {open_web} {knowledge}"
    )
}

/// Fill in whose each content row is, for rows that do not say: written
/// before the column existed, or restored from a backup.
pub const CONTENT_OWNERS: &str = "UPDATE public.content c SET person = o.person FROM (
    SELECT e.seq, CASE
        -- A knowledge entry is whose its readers now say: `knowledge_owner::owners_sql`, after this.
        WHEN e.kind = 'knowledge_added' THEN NULL
        WHEN e.kind = 'bundle_written' THEN e.body->>'owner_user'
        WHEN e.kind = 'item_resolved' THEN (SELECT i.body->>'person' FROM public.events i
            WHERE i.kind = 'item' AND i.body->>'id' = e.body->>'id' ORDER BY i.seq LIMIT 1)
        WHEN e.kind = 'schedule_ended' THEN (SELECT x.body->>'person' FROM public.events x
            WHERE x.kind = 'schedule_set' AND x.body->>'id' = e.body->>'id' ORDER BY x.seq LIMIT 1)
        -- `team::owner`: a message and its edits are the sender's, a
        -- request's are the requester's, a mark is its person's.
        WHEN e.kind = 'team' THEN NULLIF(CASE e.body->>'change'
            WHEN 'message' THEN regexp_replace(w.body->'message'->>'sender', '^agent:', '')
            WHEN 'note' THEN regexp_replace(w.body->'note'->>'by', '^agent:', '')
            WHEN 'run' THEN e.body->>'requester'
            WHEN 'whisper' THEN e.body->>'owner'
            WHEN 'coedit' THEN CASE w.body->'coedit'->>'step' WHEN 'proposed' THEN w.body->'coedit'->'author'->>'person'
                WHEN 'checked_out' THEN w.body->'coedit'->>'person' WHEN 'checked_in' THEN w.body->'coedit'->>'person' END
            WHEN 'chat' THEN CASE WHEN w.body->'chat'->>'what' = 'edited' THEN (SELECT regexp_replace(mc.body->'message'->>'sender', '^agent:', '')
                FROM public.events m JOIN public.content mc USING (seq) WHERE m.kind = 'team' AND m.body->>'change' = 'message'
                AND m.body->>'workspace' = e.body->>'workspace' AND m.body->>'channel' = w.body->'chat'->>'channel'
                AND mc.body->'message'->>'id' = w.body->'chat'->>'id' ORDER BY m.seq LIMIT 1) ELSE w.body->'chat'->>'person' END
            ELSE e.body->>'person' END, '')
        -- `MissionLedger::content_owner`: a draft its owner's, a release its releaser's, the rest the requester's.
        WHEN e.kind IN ('team_mission', 'member_agent', 'open_web', 'peer_agent') THEN COALESCE(e.body->>'requester', e.body->>'owner', e.body->>'released_by', (SELECT s.body->>'requester'
            FROM public.events s WHERE s.kind = 'team_mission' AND s.body->>'change' = 'started' AND s.body->>'mission' = e.body->>'mission' ORDER BY s.seq LIMIT 1))
        ELSE COALESCE(e.body->>'person', (SELECT t.body->>'person' FROM public.events t
            WHERE t.kind = 'task_created' AND t.run_id = e.run_id ORDER BY t.seq LIMIT 1))
    END AS person
    FROM public.events e JOIN public.content w USING (seq) WHERE w.person IS NULL
) o WHERE c.seq = o.seq AND o.person IS NOT NULL;";
