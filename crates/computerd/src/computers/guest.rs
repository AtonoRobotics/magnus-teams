//! Asks a computer's guest agent something, for computerd's own use. The
//! vsock socket is in the VM's own directory, so root still does not open it:
//! the relay process does, locked in the jail as the VM's user.

use super::*;
use magnus_computerd_proto::guest::{GuestRequest, MAX_GUEST_REPLY_BYTES};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::Stdio;

const GUEST_ANSWER_SECS: Duration = Duration::from_secs(30);
const GUEST_RETRY_MS: Duration = Duration::from_millis(100);

impl Computers {
    fn guest_once(&self, id: &ComputerId, uid: u32, req: &GuestRequest) -> Result<Value> {
        let (mine, theirs) = UnixStream::pair()?;
        mine.set_read_timeout(Some(GUEST_ANSWER_SECS))?;
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
            // The guest runs the model's commands: it is not trusted to end its line.
            let mut from_guest = BufReader::new(mine.try_clone()?.take(MAX_GUEST_REPLY_BYTES));
            let mut line = String::new();
            from_guest.read_line(&mut line)?;
            let ack: Value = serde_json::from_str(&line).context("the relay's answer")?;
            ensure!(ack["ok"] == true, "{}", ack["error"].as_str().unwrap_or("the guest did not answer"));
            writeln!(to_guest, "{}", serde_json::to_string(req)?)?;
            line.clear();
            from_guest.read_line(&mut line)?;
            ensure!(line.ends_with('\n'), "the guest's answer was cut short");
            serde_json::from_str::<Value>(&line).context("the guest's answer")
        })();
        let _ = relay.kill();
        let _ = relay.wait();
        answer
    }

    /// Keeps trying for a moment: a guest just resumed may still be coming up.
    pub(super) fn ask_guest(&self, id: &ComputerId, uid: u32, req: &GuestRequest) -> Result<Value> {
        let started = Instant::now();
        loop {
            match self.guest_once(id, uid, req) {
                Ok(v) => return Ok(v),
                Err(e) if started.elapsed() > GUEST_ANSWER_SECS => return Err(e),
                Err(_) => std::thread::sleep(GUEST_RETRY_MS),
            }
        }
    }
}
