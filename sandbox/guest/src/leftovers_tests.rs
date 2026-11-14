use super::*;
use std::cell::RefCell;

/// How a fake process answers a signal.
#[derive(Clone, Copy, PartialEq)]
enum Answer {
    /// Exits on SIGTERM.
    Polite,
    /// Only SIGKILL ends it.
    Stubborn,
    /// Forks a child on SIGTERM, then exits.
    Forks,
    /// Nothing ends it (a process stuck in the kernel).
    Unkillable,
}

struct Fake {
    procs: RefCell<Vec<(Proc, Answer)>>,
    signals: RefCell<Vec<(i32, i32)>>,
    next_pid: RefCell<i32>,
}

const ME: i32 = 50;
const CHROMIUM: &str = "/usr/lib/chromium/chromium";

fn proc(pid: i32, ppid: i32, name: &str, exe: &str) -> Proc {
    Proc { pid, ppid, start: 1000 + pid as u64, name: name.into(), exe: exe.into(), kernel: false, zombie: false }
}

impl Fake {
    fn new(procs: Vec<Proc>) -> Fake {
        Fake { procs: RefCell::new(procs.into_iter().map(|p| (p, Answer::Polite)).collect()), signals: RefCell::default(), next_pid: RefCell::new(900) }
    }

    fn start(&self, p: Proc, answer: Answer) {
        self.procs.borrow_mut().push((p, answer));
    }

    fn alive(&self, pid: i32) -> bool {
        self.procs.borrow().iter().any(|(p, _)| p.pid == pid && !p.zombie)
    }
}

impl System for Fake {
    fn processes(&self) -> Result<Vec<Proc>, String> {
        Ok(self.procs.borrow().iter().map(|(p, _)| p.clone()).collect())
    }

    fn signal(&self, pid: i32, start: u64, signal: i32) {
        self.signals.borrow_mut().push((pid, signal));
        let mut procs = self.procs.borrow_mut();
        let Some(i) = procs.iter().position(|(p, _)| p.pid == pid && p.start == start) else { return };
        let answer = procs[i].1;
        let ends = match signal {
            libc::SIGTERM => matches!(answer, Answer::Polite | Answer::Forks),
            libc::SIGKILL => answer != Answer::Unkillable,
            _ => false,
        };
        if signal == libc::SIGTERM && answer == Answer::Forks {
            let mut next = self.next_pid.borrow_mut();
            *next += 1;
            procs.push((proc(*next, 1, "respawned", "/usr/bin/python3"), Answer::Stubborn));
        }
        if ends {
            procs.remove(i);
        }
    }

    fn pause(&self, _: Duration) {}

    fn me(&self) -> i32 {
        ME
    }
}

/// A desktop as a takeover finds it: the kernel's threads, PID 1, this
/// agent, the X server and panel, and a browser the agent had open.
fn desktop() -> Vec<Proc> {
    let mut kthread = proc(2, 0, "kthreadd", "");
    kthread.kernel = true;
    vec![
        proc(1, 0, "magnus-guest", "/sbin/magnus-guest"),
        kthread,
        proc(ME, 1, "magnus-guest", "/sbin/magnus-guest"),
        proc(60, 1, "Xvfb", "/usr/bin/Xvfb"),
        proc(61, 1, "xfce4-panel", "/usr/bin/xfce4-panel"),
        proc(62, 1, "bash", "/usr/bin/bash"),
        proc(70, 1, "chromium", CHROMIUM),
        proc(71, 70, "chromium", CHROMIUM),
    ]
}

fn no_dirs() -> Vec<PathBuf> {
    vec![]
}

