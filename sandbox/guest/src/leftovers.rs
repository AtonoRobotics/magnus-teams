//! What a takeover leaves behind (decisions/2026-11-13-takeover-leaves-nothing-running.md).
//!
//! When a person takes the screen, `begin` records a boundary: every process
//! running then, and the desktop's autostart entries. At handback, `end`
//! stops every process that was not there at the boundary, whatever it was
//! started from: a launcher, a terminal, a program that forked itself away
//! to PID 1, or one that replaced its own image with another (its exe is no
//! longer the one recorded). Each gets SIGSTOP first, so none forks while
//! the others are found, then SIGTERM and a grace to exit, then SIGKILL.
//! Autostart entries made or changed in the session are removed or put back.
//! Only when nothing of the session is left does `end` answer ok, naming
//! what it stopped and removed; otherwise the kernel keeps the agent paused.
//!
//! Not stopped: kernel threads, PID 1, this agent and what it runs for the
//! kernel (a screenshot while the person holds the screen), and the workers
//! an app that was already running forks for itself from its own binary
//! (a browser's new tab is a new renderer process). The image has no
//! systemd, cron or at, and nothing reads autostart entries, so there is no
//! other place a session can leave something to start later.
//!
//! A boundary lives in this process's memory only: a guest agent that was
//! restarted has none, and `end` then refuses to say the computer is clean.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// How long a stopped process has to exit after SIGTERM before SIGKILL.
pub const TERM_GRACE: Duration = Duration::from_secs(3);
/// How often the process table is looked at while waiting.
pub const SWEEP_PAUSE: Duration = Duration::from_millis(50);
/// Sweeps for processes forked while the last one ran, before giving up.
pub const MAX_SWEEPS: u32 = 40;
/// How far up a process's parents are followed.
const MAX_ANCESTRY: usize = 256;
/// Names in a report; the count is always whole.
const MAX_NAMED: usize = 50;
const MAX_AUTOSTART_ENTRIES: usize = 256;
const MAX_AUTOSTART_ENTRY_BYTES: u64 = 64 * 1024;
/// Where an XDG session manager looks for programs to start: the image's,
/// the desktop's (HOME is /workspace) and the session's own config home
/// (sandbox/desktop/magnus-desktop).
pub const AUTOSTART_DIRS: &[&str] = &["/etc/xdg/autostart", "/workspace/.config/autostart", "/tmp/magnus-desktop/config/autostart"];
/// Apps whose new processes are their own workers when they come from an
/// instance that was already running, through processes of the same binary.
const OWN_WORKERS: &[&str] = &["/usr/lib/chromium/chromium"];

/// One process, as `/proc` shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    /// Clock ticks after boot: with the pid, which process this is.
    pub start: u64,
    pub name: String,
    pub exe: String,
    pub kernel: bool,
    pub zombie: bool,
}

impl Proc {
    fn key(&self) -> (i32, u64) {
        (self.pid, self.start)
    }

    fn named(&self) -> String {
        format!("{} (pid {})", self.name, self.pid)
    }
}

/// The process table and signals, so the sweep can be tried without a VM.
pub trait System {
    fn processes(&self) -> Result<Vec<Proc>, String>;
    /// Sends `signal` to the process `pid` started at `start`, and to no
    /// other that has since taken its pid.
    fn signal(&self, pid: i32, start: u64, signal: i32);
    fn pause(&self, d: Duration);
    /// This agent's pid.
    fn me(&self) -> i32;
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    File(Vec<u8>),
    Link(PathBuf),
}

/// What was there when the person took the screen.
pub struct Boundary {
    seen: HashMap<i32, (u64, String)>,
    autostart: BTreeMap<PathBuf, Entry>,
    /// Until a handback has cleaned up after it.
    open: bool,
}

static BOUNDARY: Mutex<Option<Boundary>> = Mutex::new(None);

/// Records the boundary, unless a takeover is still open: a new holder
/// does not wash out what the last one started.
pub fn begin(sys: &dyn System, dirs: &[PathBuf]) -> Value {
    let mut slot = BOUNDARY.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|b| b.open) {
        return json!({ "ok": true, "kept": true });
    }
    match boundary(sys, dirs) {
        Ok(b) => {
            *slot = Some(b);
            json!({ "ok": true })
        }
        Err(e) => json!({ "error": format!("the takeover's boundary was not recorded: {e}") }),
    }
}

