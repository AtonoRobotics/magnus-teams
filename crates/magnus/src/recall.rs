//! Recall: a person's content read from the content store when a request
//! needs it, through the per-person role, never taken from what the kernel
//! holds (owner, 2026-11-01: "recall only"; `docs/proposals/recall-only.md`,
//! step 2). Each read runs in a transaction as `{role}_person` with
//! `magnus.person` set to the person it is for, so row-level security
//! returns only that person's rows: a read asked for the wrong person
//! returns nothing, not someone else's content.

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use tokio_postgres::Client;

pub struct Recall {
    url: String,
    reader: String,
    client: tokio::sync::Mutex<Option<Client>>,
}

impl Recall {
    pub fn new(url: &str) -> Recall {
        let role = url::Url::parse(url).map(|u| u.username().to_string()).unwrap_or_default();
        Recall { url: url.into(), reader: format!("{role}_person"), client: tokio::sync::Mutex::new(None) }
    }

    /// The content of these events that belongs to `person`, by event seq.
    /// Events whose content is not theirs, or was deleted, are not in it.
    pub async fn content(&self, person: &str, seqs: &[i64]) -> Result<HashMap<i64, Value>> {
        self.read(person, seqs, None).await
    }

    /// Only these fields of that content: what a view shows, without the
    /// rest (a task's attachments, a conversation).
    pub async fn fields(&self, person: &str, seqs: &[i64], fields: &[&str]) -> Result<HashMap<i64, Value>> {
        self.read(person, seqs, Some(fields)).await
    }

    async fn read(&self, person: &str, seqs: &[i64], fields: Option<&[&str]>) -> Result<HashMap<i64, Value>> {
        if seqs.is_empty() {
            return Ok(HashMap::new());
        }
        let mut g = self.client.lock().await;
        if g.as_ref().map_or(true, |c| c.is_closed()) {
            *g = Some(crate::log::connect(&self.url).await?);
        }
        let c = g.as_mut().context("recall connection")?;
        let tx = c.transaction().await?;
        tx.batch_execute(&format!("SET LOCAL ROLE {}", self.reader)).await.context("recall as the person (has `magnus migrate` run since this release was installed?)")?;
        tx.execute("SELECT set_config('magnus.person', $1, true)", &[&person]).await?;
        let rows = match fields {
            None => tx.query("SELECT seq, body FROM public.content WHERE seq = ANY($1)", &[&seqs]).await?,
            Some(f) => {
                let f: Vec<String> = f.iter().map(|x| x.to_string()).collect();
                tx.query(
                    "SELECT seq, COALESCE((SELECT jsonb_object_agg(k, body->k) FROM unnest($2::text[]) AS k WHERE body ? k), '{}'::jsonb) FROM public.content WHERE seq = ANY($1)",
                    &[&seqs, &f],
                )
                .await?
            }
        };
        tx.commit().await?;
        Ok(rows.iter().map(|r| (r.get::<_, i64>(0), r.get::<_, Value>(1))).collect())
    }
}

/// A remembered fact as it is shown: its record and what it says.
pub struct Remembered {
    pub entry: crate::state::MemoryEntry,
    pub fact: String,
    pub topic: Option<String>,
}

impl crate::kernel::Kernel {
    /// A person's remembered facts that `keep` selects, newest last, each
    /// with what it says recalled for them. A fact the content store does
    /// not give them (deleted, or not theirs) is left out.
    pub async fn remembered(&self, person: &str, keep: impl Fn(&crate::state::MemoryEntry) -> bool) -> Result<Vec<Remembered>> {
        let entries: Vec<crate::state::MemoryEntry> = self.read(|s| s.memory.iter().filter(|m| m.person == person && keep(m)).cloned().collect());
        let seqs: Vec<i64> = entries.iter().map(|m| m.seq).collect();
        let content = self.recall.content(person, &seqs).await?;
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                let c = content.get(&entry.seq)?;
                let fact = c["fact"].as_str()?.to_string();
                let topic = c["topic"].as_str().map(String::from);
                Some(Remembered { entry, fact, topic })
            })
            .collect())
    }
}

impl crate::kernel::Kernel {
    /// A library file's bytes, recalled for its person from the event that
    /// holds them. None when its content is gone or not theirs.
    pub async fn file_bytes(&self, f: &crate::state::LibFile) -> Result<Option<Vec<u8>>> {
        use base64::Engine;
        let Some(b64) = self.file_b64(f).await? else { return Ok(None) };
        Ok(Some(base64::engine::general_purpose::STANDARD.decode(b64)?))
    }

    /// A library file's content as base64, recalled as `file_bytes` is.
    pub async fn file_b64(&self, f: &crate::state::LibFile) -> Result<Option<String>> {
        let mut c = self.recall.content(&f.person, &[f.seq]).await?;
        Ok(c.remove(&f.seq).and_then(|body| body["data_b64"].as_str().map(String::from)))
    }

