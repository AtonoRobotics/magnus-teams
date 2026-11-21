//! "Handle things like this" on an approval card (owner, 2026-11-19;
//! decisions/2026-11-19-handle-things-like-this-on-an-ask.md). Where Allow
//! always is not offered on an ask Auto Review raised (what cannot be
//! undone), the person may set a grant for this kind of their work from the
//! card itself: the grant form opens in place, starting from the task's
//! title, the action's app and its exclusion lifted. Choosing it sets the
//! grant and allows the waiting action and the rest of this task, as Allow
//! for this task does, in one write (`inbox::respond`). Later tasks are
//! covered only as any grant covers them: when they are started under it
//! (`create_task` with `grant`), which the chat's system context lists.

use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use crate::state::Item;
use crate::tools::Label;
use serde_json::{json, Value};

impl Kernel {
    /// The title of an ask's task, as the grant form offers it; empty when
    /// it cannot be recalled.
    async fn ask_task_title(&self, task: Option<&str>) -> String {
        let Some((t, run)) = self.read(|s| task.and_then(|id| s.tasks.get(id)).map(|t| (t.clone(), s.runs.get(&t.run_id).cloned()))) else { return String::new() };
        self.task_title(&t, run.as_ref()).await.unwrap_or_else(|e| {
            tracing::warn!("recall the title of {} for its card: {e:#}", t.task_id);
            String::new()
        })
    }

    /// The grant form's starting values on each of `person`'s cards that
    /// offer Handle: the task's title, a budget of the server's limit for
    /// one task per week, and the person's apps to choose from. A card whose
    /// action is not in one of their apps (a program on their computer, an
    /// app since removed) offers no Handle: no grant could cover it.
    pub(crate) async fn handle_prefill(&self, person: &str, items: &mut [Value]) {
        let apps = self.grant_apps(person);
        let names: Vec<String> = apps.as_array().into_iter().flatten().filter_map(|a| a["name"].as_str().map(String::from)).collect();
        for i in items.iter_mut().filter(|i| i["person"] == person && i["ask"]["handle"].is_object()) {
            let app = i["ask"]["handle"]["apps"][0].as_str().unwrap_or_default().to_string();
            if !names.contains(&app) {
                if let Some(offers) = i["ask"]["offers"].as_array_mut() {
                    offers.retain(|o| o != "handle");
                }
                i["ask"]["handle"] = Value::Null;
                continue;
            }
            let task = i["task_id"].as_str().map(String::from);
            let title = self.ask_task_title(task.as_deref()).await;
            let h = &mut i["ask"]["handle"];
            h["title"] = json!(title);
            h["max_usd"] = json!(self.cfg.budget.max_usd);
            h["your_apps"] = apps.clone();
        }
    }

    /// The grant "Handle things like this" sets from `item`, from the
    /// person's fields in `body["grant"]`, checked as any grant's are; or
    /// why not. Only on a card that offers it, to the person whose work it
    /// is. A title left as the card offered it is the task's words, carrying
    /// the run's taint, as a grant card's did; one the person wrote is theirs.
    pub(crate) async fn grant_from_ask(&self, p: &Person, item: &Item, body: &Value) -> anyhow::Result<Result<Ev, String>> {
        let d = &item.details;
        let reason = crate::asks::Reason::of(d["rule"].as_str());
        let dests = crate::asks::item_destinations(d);
        let offered =
            crate::asks::handle_offered(reason, d["risk"].as_str().unwrap_or_default(), &dests, d["tool"].as_str().unwrap_or_default(), d["app"].as_str().unwrap_or_default());
        if p.id != item.person || !offered {
            return Ok(Err("Handle things like this is not offered for this".into()));
        }
        let f = match self.grant_fields(p, &body["grant"]) {
            Ok(f) => f,
            Err(why) => return Ok(Err(why)),
        };
        let title = self.ask_task_title(item.task.as_deref()).await;
        let label =
            if f.title == title.trim() { self.read(|s| item.run.as_ref().and_then(|r| s.runs.get(r)).map(|r| Label { untrusted: r.untrusted, private: r.private })) } else { None };
        let ev = crate::grants::grant_set(&p.id, &crate::kernel::new_id("grant"), &f, None, label)?;
        Ok(Ok(ev))
    }
}

#[cfg(test)]
#[path = "ask_grants_tests.rs"]
mod tests;
