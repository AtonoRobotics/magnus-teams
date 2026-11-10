//! Children this process inherited from the image it replaced.
//!
//! An update (and a restart in place) execs the new release in the same
//! process. The adapters, microVMs and sidecars the old image started are
//! still this process's children: the spawner thread that forked them is gone,
//! so PR_SET_PDEATHSIG kills them, but the new image holds no handle to wait
//! on them and each would stay a zombie until the process exits. `serve`
//! collects them before it spawns anything of its own.

/// The process ids whose parent is this process, read from /proc.
pub fn children() -> Vec<i32> {
    let me = std::process::id() as i32;
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok()).filter(|pid| parent_of(*pid) == Some(me)).collect()
}

/// The parent of `pid`, from the field after the state in /proc/<pid>/stat
/// (the name before it may hold spaces and parentheses, so parse from the last ')').
fn parent_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// Wait for each of `pids` on a thread of its own, so a zombie is collected
/// at once and one still dying is collected when it dies.
pub fn reap(pids: Vec<i32>) {
    if pids.is_empty() {
        return;
    }
    tracing::info!("collecting {} processes left by the previous release", pids.len());
    let spawned = std::thread::Builder::new().name("reaper".into()).spawn(move || {
        for pid in pids {
            let mut status = 0;
            // SAFETY: waitpid on a specific child id with a valid status pointer.
            unsafe { libc::waitpid(pid, &mut status, 0) };
        }
    });
    if let Err(e) = spawned {
        tracing::error!("could not start the reaper thread: {e}");
    }
}

/// Collect every child this process has before it has started any.
pub fn reap_inherited() {
    reap(children());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const ZOMBIE_WAIT: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(20);

    fn state(pid: i32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat[stat.rfind(')')? + 1..].split_whitespace().next()?.chars().next()
    }

    fn until(what: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < ZOMBIE_WAIT {
            if what() {
                return true;
            }
            std::thread::sleep(POLL);
        }
        false
    }

    /// A child whose handle is gone (as after an exec) becomes a zombie of
    /// this process; reaping the inherited children collects it.
    #[test]
    fn a_child_left_without_a_handle_is_collected() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id() as i32;
        std::mem::forget(child);
        assert!(until(|| state(pid) == Some('Z')), "the child did not exit");
        let mine: Vec<i32> = children().into_iter().filter(|p| *p == pid).collect();
        assert_eq!(mine, vec![pid], "the zombie is listed as a child");
        reap(mine);
        assert!(until(|| state(pid).is_none()), "pid {pid} is still a zombie of this process");
    }
}
