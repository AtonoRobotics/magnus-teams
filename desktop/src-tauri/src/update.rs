//! Updates of the app itself, from the appliance it is connected to (the
//! appliance serves the app's signed releases): looked for when the app
//! starts and every hour, downloaded in the background and checked against
//! the release key (the signature must bind the version), then installed
//! when the person chooses "Restart to update". After the restart the app
//! shows what changed, in plain words.
//!
//! The appliance decides which release channel the app gets by who is signed
//! in (DECISIONS.md, "Release channels"), so the app asks with its session.
//! It only ever moves forward (the updater's own rule: a newer version).
//!
//! On Windows the NSIS installer updates in place; on Linux only the
//! AppImage updates itself (a .deb is updated with the package manager).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// An hour, as the appliance looks for its own releases: a release reaches
/// every app within the hour, and a look is one small request.
const EVERY: Duration = Duration::from_secs(3600);

/// How long the page has, from the app's start, to say it works (its first
/// screen drawn with no error while its scripts loaded). A page that has not
/// said so by then is broken: the "Restart to update" button it would draw
/// is not there, so the app installs a downloaded release on its own.
const PAGE_WORKS_WAIT: Duration = Duration::from_secs(90);

/// How often an app whose page is broken looks for a release that mends it.
const BROKEN_PAGE_LOOK_EVERY: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>,
    error: Mutex<Option<String>>,
    looked: Mutex<Option<String>>,
    page_works: AtomicBool,
}

/// Whether to install a downloaded release without the person's click:
/// only when the page has not said it works within `PAGE_WORKS_WAIT`, as on
/// 0.2.6-test.380, whose page stopped while loading and left the app an
/// empty window that could not offer the update that mended it.
fn install_unasked(page_works: bool, since_start: Duration, update_ready: bool) -> bool {
    !page_works && since_start >= PAGE_WORKS_WAIT && update_ready
}

/// Whether this copy of the app can replace itself.
pub fn can_update() -> Result<(), &'static str> {
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none() {
        return Err("This copy was installed from a package; the package manager updates it.");
    }
    Ok(())
}

/// The manifest's address on the appliance, TLS for reaching it, and the
/// session to ask with, if signed in. A pinned address is trusted by its pin;
/// the public address by the public roots, presenting this computer's
/// device certificate as the tunnel requires (it has no pin, so asking for
/// one left every install on it without updates).
fn source(app: &crate::App) -> Result<(url::Url, rustls::ClientConfig, Option<String>), String> {
    let cfg = app.store.config();
    let base = cfg.appliance.ok_or("not connected to a server")?;
    if !base.starts_with("https://") {
        return Err("updates come only over the server's https address".into());
    }
    let tls = if crate::pin::is_public_appliance(&base) {
        let (cert, key) = app.store.device(&base).ok_or("this computer is not paired with the server yet")?;
        crate::pin::public_tls(&cert, &key)?
    } else {
        let pin = cfg.pins.get(&base).cloned().ok_or("the server's certificate is not trusted yet")?;
        crate::pin::tls(Some(&pin))?
    };
    let token = app.store.token(&base);
    Ok((url::Url::parse(&format!("{base}/v1/updates/desktop/latest.json")).map_err(|e| e.to_string())?, tls, token))
}

/// Look for a newer release and download it; true when one is ready.
pub async fn look<R: Runtime>(handle: &AppHandle<R>) -> Result<bool, String> {
    let app = handle.state::<crate::App>();
    let ups = handle.state::<Updates>();
    if ups.ready.lock().unwrap().is_some() {
        return Ok(true);
    }
    can_update()?;
    let (endpoint, tls, token) = source(&app)?;
    let mut builder = handle
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .header("x-magnus-app", crate::app_version())
        .map_err(|e| e.to_string())?;
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}")).map_err(|e| e.to_string())?;
    }
    let updater = builder
        .configure_client(move |b| b.tls_backend_preconfigured(tls.clone()))
        .build()
        .map_err(|e| e.to_string())?;
    *ups.looked.lock().unwrap() = Some(chrono_now());
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else { return Ok(false) };
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    let version = update.version.clone();
    *ups.ready.lock().unwrap() = Some((update, bytes));
    let _ = handle.emit("update-ready", json!({ "version": version }));
    Ok(true)
}

fn chrono_now() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    s.to_string()
}

