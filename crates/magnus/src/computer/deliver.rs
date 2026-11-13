//! A file the agent made on its computer, given to the person. The agent
//! works in two places: the run's sandbox and its persistent computer, where
//! the apps it installed run (Blender, FreeCAD). `deliver_file` used to read
//! only the sandbox, so work made on the computer could not reach the person
//! (owner, 2026-11-13: a CAD package stayed at a path on the computer). Now
//! a delivery reads from the computer when the file is there.
use super::service::user_path;
use crate::kernel::Kernel;
use base64::Engine;
use serde_json::{json, Value};

/// The most one delivery carries, in bytes.
pub(crate) const MAX_DELIVER_BYTES: usize = 25 * 1024 * 1024;
/// Read in pieces this size, in bytes, so no one reply carries the whole file.
const READ_PIECE_BYTES: u64 = 4 * 1024 * 1024;

/// Where a delivery reads from: as the agent said, else where the run has
/// been working (its computer when it used the computer and not the
/// sandbox), and the other place when the file is not in the first.
pub(crate) fn places(input: &Value, used_computer: bool, used_sandbox: bool) -> [Place; 2] {
    let first = match input["from"].as_str() {
        Some("computer") => Place::Computer,
        Some("sandbox") => Place::Sandbox,
        _ if used_computer && !used_sandbox => Place::Computer,
        _ => Place::Sandbox,
    };
    [first, first.other()]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Place {
    Sandbox,
    Computer,
}

impl Place {
    fn other(self) -> Place {
        match self {
            Place::Sandbox => Place::Computer,
            Place::Computer => Place::Sandbox,
        }
    }

    pub(crate) fn said(self) -> &'static str {
        match self {
            Place::Sandbox => "your sandbox",
            Place::Computer => "your computer",
        }
    }
}

impl Kernel {
    /// The file a delivery names, and where it was found: in the order
    /// `places` gives, None when it is in neither.
    pub(crate) async fn delivered_bytes(&self, run: &str, path: &str, input: &Value) -> anyhow::Result<Option<(Vec<u8>, Place)>> {
        let tools: Vec<String> = self.read(|s| s.runs.get(run).map(|r| r.intents.values().map(|i| i.tool.clone()).collect()).unwrap_or_default());
        let used_computer = tools.iter().any(|t| t.starts_with("computer_"));
        let used_sandbox = tools.iter().any(|t| t.starts_with("sandbox_"));
        for place in places(input, used_computer, used_sandbox) {
            let got = match place {
                Place::Sandbox => self.sandboxes.read(run, path).await.map_err(|e| e.to_string()),
                Place::Computer => self.computer_file_bytes(run, path).await,
            };
            match got {
                Ok(b) => return Ok(Some((b, place))),
                Err(e) => tracing::debug!("deliver {path} from {}: {e}", place.said()),
            }
        }
        Ok(None)
    }

    /// A file on the run's computer, under /workspace. Err says why it could
    /// not be read: no computer, a path outside the person's files, no such
    /// file, or too large.
    pub(crate) async fn computer_file_bytes(&self, run: &str, path: &str) -> Result<Vec<u8>, String> {
        let r = self.read(|s| s.runs.get(run).cloned()).ok_or("no such run")?;
        let id = self.computer_of_run(&r).ok_or("there is no computer")?;
        let c = self.read(|s| s.computers.get(&id).cloned()).ok_or("there is no computer")?;
        let path = user_path(path).map_err(|_| format!("{path} is not under /workspace/ on your computer; copy it there first"))?;
        let mut out = vec![];
        loop {
            let v = self.computer_file_for(&c, json!({ "op": "read", "path": path, "offset": out.len(), "length": READ_PIECE_BYTES })).await.map_err(|e| format!("{e:?}"))?;
            if let Some(e) = v["error"].as_str() {
                return Err(e.to_string());
            }
            let piece = base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap_or_default()).map_err(|e| e.to_string())?;
            let short = (piece.len() as u64) < READ_PIECE_BYTES;
            out.extend(piece);
            if out.len() > MAX_DELIVER_BYTES {
                return Err(format!("{path} is larger than {} MB: deliver it in parts, or fewer files at once", MAX_DELIVER_BYTES / 1024 / 1024));
            }
            if short {
                return Ok(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivery_reads_first_where_the_run_has_been_working() {
        assert_eq!(places(&json!({}), true, false), [Place::Computer, Place::Sandbox], "work made on the computer was looked for in the sandbox first");
        assert_eq!(places(&json!({}), false, true), [Place::Sandbox, Place::Computer]);
        assert_eq!(places(&json!({}), true, true), [Place::Sandbox, Place::Computer]);
        assert_eq!(places(&json!({ "from": "computer" }), false, true), [Place::Computer, Place::Sandbox], "the agent's own word was not followed");
        assert_eq!(places(&json!({ "from": "sandbox" }), true, false), [Place::Sandbox, Place::Computer]);
    }
}
