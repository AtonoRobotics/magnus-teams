//! Asks a computer's guest agent something, for computerd's own use. The
//! vsock socket is in the VM's own directory, so root still does not open it:
//! the relay process does, locked in the jail as the VM's user.

use super::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::Stdio;

const GUEST_ANSWER: Duration = Duration::from_secs(30);
const GUEST_RETRY: Duration = Duration::from_millis(100);

impl Computers {
    fn guest_once(&self, id: &ComputerId, uid: u32, req: &Value) -> Result<Value> {
        let (mine, theirs) = UnixStream::pair()?;
        mine.set_read_timeout(Some(GUEST_ANSWER))?;
        let out = theirs.try_clone()?;
        let mut relay = Command::new(std::env::current_exe().context("find this program")?)
            .env_clear()
            .arg("relay")
            .arg(self.jail_root(id))
            .arg(uid.to_string())
            .stdin(Stdio::from(OwnedFd::from(theirs)))
            .stdout(Stdio::from(OwnedFd::from(out)))
            .stderr(Stdio::null())
            .spawn()
            .context("run the relay")?;
        let answer = (|| {
            let mut to_guest = mine.try_clone()?;
            let mut from_guest = BufReader::new(mine.try_clone()?);
            let mut line = String::new();
            from_guest.read_line(&mut line)?;
            let ack: Value = serde_json::from_str(&line).context("the relay's answer")?;
            ensure!(ack["ok"] == true, "{}", ack["error"].as_str().unwrap_or("the guest did not answer"));
            writeln!(to_guest, "{req}")?;
            line.clear();
            from_guest.read_line(&mut line)?;
            serde_json::from_str::<Value>(&line).context("the guest's answer")
        })();
        let _ = relay.kill();
        let _ = relay.wait();
        answer
    }

    /// Keeps trying for a moment: a guest just resumed may still be coming up.
    pub(super) fn ask_guest(&self, id: &ComputerId, uid: u32, req: &Value) -> Result<Value> {
        let started = Instant::now();
        loop {
            match self.guest_once(id, uid, req) {
                Ok(v) => return Ok(v),
                Err(e) if started.elapsed() > GUEST_ANSWER => return Err(e),
                Err(_) => std::thread::sleep(GUEST_RETRY),
            }
        }
    }
}