    /// The files a run's task was given, with their content recalled for
    /// the task's person, as `{name, media_type, data_b64}`.
    pub async fn given_files(&self, task: &str) -> Result<Vec<Value>> {
        let given: Vec<crate::state::LibFile> = self.read(|s| s.tasks.get(task).into_iter().flat_map(|t| &t.files).filter_map(|id| s.files.get(id)).cloned().collect());
        let mut out = vec![];
        for f in given {
            if let Some(data) = self.file_b64(&f).await? {
                out.push(serde_json::json!({ "name": f.name, "media_type": f.media_type, "data_b64": data }));
            }
        }
        Ok(out)
    }

    /// The search text of a person's text files (in lower case, up to what
    /// a page shows), recalled for them, by file id. Only when a search
    /// has words to match: a listing needs none.
    pub async fn file_texts(&self, person: &str, terms: &[String]) -> Result<HashMap<String, String>> {
        if terms.is_empty() {
            return Ok(HashMap::new());
        }
        let files: Vec<(String, i64, String)> = self.read(|s| s.files.values().filter(|f| f.person == person).map(|f| (f.id.clone(), f.seq, f.media_type.clone())).collect());
        let seqs: Vec<i64> = files.iter().map(|(_, seq, _)| *seq).collect();
        let content = self.recall.content(person, &seqs).await?;
        Ok(files
            .into_iter()
            .filter_map(|(id, seq, media)| content.get(&seq).map(|c| (id, crate::state::search_text(&media, c["data_b64"].as_str().unwrap_or_default()))))
            .collect())
    }
}

/// What a view or a step reads of a task's or a schedule's content.
const WORDS: &[&str] = &["text", "title", "scope"];

/// Recalled content by event seq, for building a view or a step: a seq
/// whose content is not there (deleted, or not its person's) reads as
/// nothing.
#[derive(Default, Debug)]
pub struct Words(HashMap<i64, Value>);

impl Words {
    /// The content of the event `seq` names, or null.
    pub fn of(&self, seq: i64) -> &Value {
        self.0.get(&seq).unwrap_or(&Value::Null)
    }

    /// A string field of that content, or "".
    pub fn str(&self, seq: i64, field: &str) -> &str {
        self.of(seq)[field].as_str().unwrap_or_default()
    }

    /// An optional string field of that content.
    pub fn opt(&self, seq: i64, field: &str) -> Option<String> {
        self.of(seq)[field].as_str().map(String::from)
    }

    /// What a task or schedule asked.
    pub fn text(&self, seq: i64) -> &str {
        self.str(seq, "text")
    }

    /// A task's title as shown: the one the model gave it, else the start
    /// of what the person wrote.
    pub fn title(&self, seq: i64) -> Option<String> {
        self.opt(seq, "title").filter(|t| !t.trim().is_empty())
    }
}

impl crate::kernel::Kernel {
    /// The content of these events, each recalled for the person it is
    /// named with, through their wall: one read per person.
    pub async fn words(&self, wanted: impl IntoIterator<Item = (String, i64)>) -> Result<Words> {
        let mut by: HashMap<String, Vec<i64>> = HashMap::new();
        for (person, seq) in wanted {
            by.entry(person).or_default().push(seq);
        }
        let mut out = HashMap::new();
        for (person, seqs) in by {
            out.extend(self.recall.fields(&person, &seqs, WORDS).await?);
        }
        Ok(Words(out))
    }

    /// The words of every task, each recalled for its person: for the
    /// developer views, which show every run.
    pub async fn all_words(&self) -> Result<Words> {
        let seqs: Vec<(String, i64)> = self.read(|s| s.tasks.values().map(|t| (t.person.clone(), t.seq)).collect());
        self.words(seqs).await
    }

    /// The words of a person's tasks and schedules, for a view of them.
    pub async fn person_words(&self, person: &str) -> Result<Words> {
        let seqs: Vec<(String, i64)> = self.read(|s| {
            let tasks = s.tasks.values().filter(|t| t.person == person).map(|t| t.seq);
            let schedules = s.schedules.values().filter(|x| x.person == person).map(|x| x.seq);
            tasks.chain(schedules).map(|seq| (person.to_string(), seq)).collect()
        });
        self.words(seqs).await
    }

    /// A schedule's title as shown: the one it was given, else the start
    /// of what it asks; recalled for its person.
    pub async fn schedule_title(&self, sc: &crate::state::Schedule) -> Result<String> {
        let w = self.words_of(&sc.person, sc.seq).await?;
        Ok(w.title(sc.seq).unwrap_or_else(|| w.text(sc.seq).chars().take(80).collect()))
    }

    /// A task's title as shown, recalled for its person: see `title_of`.
    pub async fn task_title(&self, t: &crate::state::Task, run: Option<&crate::state::Run>) -> Result<String> {
        let w = self.words_of(&t.person, t.seq).await?;
        Ok(crate::api::title_of(t, run, &w))
    }

    /// What the person asked a run's task, recalled for them.
    pub async fn asked(&self, run: &crate::state::Run) -> Result<String> {
        let Some((person, seq)) = self.read(|s| s.tasks.get(&run.task).map(|t| (t.person.clone(), t.seq))) else { return Ok(String::new()) };
        Ok(self.words_of(&person, seq).await?.text(seq).to_string())
    }

    /// One task's or schedule's words, recalled for its person.
    pub async fn words_of(&self, person: &str, seq: i64) -> Result<Words> {
        self.words([(person.to_string(), seq)]).await
    }
}