#[test]
fn what_the_session_started_is_stopped_and_what_was_there_stays() {
    let sys = Fake::new(desktop());
    let b = boundary(&sys, &no_dirs()).unwrap();
    // From a launcher, from a terminal, and forked away to PID 1.
    sys.start(proc(100, 61, "xfce4-terminal", "/usr/bin/xfce4-terminal"), Answer::Polite);
    sys.start(proc(101, 62, "recorder", "/usr/bin/ffmpeg"), Answer::Stubborn);
    sys.start(proc(102, 1, "watcher", "/usr/bin/python3"), Answer::Polite);
    // What the kernel has the agent run, a new kernel thread, and a zombie.
    sys.start(proc(103, ME, "sh", "/usr/bin/dash"), Answer::Polite);
    sys.start(proc(104, 103, "import", "/usr/bin/import-im6.q16"), Answer::Polite);
    let mut kworker = proc(105, 2, "kworker/0:2", "");
    kworker.kernel = true;
    sys.start(kworker, Answer::Unkillable);
    let mut zombie = proc(106, 62, "true", "/usr/bin/true");
    zombie.zombie = true;
    sys.start(zombie, Answer::Unkillable);
    let stopped = sweep(&sys, &b).unwrap();
    for gone in [100, 101, 102] {
        assert!(!sys.alive(gone), "pid {gone}, started in the session, is still running");
    }
    for kept in [1, 2, ME, 60, 61, 62, 70, 71, 103, 104, 105] {
        assert!(sys.alive(kept), "pid {kept} was stopped though the session did not start it");
    }
    assert_eq!(stopped, ["xfce4-terminal (pid 100)", "recorder (pid 101)", "watcher (pid 102)"]);
    // Frozen before anything is asked to exit, so none forks meanwhile.
    let signals = sys.signals.borrow();
    let first_term = signals.iter().position(|(_, s)| *s == libc::SIGTERM).unwrap();
    assert!(signals[..first_term].iter().all(|(_, s)| *s == libc::SIGSTOP), "{:?}", &signals[..first_term]);
    assert!(signals.contains(&(101, libc::SIGKILL)), "a process that ignored SIGTERM was not killed");
    assert!(!signals.contains(&(100, libc::SIGKILL)), "a process that would exit was killed without its grace");
}

#[test]
fn a_browsers_own_new_tabs_stay_but_what_it_launches_does_not() {
    let sys = Fake::new(desktop());
    let b = boundary(&sys, &no_dirs()).unwrap();
    sys.start(proc(200, 71, "chromium", CHROMIUM), Answer::Polite);
    sys.start(proc(201, 200, "chromium", CHROMIUM), Answer::Polite);
    sys.start(proc(202, 70, "native-host", "/usr/bin/python3"), Answer::Polite);
    // A browser the person started is theirs, workers and all.
    sys.start(proc(203, 61, "chromium", CHROMIUM), Answer::Polite);
    sys.start(proc(204, 203, "chromium", CHROMIUM), Answer::Polite);
    let stopped = sweep(&sys, &b).unwrap();
    assert!(sys.alive(200) && sys.alive(201), "a tab of the agent's browser was closed");
    assert!(!sys.alive(202) && !sys.alive(203) && !sys.alive(204), "{stopped:?}");
}

#[test]
fn a_process_that_became_another_program_is_stopped() {
    let sys = Fake::new(desktop());
    let b = boundary(&sys, &no_dirs()).unwrap();
    // The terminal's shell, there before, ran `exec` into something else.
    sys.procs.borrow_mut().iter_mut().find(|(p, _)| p.pid == 62).unwrap().0.exe = "/usr/bin/ffmpeg".into();
    // A pid the session's process took after the one at the boundary exited.
    sys.procs.borrow_mut().retain(|(p, _)| p.pid != 60);
    let mut reused = proc(60, 1, "Xvfb", "/usr/bin/Xvfb");
    reused.start = 99_999;
    sys.start(reused, Answer::Polite);
    let stopped = sweep(&sys, &b).unwrap();
    assert_eq!(stopped, ["Xvfb (pid 60)", "bash (pid 62)"]);
}

#[test]
fn a_process_that_respawns_is_followed_and_one_that_cannot_be_stopped_is_reported() {
    let sys = Fake::new(desktop());
    let b = boundary(&sys, &no_dirs()).unwrap();
    sys.start(proc(300, 1, "hydra", "/usr/bin/python3"), Answer::Forks);
    let stopped = sweep(&sys, &b).unwrap();
    assert!(stopped.iter().any(|s| s.starts_with("respawned")), "the child forked on the way out was not stopped: {stopped:?}");
    assert!(sweep(&sys, &b).unwrap().is_empty(), "a second sweep found something");
    sys.start(proc(301, 1, "stuck", "/usr/bin/stuck"), Answer::Unkillable);
    let e = sweep(&sys, &b).unwrap_err();
    assert!(e.contains("would not stop") && e.contains("stuck (pid 301)"), "{e}");
}

