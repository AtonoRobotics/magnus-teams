//! One finish step for a tool's answer, whichever try produced it: the
//! first, or the resend after a first try whose outcome was lost. The
//! vault secret is masked, login codes are taken out where the app's answers
//! lose them, attachments are held back, an irreversible action is read
//! back, and the text is labelled by where it came from
//! (`decisions/2026-11-23-one-finish-step-for-a-tools-answer.md`).

use super::record::Record;
use super::{is_chat_tool, Action, Facts, Shown, Verdict};
use crate::kernel::Kernel;
use crate::tool_registry::Taint;
use crate::tools::Label;
use crate::vault::redact;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

/// What a tool answered, as the call returned it.
pub(super) struct Answer {
    pub text: String,
    pub is_error: bool,
    pub data: Option<Value>,
    /// The label and sources it was read under when the read itself says
    /// (memory_search, from the entries it returns); None: decided here.
    pub taint: Option<(Label, Vec<String>)>,
}

impl Answer {
    pub(super) fn of((text, is_error, data): (String, bool, Option<Value>)) -> Answer {
        Answer { text, is_error, data, taint: None }
    }
}

impl Kernel {
    /// Finish an answer and record it as the action's result.
    pub(super) async fn finish(self: &Arc<Self>, a: &Action, f: &Facts, shown: Option<&Shown>, secret: Option<&str>, key: String, answer: Answer) -> Result<Verdict> {
        let Answer { text, is_error, data, taint } = answer;
        let (label, sources) = match taint {
            Some(t) => t,
            None => self.answer_taint(a, f, data.as_ref()),
        };
        let clean = |t: &str| secret.map_or_else(|| t.to_string(), |s| redact(t, s));
        let codes = f.connector.as_ref().is_some_and(|c| c.filters_login_codes(&a.tool));
        let strip = |t: String| if codes { crate::login_codes::strip(&t) } else { t };
        let (text, data) = crate::attachment::lift(&a.run, codes, text, data, |t| strip(clean(&t)));
        let text = match crate::read_back::applies(a, f, is_error) {
            true => format!("{text}\n\n{}", self.read_back(a, f, shown, data.as_ref()).await?),
            false => text,
        };
        let status = if is_error { "failed" } else { "done" };
        self.record_from(a, Some(key), sources, Record { status, text, is_error, label, data }).await
    }

    /// The label and sources of an answer, by the tool's taint and what it
    /// returned (M1, M2).
    fn answer_taint(&self, a: &Action, f: &Facts, data: Option<&Value>) -> (Label, Vec<String>) {
        let d = || data.cloned().unwrap_or_default();
        match crate::tool_registry::taint(&a.tool) {
            // What the search returned decides; a result that says nothing is untrusted and private.
            // A read may name where its private data is from (a shared project's own, `team::project_source`).
            Taint::Returned => {
                let source = d()[crate::team::SOURCE_KEY].as_str().map_or_else(|| f.app.clone(), str::to_string);
                (Label { untrusted: d()["untrusted"] != false, private: d()["private"] != false }, vec![source])
            }
            // A reference file carries its own taint (M1, M2).
            Taint::ProjectFile => (self.project_file(&a.run, a.input["name"].as_str().unwrap_or_default()).map_or(Label::PRINCIPAL, |f| f.label), vec![f.app.clone()]),
            // The summaries it lists carry what their runs read.
            Taint::Listed if is_chat_tool(f) => self.listed_taint(&d()),
            _ => (f.label, vec![f.app.clone()]),
        }
    }
}
