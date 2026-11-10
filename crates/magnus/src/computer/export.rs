//! A computer's files, leaving in bulk: what the person picked, or the whole
//! user volume with its app manifest, as one ordinary tar file.
use super::service::{user_path, USER_FILES};
use crate::api::who;
use crate::config::Person;
use crate::controls::Refused;
use crate::grants::answered;
use crate::kernel::Kernel;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;

/// Held in memory while it is packed, so it has a ceiling.
const MAX_EXPORT_BYTES: usize = 128 * 1024 * 1024;
const MAX_EXPORT_FILES: usize = 10_000;
const MAX_EXPORT_DEPTH: usize = 32;
const MAX_PICKED: usize = 1_000;
/// Inside the archive: the files, and beside them the apps to reinstall.
const FILES_FOLDER: &str = "files/";
const MANIFEST_NAME: &str = "apps.json";
const FILE_MODE: u32 = 0o644;

/// Every file under `roots`, read through `ask` (the guest's `list` and
/// `read`). A root ending in `/` is a folder and is walked; links are listed
/// by the guest as files and never followed into.
pub(super) async fn gather<F, Fut>(roots: &[String], mut ask: F) -> Result<Vec<(String, Vec<u8>)>, Refused>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, Refused>>,
{
    let mut todo: Vec<(String, usize)> = roots.iter().rev().map(|r| (r.clone(), 0)).collect();
    let (mut out, mut total) = (vec![], 0usize);
    while let Some((path, depth)) = todo.pop() {
        if depth > MAX_EXPORT_DEPTH {
            return Err(Refused::Invalid(format!("{path} is more than {MAX_EXPORT_DEPTH} folders deep")));
        }
        if path.ends_with('/') {
            let listed = ask(json!({ "op": "list", "path": path })).await?;
            let entries = listed["entries"].as_array().cloned().unwrap_or_default();
            for e in entries.iter().rev() {
                let name = e["name"].as_str().unwrap_or_default();
                if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                    continue;
                }
                let slash = if e["dir"] == json!(true) { "/" } else { "" };
                todo.push((format!("{path}{name}{slash}"), depth + 1));
            }
            continue;
        }
        let read = ask(json!({ "op": "read", "path": path })).await?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(read["data_b64"].as_str().unwrap_or_default()).map_err(|e| Refused::Failed(format!("{path}: {e}")))?;
        total += bytes.len();
        if total > MAX_EXPORT_BYTES || out.len() >= MAX_EXPORT_FILES {
            return Err(Refused::Invalid(format!("that is more than {} MB or {MAX_EXPORT_FILES} files at once: pick fewer", MAX_EXPORT_BYTES / 1024 / 1024)));
        }
        out.push((path, bytes));
    }
    Ok(out)
}

/// An ordinary tar file of `files`, each named as given.
pub(super) fn pack(files: &[(String, Vec<u8>)]) -> std::io::Result<Vec<u8>> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(FILE_MODE);
        b.append_data(&mut h, name, &data[..])?;
    }
    b.into_inner()
}

/// The paths the person picked, checked, folders keeping their slash. None
/// picked is the whole volume.
fn picked(paths: &str) -> Result<Vec<String>, Refused> {
    let picked: Vec<&str> = paths.split('\n').filter(|p| !p.is_empty()).collect();
    if picked.len() > MAX_PICKED {
        return Err(Refused::Invalid(format!("pick at most {MAX_PICKED} at once")));
    }
    if picked.is_empty() {
        return Ok(vec![USER_FILES.to_string()]);
    }
    picked.into_iter().map(|p| user_path(p.trim_end_matches('/')).map(|_| p.to_string())).collect()
}

