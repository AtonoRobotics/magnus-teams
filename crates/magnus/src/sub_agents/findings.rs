//! Rendering composed deliverables and full sub-agent findings so parent
//! agents, chat transcripts, lane watchers, and task inspection see concrete
//! results instead of bare meta-summaries.
use crate::kernel::Kernel;
use crate::state::Run;
use serde_json::Value;

/// Renders a composed `outcome_view` (deliverable JSON array) into readable
/// plain text / markdown so parent agents, transcripts, lane watchers, and
/// search see the full substantive findings rather than a thin meta-summary.
pub(crate) fn render_deliverable(view: &Value) -> String {
    let Some(items) = view.as_array() else { return String::new() };
    let mut parts = Vec::new();
    for item in items {
        let kind = item["type"].as_str().unwrap_or_default();
        let title = item["title"].as_str().map(str::trim).filter(|s| !s.is_empty());
        let mut block = String::new();
        if let Some(t) = title {
            block.push_str(&format!("### {t}\n"));
        }
        match kind {
            "text" | "markdown" | "callout" | "note" => {
                if let Some(t) = item["text"].as_str().or_else(|| item["body"].as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                    block.push_str(t);
                }
            }
            "stat" => {
                let label = item["label"].as_str().unwrap_or_default();
                let val = item["value"].as_str().map(str::to_string).unwrap_or_else(|| item["value"].to_string());
                let delta = item["delta"].as_str().or_else(|| item["note"].as_str()).unwrap_or_default();
                if delta.is_empty() {
                    block.push_str(&format!("- **{label}**: {val}"));
                } else {
                    block.push_str(&format!("- **{label}**: {val} ({delta})"));
                }
            }
            "table" => {
                let cols: Vec<String> = item["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
                if !cols.is_empty() {
                    block.push_str(&format!("| {} |\n| {} |\n", cols.join(" | "), vec!["---"; cols.len()].join(" | ")));
                }
                for row in item["rows"].as_array().into_iter().flatten() {
                    if let Some(arr) = row.as_array() {
                        let cells: Vec<String> = arr.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).collect();
                        block.push_str(&format!("| {} |\n", cells.join(" | ")));
                    }
                }
            }
            "comparison" => {
                for opt in item["options"].as_array().into_iter().flatten() {
                    let name = opt["name"].as_str().or_else(|| opt["title"].as_str()).unwrap_or("Option");
                    let desc = opt["summary"].as_str().or_else(|| opt["description"].as_str()).unwrap_or_default();
                    block.push_str(&format!("- **{name}**"));
                    if !desc.is_empty() {
                        block.push_str(&format!(": {desc}"));
                    }
                    if let Some(pros) = opt["pros"].as_array() {
                        let p: Vec<&str> = pros.iter().filter_map(Value::as_str).collect();
                        if !p.is_empty() {
                            block.push_str(&format!(" | Pros: {}", p.join("; ")));
                        }
                    }
                    if let Some(cons) = opt["cons"].as_array() {
                        let c: Vec<&str> = cons.iter().filter_map(Value::as_str).collect();
                        if !c.is_empty() {
                            block.push_str(&format!(" | Cons: {}", c.join("; ")));
                        }
                    }
                    block.push('\n');
                }
            }
            "timeline" => {
                for ev in item["events"].as_array().or_else(|| item["items"].as_array()).into_iter().flatten() {
                    let when = ev["date"].as_str().or_else(|| ev["at"].as_str()).unwrap_or_default();
                    let what = ev["label"].as_str().or_else(|| ev["title"].as_str()).or_else(|| ev["text"].as_str()).unwrap_or_default();
                    let detail = ev["detail"].as_str().or_else(|| ev["description"].as_str()).unwrap_or_default();
                    block.push_str(&format!("- {when}: {what}"));
                    if !detail.is_empty() {
                        block.push_str(&format!(" — {detail}"));
                    }
                    block.push('\n');
                }
            }
            "code" => {
                let lang = item["language"].as_str().unwrap_or_default();
                let code = item["code"].as_str().or_else(|| item["text"].as_str()).unwrap_or_default();
                block.push_str(&format!("```{lang}\n{code}\n```"));
            }
            "file" | "image" => {
                let name = item["file"].as_str().or_else(|| item["name"].as_str()).unwrap_or("file");
                let caption = item["caption"].as_str().unwrap_or_default();
                if caption.is_empty() {
                    block.push_str(&format!("[Delivered {kind}: {name}]"));
                } else {
                    block.push_str(&format!("[Delivered {kind}: {name} — {caption}]"));
                }
            }
            "bar_chart" | "line_chart" | "map" => {
                if let Ok(compact) = serde_json::to_string(item) {
                    block.push_str(&compact);
                }
            }
            _ => {
                if let Some(t) = item["text"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    block.push_str(t);
                } else if let Ok(compact) = serde_json::to_string(item) {
                    block.push_str(&compact);
                }
            }
        }
        let trimmed = block.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    parts.join("\n\n")
}

/// Formats a finished child run's full findings (deliverable, summary,
/// delivered files, cited sources, and action receipts) so the parent
/// agent receives actual information rather than a bare summary.
pub(crate) fn format_child_findings(k: &Kernel, r: &Run) -> String {
    let mut sections = Vec::new();
    let deliverable = r.outcome_view.as_ref().map(render_deliverable).unwrap_or_default();
    let has_deliverable = !deliverable.is_empty();
    if has_deliverable {
        sections.push(deliverable);
    }
    if let Some(summary) = r.summary.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let already_in = sections.first().is_some_and(|d| d.contains(summary));
        if !already_in {
            if sections.is_empty() {
                sections.push(summary.to_string());
            } else {
                sections.push(format!("Summary: {summary}"));
            }
        }
    }
    if !has_deliverable && r.status == crate::state::Status::Failed {
        if let Some(partial) = r.replies.iter().rev().map(|rep| rep.text()).find(|t| !t.trim().is_empty()) {
            let trimmed = partial.trim();
            if !sections.iter().any(|s| s.contains(trimmed)) {
                sections.push(format!("Partial findings before stopping:\n{trimmed}"));
            }
        }
    }
    if !r.outputs.is_empty() {
        let files: Vec<String> = r.outputs.iter().map(|(_, name, mt, _)| format!("{name} ({mt})")).collect();
        sections.push(format!("Files delivered: {}", files.join(", ")));
    }
    let evidence = k.evidence_of(r);
    if let Some(receipts) = evidence["receipts"].as_array().filter(|a| !a.is_empty()) {
        let lines: Vec<String> = receipts
            .iter()
            .filter_map(|rc| {
                let what = rc["what"].as_str()?;
                let to = rc["to"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                Some(if to.is_empty() { format!("- {what}") } else { format!("- {what} (to {to})") })
            })
            .collect();
        if !lines.is_empty() {
            sections.push(format!("Actions taken:\n{}", lines.join("\n")));
        }
    }
    if let Some(sources) = evidence["sources"].as_array().filter(|a| !a.is_empty()) {
        let lines: Vec<String> = sources
            .iter()
            .filter_map(|src| {
                let claim = src["claim"].as_str().unwrap_or_default();
                let at = src["at"].as_str().or_else(|| src["tool"].as_str()).unwrap_or_default();
                if claim.is_empty() && at.is_empty() {
                    None
                } else if at.is_empty() {
                    Some(format!("- {claim}"))
                } else if claim.is_empty() {
                    Some(format!("- {at}"))
                } else {
                    Some(format!("- {claim} ({at})"))
                }
            })
            .collect();
        if !lines.is_empty() {
            sections.push(format!("Sources:\n{}", lines.join("\n")));
        }
    }
    if sections.is_empty() {
        "(it left no summary)".to_string()
    } else {
        sections.join("\n\n")
    }
}
