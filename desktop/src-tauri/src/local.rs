//! This computer, connected to the person's Magnus: `magnus-local` runs
//! beside the app while it is open, inside the folders the person granted.
use crate::{App, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use tauri::State;

const BINARY: &str = "magnus-local";
const SAVED_FILE: &str = "local-computer.json";
const PAUSE_FILE: &str = "local-paused";
const MAX_FOLDERS: usize = 32;

#[derive(Serialize, Deserialize, Clone)]
struct Connection {
    name: String,
    mode: String,
    folders: Vec<String>,
}

/// The running agent. It goes when its connection does. A plain process,
/// because the commands that start it run on the interface thread, which
/// has no async runtime.
struct Agent(Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
pub struct Local(Mutex<Option<(Agent, Connection)>>);

fn dir() -> Res<PathBuf> {
    let d = dirs::config_dir().ok_or("this computer has no settings folder")?.join("magnus");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn saved() -> Option<Connection> {
    serde_json::from_slice(&std::fs::read(dir().ok()?.join(SAVED_FILE)).ok()?).ok()
}

fn check(c: &Connection) -> Res<()> {
    if c.name.trim().is_empty() {
        return Err("Give this computer a name.".into());
    }
    if !["ask", "allow", "never"].contains(&c.mode.as_str()) {
        return Err("That is not one of the choices.".into());
    }
    if c.folders.is_empty() || c.folders.len() > MAX_FOLDERS {
        return Err(format!("Name between 1 and {MAX_FOLDERS} folders."));
    }
    match c.folders.iter().find(|f| !Path::new(f).is_absolute() || !Path::new(f).is_dir()) {
        Some(f) => Err(format!("{f} is not a folder on this computer.")),
        None => Ok(()),
    }
}

/// Beside the app when installed, otherwise on the PATH.
fn binary() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(BINARY))).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from(BINARY))
}

fn launch(mut command: Command) -> Res<Agent> {
    command.spawn().map(Agent).map_err(|e| format!("The local agent could not start: {e}"))
}

fn spawn(app: &App, c: &Connection) -> Res<Agent> {
    let (url, token, pin, device) = app.local_session()?;
    let mut command = Command::new(binary());
    command.arg("--server").arg(&url).arg("--name").arg(&c.name).arg("--mode").arg(&c.mode).arg("--pause-file").arg(dir()?.join(PAUSE_FILE));
    for f in &c.folders {
        command.arg("--folder").arg(f);
    }
    if let Some(p) = pin {
        command.arg("--pin").arg(p);
    }
    if let Some((cert, key)) = device {
        command.env("MAGNUS_LOCAL_DEVICE_CERT", cert).env("MAGNUS_LOCAL_DEVICE_KEY", key);
    }
    command.env("MAGNUS_LOCAL_TOKEN", token);
    launch(command)
}

/// Takes a connection that ended off the books.
fn running(slot: &mut Option<(Agent, Connection)>) -> Option<&Connection> {
    if slot.as_mut().is_some_and(|(agent, _)| !matches!(agent.0.try_wait(), Ok(None))) {
        *slot = None;
    }
    slot.as_ref().map(|(_, c)| c)
}

/// What this computer is doing. A saved connection starts again here, so
/// it comes back with the app.
#[tauri::command]
pub fn local_state(app: State<'_, App>, local: State<'_, Local>) -> Res<Value> {
    let mut slot = local.0.lock().map_err(|e| e.to_string())?;
    if running(&mut slot).is_none() {
        if let Some(c) = saved() {
            if let Ok(child) = spawn(&app, &c) {
                *slot = Some((child, c));
            }
        }
    }
    let paused = dir()?.join(PAUSE_FILE).exists();
    let name = running(&mut slot).map(|c| c.name.clone());
    let suggested = std::env::var("HOSTNAME").ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|h| h.trim().to_string()).unwrap_or_default();
    Ok(json!({ "running": name.is_some(), "name": name, "paused": paused, "suggested_name": suggested }))
}

#[tauri::command]
pub fn local_start(app: State<'_, App>, local: State<'_, Local>, name: String, mode: String, folders: Vec<String>) -> Res<()> {
    let c = Connection { name: name.trim().to_string(), mode, folders };
    check(&c)?;
    let child = spawn(&app, &c)?;
    let body = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
    std::fs::write(dir()?.join(SAVED_FILE), body).map_err(|e| e.to_string())?;
    *local.0.lock().map_err(|e| e.to_string())? = Some((child, c));
    Ok(())
}

/// The agent stays off this computer while the file exists.
#[tauri::command]
pub fn local_pause(paused: bool) -> Res<()> {
    let file = dir()?.join(PAUSE_FILE);
    if paused {
        std::fs::write(file, b"").map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[tauri::command]
pub fn local_stop(local: State<'_, Local>) -> Res<()> {
    let file = dir()?.join(SAVED_FILE);
    if let Err(e) = std::fs::remove_file(file) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.to_string());
        }
    }
    *local.0.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(folders: Vec<String>) -> Connection {
        Connection { name: "desk".into(), mode: "ask".into(), folders }
    }

    // No runtime here, as on the interface thread: a tokio process would
    // panic and take the app down with it.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_agent_starts_without_a_runtime_and_goes_with_its_connection() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let Ok(mut agent) = launch(command) else { panic!("it did not start") };
        assert!(matches!(agent.0.try_wait(), Ok(None)));
        let process = format!("/proc/{}", agent.0.id());
        drop(agent);
        assert!(!Path::new(&process).exists());
    }

    #[test]
    fn only_real_folders_can_be_granted() {
        let d = std::env::temp_dir().to_string_lossy().to_string();
        assert!(check(&connection(vec![d])).is_ok());
        assert!(check(&connection(vec![])).is_err());
        assert!(check(&connection(vec!["relative/path".into()])).is_err());
        assert!(check(&connection(vec!["/no/such/folder/here".into()])).is_err());
    }
}