impl Kernel {
    pub async fn export_computer_files(&self, who: &Person, id: &str, paths: &str) -> Result<Vec<u8>, Refused> {
        let c = self.opened_computer(who, id)?;
        let roots = picked(paths)?;
        let whole = roots == [USER_FILES];
        let files = gather(&roots, |req| self.computer_file_for(&c, req)).await?;
        let mut named: Vec<(String, Vec<u8>)> = files.into_iter().map(|(p, b)| (format!("{FILES_FOLDER}{}", p.trim_start_matches(USER_FILES)), b)).collect();
        if whole {
            let manifest = serde_json::to_vec_pretty(&c.apps).map_err(|e| Refused::Failed(e.to_string()))?;
            named.push((MANIFEST_NAME.into(), manifest));
        }
        pack(&named).map_err(|e| Refused::Failed(format!("pack the files: {e}")))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Picked {
    /// One path per line; a folder ends in `/`. Empty: everything.
    #[serde(default)]
    paths: String,
}

pub(super) async fn export(State(k): State<Arc<Kernel>>, h: HeaderMap, Path(id): Path<String>, Query(q): Query<Picked>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match k.export_computer_files(&p, &id, &q.paths).await {
        Ok(tar) => ([(header::CONTENT_TYPE, "application/x-tar"), (header::CONTENT_DISPOSITION, "attachment; filename=\"magnus-files.tar\"")], tar).into_response(),
        Err(e) => answered(Err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A guest with a small tree of files.
    fn guest() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([("/workspace/a.txt", "one"), ("/workspace/notes/b.md", "two"), ("/workspace/notes/deep/c.csv", "three"), ("/workspace/other.txt", "four")])
    }

    async fn ask(tree: &BTreeMap<&'static str, &'static str>, req: Value) -> Result<Value, Refused> {
        let path = req["path"].as_str().unwrap_or_default();
        match req["op"].as_str() {
            Some("read") => {
                tree.get(path).map(|t| json!({ "data_b64": base64::engine::general_purpose::STANDARD.encode(t) })).ok_or(Refused::Failed(format!("{path}: no such file")))
            }
            Some("list") => {
                let mut names = std::collections::BTreeSet::new();
                for k in tree.keys().filter_map(|k| k.strip_prefix(path)) {
                    names.insert(match k.split_once('/') {
                        Some((d, _)) => (d.to_string(), true),
                        None => (k.to_string(), false),
                    });
                }
                Ok(json!({ "entries": names.into_iter().map(|(name, dir)| json!({ "name": name, "dir": dir })).collect::<Vec<_>>() }))
            }
            _ => Err(Refused::Failed("unknown op".into())),
        }
    }

    fn unpacked(tar: &[u8]) -> BTreeMap<String, String> {
        let mut a = tar::Archive::new(tar);
        a.entries()
            .unwrap()
            .map(|e| {
                let mut e = e.unwrap();
                let name = e.path().unwrap().display().to_string();
                let mut s = String::new();
                std::io::Read::read_to_string(&mut e, &mut s).unwrap();
                (name, s)
            })
            .collect()
    }

    #[tokio::test]
    async fn a_picked_folder_and_file_leave_together_with_everything_inside() {
        let tree = guest();
        let roots = picked("/workspace/notes/\n/workspace/a.txt").unwrap();
        let files = gather(&roots, |req| ask(&tree, req)).await.unwrap();
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["/workspace/notes/b.md", "/workspace/notes/deep/c.csv", "/workspace/a.txt"]);
        let tar = pack(&files.into_iter().map(|(p, b)| (p.trim_start_matches(USER_FILES).to_string(), b)).collect::<Vec<_>>()).unwrap();
        let got = unpacked(&tar);
        assert_eq!(got["notes/deep/c.csv"], "three");
        assert_eq!(got["a.txt"], "one");
        assert!(!got.contains_key("other.txt"), "a file nobody picked left");
    }

    #[tokio::test]
    async fn nothing_picked_is_the_whole_volume() {
        let tree = guest();
        let roots = picked("").unwrap();
        assert_eq!(roots, [USER_FILES]);
        assert_eq!(gather(&roots, |req| ask(&tree, req)).await.unwrap().len(), tree.len());
    }

    #[test]
    fn only_the_persons_files_can_be_picked() {
        assert!(picked("/etc/passwd").is_err());
        assert!(picked("/workspace/../etc/").is_err());
        assert!(picked("/workspace/").is_err(), "the root is asked for by picking nothing");
        assert!(picked(&"/workspace/a\n".repeat(MAX_PICKED + 1)).is_err());
    }

    #[tokio::test]
    async fn a_tree_too_deep_is_refused_not_walked_forever() {
        let deep = |req: Value| async move {
            let p = req["path"].as_str().unwrap_or_default().to_string();
            Ok::<_, Refused>(if p.ends_with('/') { json!({ "entries": [{ "name": "d", "dir": true }] }) } else { json!({}) })
        };
        assert!(matches!(gather(&["/workspace/x/".to_string()], deep).await, Err(Refused::Invalid(_))));
    }
}
