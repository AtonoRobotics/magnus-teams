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
mod tests {
    use crate::admin_app::tests::server_with;
    use crate::events::Ev;
    use crate::inbox::Answer;
    use axum::Router;
    use serde_json::json;

    /// An ask Auto Review raised for mo's action in an app, waiting.
    async fn ask(k: &crate::kernel::Kernel, id: &str, risk: &str) {
        let details = json!({
            "call_id": "c1", "tool": "github__merge_pull_request", "input": { "pr": 7 }, "input_hash": "h1",
            "risk": risk, "app": "github", "app_title": "GitHub", "audience": "internal",
            "ask_destinations": [], "reason": "outside_request", "why": "beyond the request", "rule": crate::asks::OUTSIDE_REQUEST,
        });
        let ev = Ev::Item {
            id: id.into(),
            item_kind: "approval".into(),
            task: Some("t1".into()),
            run: None,
            person: "mo".into(),
            summary: "Magnus wants to merge pull request in GitHub.".into(),
            details,
        };
        k.emit("kernel", ev).await.unwrap();
    }

    /// Handle on an ask sets mo's grant from the form and allows the rest
    /// of the task in the same decision; it is not offered where Allow
    /// always is, and no one else answers it.
    #[tokio::test]
    async fn handling_an_ask_sets_the_grant_and_allows_the_task() {
        let s = server_with("", |_| Router::new()).await;
        let mo = s.k.person("mo").unwrap();
        let sam = s.k.person("sam").unwrap();
        ask(&s.k, "item_h", "irreversible").await;
        let grant = json!({ "title": "Merge my pull requests", "apps": ["github"], "allow": ["irreversible"], "period": "week", "max_usd": 2.0 });
        let body = json!({ "decision": "approve", "scope": "handle", "grant": grant });

        // The card offers it, with the form's starting values.
        let mut rows = s.k.read(|st| vec![crate::inbox::render(st, &mo, &st.inbox.items()["item_h"])]);
        assert!(rows[0]["ask"]["offers"].as_array().unwrap().contains(&json!("handle")), "{}", rows[0]["ask"]);
        s.k.handle_prefill("mo", &mut rows).await;
        let h = &rows[0]["ask"]["handle"];
        assert_eq!((h["apps"].clone(), h["allow"].clone()), (json!(["github"]), json!(["irreversible", "send"])), "{h}");
        assert!(h["your_apps"].as_array().unwrap().iter().any(|a| a["name"] == "github"), "{h}");

        assert!(matches!(s.k.respond(&sam, "item_h", &body).await.unwrap(), Answer::NotFound | Answer::Forbidden(_)), "someone else handled mo's ask");
        let half = json!({ "decision": "approve", "scope": "handle", "grant": { "title": "x" } });
        assert!(matches!(s.k.respond(&mo, "item_h", &half).await.unwrap(), Answer::Invalid(_)));
        assert!(s.k.read(|st| st.arrangements.grants().is_empty()), "a refused Handle set a grant");

        assert!(matches!(s.k.respond(&mo, "item_h", &body).await.unwrap(), Answer::Ok));
        let (grants, approvals, resolved) =
            s.k.read(|st| (st.arrangements.grants().values().cloned().collect::<Vec<_>>(), st.inbox.approvals().clone(), st.inbox.items()["item_h"].resolution.clone()));
        assert_eq!(grants.len(), 1);
        assert_eq!((grants[0].person.as_str(), grants[0].apps.clone(), grants[0].allow.clone()), ("mo", vec!["github".to_string()], vec!["irreversible".to_string()]));
        let ap = approvals.iter().find(|a| a.task == "t1" && a.tool == "github__merge_pull_request").expect("no approval for the handled ask");
        assert_eq!((ap.scope.as_str(), ap.by.as_str()), ("task", "person:mo"), "Handle did not allow the rest of the task");
        assert_eq!(resolved.map(|r| (r.0, r.1)), Some(("approve".to_string(), Some("handle".to_string()))));

        // Where Allow always is offered (what can be undone), Handle is not.
        ask(&s.k, "item_r", "reversible_write").await;
        let row = s.k.read(|st| crate::inbox::render(st, &mo, &st.inbox.items()["item_r"]));
        assert!(!row["ask"]["offers"].as_array().unwrap().contains(&json!("handle")), "{}", row["ask"]);
        assert!(matches!(s.k.respond(&mo, "item_r", &body).await.unwrap(), Answer::Invalid(_)), "Handle was taken where it is not offered");
        s.done().await;
    }
}