/// Stops what the session left. Run again, it sweeps again against the
/// same boundary, so a handback that is retried is checked afresh.
pub fn end(sys: &dyn System, dirs: &[PathBuf]) -> Value {
    let mut slot = BOUNDARY.lock().unwrap_or_else(|e| e.into_inner());
    let Some(b) = slot.as_mut() else {
        return json!({ "error": "no takeover boundary was recorded (the guest agent restarted, or is older than this check), so what the session started cannot be told apart" });
    };
    let stopped = match sweep(sys, b) {
        Ok(s) => s,
        Err(e) => return json!({ "error": e }),
    };
    let removed = match restore_autostart(&b.autostart, dirs) {
        Ok(r) => r,
        Err(e) => return json!({ "error": e }),
    };
    b.open = false;
    json!({ "ok": true, "stopped_count": stopped.len(), "stopped": stopped.iter().take(MAX_NAMED).collect::<Vec<_>>(), "removed": removed })
}

/// What a handback would close now, by program name, each once: nothing is
/// signalled, so the person can save their work first.
pub fn preview(sys: &dyn System) -> Value {
    let slot = BOUNDARY.lock().unwrap_or_else(|e| e.into_inner());
    let Some(b) = slot.as_ref() else {
        return json!({ "error": "no takeover boundary was recorded, so what the session started cannot be told apart" });
    };
    match sys.processes() {
        Ok(procs) => {
            let names: BTreeSet<String> = strays(&procs, b, sys.me()).into_iter().map(|p| p.name).collect();
            json!({ "ok": true, "programs": names.into_iter().take(MAX_NAMED).collect::<Vec<_>>() })
        }
        Err(e) => json!({ "error": e }),
    }
}

pub fn boundary(sys: &dyn System, dirs: &[PathBuf]) -> Result<Boundary, String> {
    let seen = sys.processes()?.into_iter().map(|p| (p.pid, (p.start, p.exe))).collect();
    Ok(Boundary { seen, autostart: read_autostart(dirs)?, open: true })
}

fn ancestors<'a>(p: &'a Proc, by_pid: &'a HashMap<i32, &'a Proc>) -> impl Iterator<Item = &'a Proc> + 'a {
    let mut at = Some(p);
    std::iter::from_fn(move || {
        let parent = by_pid.get(&at?.ppid).copied().filter(|q| q.pid != at.map_or(0, |a| a.pid));
        at = parent;
        parent
    })
    .take(MAX_ANCESTRY)
}

/// The processes the session left running.
pub fn strays(procs: &[Proc], b: &Boundary, me: i32) -> Vec<Proc> {
    let by_pid: HashMap<i32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let before = |p: &Proc| b.seen.get(&p.pid).is_some_and(|(start, exe)| *start == p.start && *exe == p.exe);
    let mine = |p: &Proc| p.pid == me || ancestors(p, &by_pid).any(|a| a.pid == me);
    // Up through processes of its own binary to one that was there before.
    let own_worker = |p: &Proc| {
        OWN_WORKERS.contains(&p.exe.as_str()) && {
            let mut line = ancestors(p, &by_pid).take_while(|a| a.exe == p.exe);
            line.any(before)
        }
    };
    procs.iter().filter(|p| !p.kernel && !p.zombie && p.pid != 1 && !before(p) && !mine(p) && !own_worker(p)).cloned().collect()
}

/// Stops every stray: frozen first, so none forks while the rest are found,
/// then asked to exit, then killed. Ok only when none is left.
pub fn sweep(sys: &dyn System, b: &Boundary) -> Result<Vec<String>, String> {
    let find = || sys.processes().map(|procs| strays(&procs, b, sys.me()));
    let mut named: BTreeMap<(i32, u64), String> = BTreeMap::new();
    let mut frozen: BTreeSet<(i32, u64)> = BTreeSet::new();
    for _ in 0..MAX_SWEEPS {
        let fresh: Vec<Proc> = find()?.into_iter().filter(|p| !frozen.contains(&p.key())).collect();
        if fresh.is_empty() {
            break;
        }
        for p in fresh {
            sys.signal(p.pid, p.start, libc::SIGSTOP);
            frozen.insert(p.key());
            named.insert(p.key(), p.named());
        }
    }
    for &(pid, start) in &frozen {
        sys.signal(pid, start, libc::SIGTERM);
        sys.signal(pid, start, libc::SIGCONT);
    }
    let polls = (TERM_GRACE.as_millis() / SWEEP_PAUSE.as_millis()).max(1);
    for _ in 0..polls {
        if find()?.is_empty() {
            break;
        }
        sys.pause(SWEEP_PAUSE);
    }
    for _ in 0..MAX_SWEEPS {
        let left = find()?;
        if left.is_empty() {
            break;
        }
        for p in left {
            named.insert(p.key(), p.named());
            sys.signal(p.pid, p.start, libc::SIGKILL);
        }
        sys.pause(SWEEP_PAUSE);
    }
    let left = find()?;
    if !left.is_empty() {
        let names: Vec<String> = left.iter().take(MAX_NAMED).map(Proc::named).collect();
        return Err(format!("{} processes started during the takeover would not stop: {}", left.len(), names.join(", ")));
    }
    Ok(named.into_values().collect())
}