#[test]
fn proc_stat_is_read_past_a_name_with_spaces_and_parentheses() {
    let stat = "4242 (my (odd) name) S 61 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 98765 1000 10 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0";
    let p = parse_stat(4242, stat, "/usr/bin/x".into()).unwrap();
    assert_eq!((p.ppid, p.start, p.name.as_str(), p.kernel, p.zombie), (61, 98765, "my (odd) name", false, false));
    let kthread = "7 (kworker/0:1) I 2 0 0 0 -1 69238880 0 0 0 0 0 0 0 0 20 0 1 0 12 0 0";
    assert!(parse_stat(7, kthread, String::new()).unwrap().kernel);
    let zombie = "9 (sh) Z 1 9 9 0 -1 4194564 0 0 0 0 0 0 0 0 20 0 1 0 50 0 0";
    assert!(parse_stat(9, zombie, String::new()).unwrap().zombie);
    assert!(parse_stat(9, "9 (sh", String::new()).is_none());
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("magnus-leftovers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

#[test]
fn autostart_entries_the_session_made_or_changed_are_undone() {
    let root = scratch("autostart");
    let (etc, home) = (root.join("etc"), root.join("home"));
    std::fs::create_dir_all(&etc).unwrap();
    std::fs::write(etc.join("keep.desktop"), "[Desktop Entry]\nExec=true\n").unwrap();
    std::fs::write(etc.join("edited.desktop"), "[Desktop Entry]\nExec=true\n").unwrap();
    std::fs::write(etc.join("deleted.desktop"), "[Desktop Entry]\nExec=true\n").unwrap();
    let dirs = vec![etc.clone(), home.clone()];
    let kept = read_autostart(&dirs).unwrap();
    // The session: a new entry in a new directory, one changed, one deleted.
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("spy.desktop"), "[Desktop Entry]\nExec=/workspace/spy\n").unwrap();
    std::fs::write(etc.join("edited.desktop"), "[Desktop Entry]\nExec=/workspace/spy\n").unwrap();
    std::fs::remove_file(etc.join("deleted.desktop")).unwrap();
    let removed = restore_autostart(&kept, &dirs).unwrap();
    assert!(!home.join("spy.desktop").exists(), "{removed:?}");
    assert_eq!(std::fs::read_to_string(etc.join("edited.desktop")).unwrap(), "[Desktop Entry]\nExec=true\n");
    assert!(etc.join("deleted.desktop").exists() && etc.join("keep.desktop").exists());
    assert_eq!(removed.len(), 3, "{removed:?}");
    assert!(restore_autostart(&kept, &dirs).unwrap().is_empty(), "a clean directory was changed");
    // A directory swapped for a link elsewhere is never cleaned through it.
    std::fs::remove_dir_all(&home).unwrap();
    std::os::unix::fs::symlink(&etc, &home).unwrap();
    assert!(restore_autostart(&kept, &dirs).unwrap_err().contains("through a link"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_handback_without_a_boundary_is_never_called_clean_and_a_retry_sweeps_again() {
    let sys = Fake::new(desktop());
    let dirs = no_dirs();
    *BOUNDARY.lock().unwrap() = None;
    assert!(end(&sys, &dirs)["error"].as_str().unwrap().contains("no takeover boundary"));
    assert_eq!(begin(&sys, &dirs)["ok"], true);
    sys.start(proc(400, 61, "first-holder", "/usr/bin/x"), Answer::Polite);
    // A second holder keeps the first one's boundary.
    assert_eq!(begin(&sys, &dirs)["kept"], true);
    let done = end(&sys, &dirs);
    assert_eq!((done["ok"].as_bool(), done["stopped_count"].as_u64()), (Some(true), Some(1)), "{done}");
    sys.start(proc(401, 61, "late", "/usr/bin/x"), Answer::Polite);
    let again = end(&sys, &dirs);
    assert_eq!(again["stopped"], json!(["late (pid 401)"]), "a retried handback did not look again");
    // After a clean handback, the next takeover has a boundary of its own.
    sys.start(proc(402, 61, "agent-app", "/usr/bin/x"), Answer::Polite);
    assert!(begin(&sys, &dirs).get("kept").is_none());
    assert_eq!(end(&sys, &dirs)["stopped_count"], 0, "what ran before the takeover was stopped");
}
