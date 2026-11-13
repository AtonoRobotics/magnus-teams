//! Child processes (microVMs, adapters) that die with the kernel.
//!
//! PR_SET_PDEATHSIG fires when the thread that forked the child exits, so all
//! children are forked from one thread that lives as long as the process.

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{mpsc, Mutex, OnceLock};

type Job = (Command, mpsc::Sender<std::io::Result<Child>>);

fn spawner() -> &'static Mutex<mpsc::Sender<Job>> {
    static TX: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        // A thread that cannot start drops `rx`, so every spawn after it fails
        // with "spawner stopped" instead of the process panicking.
        let started = std::thread::Builder::new().name("spawner".into()).spawn(move || {
            for (mut cmd, reply) in rx {
                unsafe {
                    cmd.pre_exec(|| {
                        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                let _ = reply.send(cmd.spawn());
            }
        });
        if let Err(e) = started {
            tracing::error!("the spawner thread did not start: {e}");
        }
        Mutex::new(tx)
    })
}

pub fn spawn(cmd: Command) -> std::io::Result<Child> {
    let (tx, rx) = mpsc::channel();
    spawner().lock().unwrap_or_else(|e| e.into_inner()).send((cmd, tx)).map_err(|_| std::io::Error::other("spawner stopped"))?;
    rx.recv().map_err(|_| std::io::Error::other("spawner stopped"))?
}

/// What a child process inherits from the kernel's own environment: read once
/// at startup, with the config, and passed in (logic never reads the environment).
#[derive(Clone, Debug, Default)]
pub struct HostEnv {
    /// `PATH`, empty when unset or not UTF-8.
    pub path: String,
    /// `HOME`, when set and UTF-8.
    pub home: Option<String>,
}

impl HostEnv {
    pub fn from_process() -> HostEnv {
        HostEnv { path: std::env::var("PATH").unwrap_or_default(), home: std::env::var("HOME").ok() }
    }
}
