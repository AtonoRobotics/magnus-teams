//! Tool definitions pinned on first sight (B10): split from `run/mod.rs`,
//! which may only shrink.

use crate::events::Ev;
use crate::kernel::{new_id, Kernel};
use anyhow::Result;
use serde_json::json;

impl Kernel {
    /// Pin a tool definition on first sight; disable it if it later changes (B10).
    /// Returns true when the definition was seen for the first time.
    ///
    /// A brokered connector's definitions come from the image its manifest
    /// names by digest, checked before every boot, so they cannot change
    /// after the manifest was signed: one that differs from its pin is the
    /// next manifest version's (connectors spec: "by the next manifest
    /// version"), seen for the first time, and any change waiting on the
    /// owner for it is put away. A remote server's can change at any time.
    pub(crate) async fn pin(&self, c: &crate::config::Connector, name: &str, t: &crate::apps::AppTool) -> Result<bool> {
        // Runs that start together list the same tools together. Without
        // this, each read "not pinned yet" before any pinned it, and each
        // put its own review (or change) before the owner.
        static PINNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _one_at_a_time = PINNING.lock().await;
        let hash = crate::apps::definition_hash(t);
        let (pinned, changed) = self.read(|s| (s.pins.get(name).map(|p| p.hash.clone()), s.changed.get(name).map(|(item, p)| (item.clone(), p.hash.clone()))));
        let brokered = c.transport == crate::brokered::BROKERED;
        match pinned {
            Some(h) if brokered && h != hash => {
                if let Some((item, _)) = changed {
                    if self.read(|s| s.items.get(&item).is_some_and(|i| i.resolution.is_none())) {
                        let text = format!(
                            "{name}'s definition is now the one in the image {}'s manifest names by digest, so it is pinned as new and awaits only your review of its description.",
                            c.name
                        );
                        self.emit("kernel", Ev::ItemResolved { id: item, decision: "superseded".into(), scope: None, text: Some(text) }).await?;
                    }
                }
                self.emit("kernel", Ev::ToolPinned { tool: name.into(), hash, description: t.description.clone(), input_schema: t.input_schema.clone(), approved: false }).await?;
                return Ok(true);
            }
            None => {
                self.emit("kernel", Ev::ToolPinned { tool: name.into(), hash, description: t.description.clone(), input_schema: t.input_schema.clone(), approved: false }).await?;
                return Ok(true);
            }
            Some(h) if h == hash => {}
            Some(_) if changed.as_ref().map(|(_, h)| h.as_str()) == Some(hash.as_str()) => {}
            Some(_) => {
                let owner = self.owners().first().map(|p| p.id.clone()).unwrap_or_default();
                let item = new_id("item");
                // The new description is third-party text: it goes to the owner
                // for review, never to the model.
                self.emit(
                    "kernel",
                    Ev::Item {
                        id: item.clone(),
                        item_kind: "tool_change".into(),
                        task: None,
                        run: None,
                        person: owner,
                        summary: format!("The definition of {name} changed. It is disabled until you approve the new one."),
                        details: json!({ "tool": name, "hash": hash, "description": t.description, "input_schema": t.input_schema }),
                    },
                )
                .await?;
                self.emit("kernel", Ev::ToolChanged { tool: name.into(), hash, description: t.description.clone(), input_schema: t.input_schema.clone(), item }).await?;
            }
        }
        Ok(false)
    }
}
