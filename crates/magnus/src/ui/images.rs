//! Images made by a provider. The agent gets a placeholder at once and goes
//! on; the client draws the picture when it is ready. Each image is kept
//! under its run, so it goes when the run's content goes.
use crate::api::{err, who};
use crate::boundary::Action;
use crate::kernel::{new_id, Kernel};
use anyhow::{anyhow, Context, Result};
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const DIRECTORY: &str = "ui-images";
const IMAGE_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_PROMPT_CHARS: usize = 4_000;
/// Between a run's id and the image's, in a placeholder.
const JOIN: char = '.';
pub const GENERATE_TOOL: &str = "image_generate";
pub const EDIT_TOOL: &str = "image_edit";

/// `[images]`: where pictures are made. The key is a vault entry's name.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// An OpenAI-compatible images endpoint, ending in `/images`.
    pub url: String,
    pub key: String,
    pub fast_model: String,
    pub premium_model: String,
}

fn safe(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(super) fn split_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(JOIN).filter(|(run, image)| safe(run) && safe(image))
}

pub(super) fn image_dir(state: &Path, run: &str) -> PathBuf {
    state.join(DIRECTORY).join(run)
}

fn png(state: &Path, id: &str) -> Option<PathBuf> {
    let (run, image) = split_id(id)?;
    Some(image_dir(state, run).join(format!("{image}.png")))
}

fn failed_marker(state: &Path, id: &str) -> Option<PathBuf> {
    png(state, id).map(|p| p.with_extension("failed"))
}

/// Images go with the runs they were made for.
pub fn forget_runs<'a>(state: &Path, runs: impl IntoIterator<Item = &'a String>) {
    for run in runs.into_iter().filter(|r| safe(r)) {
        if let Err(e) = std::fs::remove_dir_all(image_dir(state, run)) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("the images of {run} were not removed: {e}");
            }
        }
    }
}

pub fn router() -> Router<Arc<Kernel>> {
    Router::new().route("/v1/ui/images/{id}", get(picture))
}

/// 202 while it is being made; only the person whose run made it may fetch it.
async fn picture(State(k): State<Arc<Kernel>>, h: HeaderMap, UrlPath(id): UrlPath<String>) -> Response {
    let p = match who(&k, &h).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some((run, _)) = split_id(&id) else { return err(StatusCode::NOT_FOUND, "no such image") };
    if !k.read(|s| s.runs.get(run).is_some_and(|r| r.person == p.id)) {
        return err(StatusCode::NOT_FOUND, "no such image");
    }
    let state = &k.cfg.appliance.state_dir;
    if failed_marker(state, &id).is_some_and(|m| m.exists()) {
        return err(StatusCode::BAD_GATEWAY, "this image could not be made");
    }
    match png(state, &id).map(std::fs::read) {
        Some(Ok(bytes)) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

async fn make(http: reqwest::Client, cfg: Settings, key: String, model: String, prompt: String, source: Option<String>) -> Result<Vec<u8>> {
    let (url, body) = match source {
        Some(b64) => (format!("{}/edits", cfg.url), json!({ "model": model, "prompt": prompt, "images": [{ "image_url": format!("data:image/png;base64,{b64}") }] })),
        None => (format!("{}/generations", cfg.url), json!({ "model": model, "prompt": prompt })),
    };
    let reply: Value = http.post(url).bearer_auth(key).json(&body).timeout(IMAGE_TIMEOUT).send().await?.error_for_status()?.json().await?;
    let b64 = reply["data"][0]["b64_json"].as_str().ok_or_else(|| anyhow!("the provider returned no image"))?;
    base64::engine::general_purpose::STANDARD.decode(b64).context("the provider's image is not base64")
}

impl Kernel {
    /// Starts one image and answers with its placeholder.
    pub(crate) async fn image_tool(self: &Arc<Self>, a: &Action) -> Result<(String, bool, Option<Value>)> {
        let Some(cfg) = self.cfg.images.clone() else { return Ok(("This appliance has no image provider set up.".to_string(), true, None)) };
        let prompt: String = a.input["prompt"].as_str().unwrap_or_default().chars().take(MAX_PROMPT_CHARS).collect();
        if prompt.trim().is_empty() {
            return Ok(("A prompt is required.".to_string(), true, None));
        }
        let state = self.cfg.appliance.state_dir.clone();
        // An image to edit is one this run's person may read.
        let source = match a.input["image"].as_str() {
            _ if a.tool != EDIT_TOOL => None,
            Some(id) => {
                let owner = self.read(|s| s.runs.get(&a.run).map(|r| r.person.clone()));
                let theirs = split_id(id).is_some_and(|(run, _)| self.read(|s| s.runs.get(run).map(|r| r.person.clone())) == owner);
                match png(&state, id).filter(|_| theirs).map(std::fs::read) {
                    Some(Ok(bytes)) => Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                    _ => return Ok((format!("There is no finished image {id:?}."), true, None)),
                }
            }
            None => return Ok(("Name the image to edit.".to_string(), true, None)),
        };
        let key = self.vault.get(&cfg.key)?.context("the image provider's key is not in the vault")?.expose().to_string();
        let model = if a.input["quality"] == "premium" { cfg.premium_model.clone() } else { cfg.fast_model.clone() };
        let id = format!("{}{JOIN}{}", a.run, new_id("img"));
        let run = a.run.clone();
        let task_id = id.clone();
        tokio::spawn(async move {
            if let Err(e) = finish(state, &task_id, make(reqwest::Client::new(), cfg, key, model, prompt, source).await).await {
                tracing::warn!("image for {run}: {e:#}");
            }
        });
        Ok((
            format!("The image is being made. Write it into a spec as: image placeholder={id}. It appears when ready; you need not wait."),
            false,
            Some(json!({ "placeholder": id })),
        ))
    }
}

async fn finish(state: PathBuf, id: &str, made: Result<Vec<u8>>) -> Result<()> {
    let (Some(target), Some(failed)) = (png(&state, id), failed_marker(&state, id)) else { return Err(anyhow!("a bad image id")) };
    let dir = target.parent().context("an image has a directory")?;
    tokio::fs::create_dir_all(dir).await?;
    match made {
        Ok(bytes) => {
            let part = target.with_extension("part");
            tokio::fs::write(&part, bytes).await?;
            tokio::fs::rename(&part, &target).await?;
            Ok(())
        }
        Err(e) => {
            tokio::fs::write(&failed, b"").await?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_cannot_name_a_path_outside_its_directory() {
        for bad in ["../x.y", "a/b.c", "a.b/c", "a.", ".b", "a.b.c", "a"] {
            assert!(split_id(bad).is_none(), "{bad}");
        }
        assert_eq!(split_id("run_1.img_2"), Some(("run_1", "img_2")));
    }

    #[test]
    fn forgetting_a_run_removes_its_images_only() {
        let state = tempfile::tempdir().unwrap();
        let (a, b) = (image_dir(state.path(), "a"), image_dir(state.path(), "b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        forget_runs(state.path(), [&"a".to_string(), &"../b".to_string()]);
        assert!(!a.exists() && b.exists());
    }
}