/// A directory is read only as itself: one reached through a link could
/// lead the cleanup to remove another directory's files.
fn plain_dir(dir: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", dir.display())),
        Ok(_) if std::fs::canonicalize(dir).ok().as_deref() != Some(dir) => Err(format!("{} is reached through a link", dir.display())),
        Ok(m) if m.is_dir() => Ok(true),
        Ok(_) => Err(format!("{} is not a directory", dir.display())),
    }
}

fn read_entry(path: &Path) -> Result<Option<Entry>, String> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return std::fs::read_link(path).map(|t| Some(Entry::Link(t))).map_err(|e| format!("{}: {e}", path.display()));
    }
    if !meta.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.take(MAX_AUTOSTART_ENTRY_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    // Too big to keep: not recorded, so removed at handback if still there.
    Ok((bytes.len() as u64 <= MAX_AUTOSTART_ENTRY_BYTES).then_some(Entry::File(bytes)))
}

fn read_autostart(dirs: &[PathBuf]) -> Result<BTreeMap<PathBuf, Entry>, String> {
    let mut entries = BTreeMap::new();
    for dir in dirs {
        if !plain_dir(dir)? {
            continue;
        }
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().take(MAX_AUTOSTART_ENTRIES) {
            if let Some(entry) = read_entry(&e.path())? {
                entries.insert(e.path(), entry);
            }
        }
    }
    Ok(entries)
}

/// Removes what the session added and puts back what it changed or removed.
fn restore_autostart(kept: &BTreeMap<PathBuf, Entry>, dirs: &[PathBuf]) -> Result<Vec<String>, String> {
    let now = read_autostart(dirs)?;
    let mut removed = Vec::new();
    for dir in dirs.iter().filter(|d| plain_dir(d).unwrap_or(false)) {
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
            let path = e.path();
            if !kept.contains_key(&path) {
                let gone = if e.file_type().is_ok_and(|t| t.is_dir()) { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                gone.map_err(|err| format!("remove {}: {err}", path.display()))?;
                removed.push(path.display().to_string());
            }
        }
    }
    for (path, entry) in kept {
        if now.get(path) == Some(entry) {
            continue;
        }
        let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        match entry {
            Entry::File(bytes) => std::fs::write(path, bytes),
            Entry::Link(target) => std::os::unix::fs::symlink(target, path),
        }
        .map_err(|e| format!("put back {}: {e}", path.display()))?;
        removed.push(format!("{} (put back as it was)", path.display()));
    }
    Ok(removed)
}

/// The process table of this machine.
pub struct ProcFs;

/// `PF_KTHREAD` from linux/sched.h.
const KERNEL_THREAD: u64 = 0x0020_0000;

/// The fields of `/proc/<pid>/stat` after the name: the name may hold
/// spaces and parentheses, so it ends at the last `)`.
pub fn parse_stat(pid: i32, stat: &str, exe: String) -> Option<Proc> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = stat.get(close + 1..)?.split_whitespace().collect();
    let state = *rest.first()?;
    let ppid = rest.get(1)?.parse().ok()?;
    let flags: u64 = rest.get(6)?.parse().ok()?;
    let start = rest.get(19)?.parse().ok()?;
    Some(Proc { pid, ppid, start, name, exe, kernel: flags & KERNEL_THREAD != 0 || pid == 2 || ppid == 2, zombie: state == "Z" || state == "X" })
}

fn start_of(pid: i32) -> Option<u64> {
    parse_stat(pid, &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?, String::new()).map(|p| p.start)
}

impl System for ProcFs {
    fn processes(&self) -> Result<Vec<Proc>, String> {
        let dir = std::fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;
        Ok(dir
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let exe = std::fs::read_link(format!("/proc/{pid}/exe")).map(|p| p.display().to_string()).unwrap_or_default();
                parse_stat(pid, &stat, exe)
            })
            .collect())
    }

    fn signal(&self, pid: i32, start: u64, signal: i32) {
        // A pidfd holds this process: checked to be the one meant, it cannot
        // be another that took the pid between the check and the signal.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
        if fd < 0 {
            return;
        }
        if start_of(pid) == Some(start) {
            unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, signal, std::ptr::null::<libc::siginfo_t>(), 0) };
        }
        unsafe { libc::close(fd) };
    }

    fn pause(&self, d: Duration) {
        std::thread::sleep(d);
    }

    fn me(&self) -> i32 {
        std::process::id() as i32
    }
}

pub fn dirs() -> Vec<PathBuf> {
    AUTOSTART_DIRS.iter().map(PathBuf::from).collect()
}

#[cfg(test)]
#[path = "leftovers_tests.rs"]
mod tests;