/// Keep looking, from the start and every hour.
pub fn start<R: Runtime>(handle: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let r = look(&handle).await;
            *handle.state::<Updates>().error.lock().unwrap() = r.err();
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Watch for a page that never says it works, and when it has not, install
/// a downloaded release unasked: the page is what offers "Restart to
/// update", so a release whose page breaks would otherwise keep the app on
/// it until the person reinstalled by hand. Stops once the page works.
pub fn watch_page<R: Runtime>(handle: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        tokio::time::sleep(PAGE_WORKS_WAIT).await;
        loop {
            let ups = handle.state::<Updates>();
            if ups.page_works.load(Ordering::SeqCst) {
                return;
            }
            let r = look(&handle).await;
            if let Ok(mut e) = ups.error.lock() {
                *e = r.err();
            }
            let ready = ups.ready.lock().map(|r| r.is_some()).unwrap_or(false);
            if install_unasked(ups.page_works.load(Ordering::SeqCst), started.elapsed(), ready) {
                if let Err(e) = install(&handle) {
                    if let Ok(mut err) = ups.error.lock() {
                        *err = Some(e);
                    }
                }
            }
            tokio::time::sleep(BROKEN_PAGE_LOOK_EVERY).await;
        }
    });
}

/// The page drew its first screen with no error while its scripts loaded.
#[tauri::command]
pub fn page_works<R: Runtime>(handle: AppHandle<R>) {
    handle.state::<Updates>().page_works.store(true, Ordering::SeqCst);
}

fn whats_new_file<R: Runtime>(handle: &AppHandle<R>) -> Option<std::path::PathBuf> {
    handle.path().app_config_dir().ok().map(|d| d.join("whats-new.json"))
}

#[tauri::command]
pub async fn update_state<R: Runtime>(handle: AppHandle<R>) -> Result<Value, String> {
    let ups = handle.state::<Updates>();
    let ready = ups.ready.lock().unwrap().as_ref().map(|(u, _)| json!({ "version": u.version, "notes": u.body }));
    Ok(json!({
        "current": crate::app_version(),
        "ready": ready,
        "error": ups.error.lock().unwrap().clone(),
        "can_update": can_update().err(),
        "looked_at": ups.looked.lock().unwrap().clone(),
    }))
}

/// Look now, as the person asked.
#[tauri::command]
pub async fn update_look<R: Runtime>(handle: AppHandle<R>) -> Result<Value, String> {
    let r = look(&handle).await;
    *handle.state::<Updates>().error.lock().unwrap() = r.clone().err();
    r?;
    update_state(handle).await
}

/// Install the downloaded release and restart into it.
#[tauri::command]
pub async fn update_install<R: Runtime>(handle: AppHandle<R>) -> Result<(), String> {
    install(&handle)
}

/// Install the downloaded release and restart into it: on the person's
/// click, or unasked when the page is broken (`watch_page`).
fn install<R: Runtime>(handle: &AppHandle<R>) -> Result<(), String> {
    let ups = handle.state::<Updates>();
    let Some((update, bytes)) = ups.ready.lock().unwrap().take() else { return Err("no update is ready".into()) };
    if let Some(f) = whats_new_file(handle) {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&f, serde_json::to_vec(&json!({ "version": update.version, "notes": update.body, "from": crate::app_version() })).unwrap_or_default());
    }
    if let Err(e) = update.install(&bytes) {
        *ups.ready.lock().unwrap() = Some((update, bytes));
        return Err(e.to_string());
    }
    handle.restart();
}

/// What changed in this version, after an update brought it: once for the
/// window shown at start, and `again` whenever Settings asks. Once shown, the
/// note moves from `whats-new.json` to `whats-new-shown.json`, where only
/// Settings reads it.
#[tauri::command]
pub fn whats_new<R: Runtime>(handle: AppHandle<R>, again: Option<bool>) -> Option<Value> {
    let pending = whats_new_file(&handle)?;
    let shown = pending.with_file_name("whats-new-shown.json");
    let read = |f: &std::path::Path| {
        let v: Value = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
        (v["version"].as_str() == Some(crate::app_version().as_str())).then_some(v)
    };
    if again == Some(true) {
        return read(&pending).or_else(|| read(&shown));
    }
    let v = read(&pending)?;
    let _ = std::fs::rename(&pending, &shown);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_that_never_works_gets_the_downloaded_release_unasked() {
        assert!(install_unasked(false, PAGE_WORKS_WAIT, true));
    }

    #[test]
    fn a_working_page_waits_for_the_persons_click() {
        assert!(!install_unasked(true, PAGE_WORKS_WAIT * 10, true));
    }

    #[test]
    fn a_page_still_starting_is_given_its_time() {
        assert!(!install_unasked(false, PAGE_WORKS_WAIT - Duration::from_secs(1), true));
    }

    #[test]
    fn nothing_is_installed_before_a_release_is_downloaded() {
        assert!(!install_unasked(false, PAGE_WORKS_WAIT * 10, false));
    }
}
