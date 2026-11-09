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
        std::thread::Builder::new()
            .name("spawner".into())
            .spawn(move || {
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
            })
            .expect("spawner thread");
        Mutex::new(tx)
    })
}

pub fn spawn(cmd: Command) -> std::io::Result<Child> {
    let (tx, rx) = mpsc::channel();
    spawner().lock().unwrap().send((cmd, tx)).map_err(|_| std::io::Error::other("spawner stopped"))?;
    rx.recv().map_err(|_| std::io::Error::other("spawner stopped"))?
}
