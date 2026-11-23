//! This computer, connected to the person's Magnus: `magnus-local` runs
//! beside the app while it is open, inside the folders the person granted.
//! The connection is kept for that person at that server (`session::key`),
//! so another person signing in here never inherits it; the running agent
//! belongs to the session and ends with it.
use crate::session::Session;
use crate::{App, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use tauri::State;

const BINARY: &str = "magnus-local";
/// Where an older app kept one connection for the whole computer, whoever
/// made it. It is removed, never used: nothing says whose it was.
const UNKEYED_FILE: &str = "local-computer.json";
const PAUSE_FILE: &str = "local-paused";
const MAX_FOLDERS: usize = 32;

#[derive(Serialize, Deserialize, Clone)]
pub struct Connection {
    name: String,
    mode: String,
    folders: Vec<String>,
}

#[cfg(test)]
impl Connection {
    pub fn for_test() -> Connection {
        Connection { name: "desk".into(), mode: "ask".into(), folders: vec![] }
    }
}

/// The running agent. It goes when its connection does. A plain process,
/// because the commands that start it run on the interface thread, which
/// has no async runtime.
pub struct Agent(Child);

#[cfg(test)]
impl Agent {
    pub fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn dir() -> Res<PathBuf> {
    let d = dirs::config_dir().ok_or("this computer has no settings folder")?.join("magnus");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn saved_file(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("local-computer-{key}.json"))
}

/// The connection kept for the person `key` names, if any.
fn saved(dir: &Path, key: &str) -> Option<Connection> {
    serde_json::from_slice(&std::fs::read(saved_file(dir, key)).ok()?).ok()
}

fn save(dir: &Path, key: &str, c: &Connection) -> Res<()> {
    let body = serde_json::to_vec(c).map_err(|e| e.to_string())?;
    std::fs::write(saved_file(dir, key), body).map_err(|e| e.to_string())
}

fn forget(file: &Path) -> Res<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
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

pub fn launch(mut command: Command) -> Res<Agent> {
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

/// What this computer is doing. The signed-in person's saved connection
/// starts again here, so it comes back with the app.
#[tauri::command]
pub fn local_state(app: State<'_, App>) -> Res<Value> {
    let dir = dir()?;
    forget(&dir.join(UNKEYED_FILE))?;
    let key = Session::key_of(&app.store);
    let mut slot = app.person.local();
    if running(&mut slot).is_none() {
        if let Some(c) = key.and_then(|k| saved(&dir, &k)) {
            if let Ok(child) = spawn(&app, &c) {
                *slot = Some((child, c));
            }
        }
    }
    let paused = dir.join(PAUSE_FILE).exists();
    let name = running(&mut slot).map(|c| c.name.clone());
    let suggested = std::env::var("HOSTNAME").ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|h| h.trim().to_string()).unwrap_or_default();
    Ok(json!({ "running": name.is_some(), "name": name, "paused": paused, "suggested_name": suggested }))
}

#[tauri::command]
pub fn local_start(app: State<'_, App>, name: String, mode: String, folders: Vec<String>) -> Res<()> {
    let c = Connection { name: name.trim().to_string(), mode, folders };
    check(&c)?;
    let key = Session::key_of(&app.store).ok_or("signed out")?;
    let child = spawn(&app, &c)?;
    save(&dir()?, &key, &c)?;
    *app.person.local() = Some((child, c));
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
pub fn local_stop(app: State<'_, App>) -> Res<()> {
    if let Some(key) = Session::key_of(&app.store) {
        forget(&saved_file(&dir()?, &key))?;
    }
    *app.person.local() = None;
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

    /// One person's connection is theirs at that server: another person
    /// signing in on this computer finds none, and the old unkeyed file
    /// goes without being used.
    #[test]
    fn a_saved_connection_is_one_persons() {
        let dir = tempfile_dir();
        let (ana, ben) = (crate::session::key("https://s", "ana"), crate::session::key("https://s", "ben"));
        let c = Connection { name: "desk".into(), mode: "allow".into(), folders: vec!["/home/ana".into()] };
        save(&dir, &ana, &c).unwrap();
        assert!(saved(&dir, &ben).is_none(), "the next person inherited the last one's folders");
        assert_eq!(saved(&dir, &ana).map(|c| c.folders), Some(vec!["/home/ana".to_string()]));
        std::fs::write(dir.join(UNKEYED_FILE), serde_json::to_vec(&c).unwrap()).unwrap();
        forget(&dir.join(UNKEYED_FILE)).unwrap();
        assert!(!dir.join(UNKEYED_FILE).exists());
        forget(&saved_file(&dir, &ana)).unwrap();
        assert!(saved(&dir, &ana).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("magnus-local-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
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
