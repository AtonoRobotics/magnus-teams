//! The memory summary older apps read (`GET /v1/memory/summary`).

use crate::kernel::Kernel;
use serde_json::{json, Value};

impl Kernel {
    /// `GET /v1/memory/summary`: the person's current facts, split by taint,
    /// and their playbook. The app now reads `/v1/memory` and `/v1/playbook`;
    /// this stays for apps built before that, until they update.
    pub async fn memory_summary_json(&self, person: &str) -> Value {
        let recalled = self.remembered(person, |_| true).await.unwrap_or_default();
        self.read(|s| {
            let mut clean_facts = Vec::new();
            let mut untrusted_facts = Vec::new();
            for r in recalled {
                let m = r.entry;
                let entry = json!({
                    "id": m.id,
                    "fact": r.fact,
                    "topic": r.topic,
                    "run": m.run,
                    "by": m.by.as_deref().unwrap_or("model"),
                    "at": m.at,
                    "label": m.label,
                    "sources": m.sources,
                });
                if m.label.untrusted {
                    untrusted_facts.push(entry);
                } else {
                    clean_facts.push(entry);
                }
            }
            let pb_ver = s.library.playbook_version();
            let playbook: Vec<Value> = s
                .playbook_at(person, pb_ver)
                .into_iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "text": p.text,
                        "addresses": p.addresses,
                        "label": p.label,
                        "added": p.added,
                    })
                })
                .collect();
            json!({
                "person": person,
                "clean_facts": clean_facts,
                "untrusted_facts": untrusted_facts,
                "playbook": playbook,
            })
        })
    }
}
