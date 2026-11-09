//! The identity provider's real email, as tests read it. The build box lets
//! no container reach the host, so the SMTP server (`mail-sink`, this
//! crate's own) runs in a container of its own that shares the test
//! Keycloak's network (`docker run --network container:magnus-claude-kc`,
//! on the Keycloak image already on the box): Keycloak sends to it at
//! 127.0.0.1, and it writes each message to a directory the test reads.

use magnus_invariants::appliance::bin;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Mail {
    pub to: Vec<String>,
    pub data: String,
}

pub struct Inbox {
    /// Where the sender sends.
    pub addr: SocketAddr,
    dir: tempfile::TempDir,
    container: Option<String>,
    local: Option<std::process::Child>,
}

impl Drop for Inbox {
    fn drop(&mut self) {
        if let Some(child) = self.local.as_mut() {
            let _ = child.kill();
        }
        if let Some(container) = &self.container {
            let _ = std::process::Command::new("timeout").args([DOCKER_PATIENCE, "docker", "rm", "-f", container]).output();
        }
    }
}

/// How long a docker command may take, in seconds, before the test fails
/// naming it: a container engine stalled under load held the invite test
/// silently until the runner killed it at 600 s (#246). These commands take
/// under a second on an idle box, but `docker run` took over a minute on
/// the loaded box (run 42167240885). Three minutes, beside the 249 s the
/// whole test has taken under load, stays under the runner's 600 s.
pub const DOCKER_PATIENCE: &str = "180";

fn docker_result(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("timeout").arg(DOCKER_PATIENCE).arg("docker").args(args).output().expect("run docker");
    // `timeout` exits 124 when it had to stop the command.
    if out.status.code() == Some(124) {
        return Err(format!("docker {} did not finish in {DOCKER_PATIENCE} s", args.join(" ")));
    }
    if !out.status.success() {
        return Err(format!("docker {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn docker(args: &[&str]) -> String {
    docker_result(args).unwrap_or_else(|e| panic!("{e}"))
}

impl Inbox {
    pub async fn start() -> Inbox {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // The Keycloak image runs as its own user.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let port = 20000 + (uuid::Uuid::new_v4().as_u128() % 20000) as u16;
        let image = docker(&["inspect", "magnus-claude-kc", "--format", "{{.Image}}"]);
        let sink = bin("mail-sink");
        let (sink_mount, out_mount) = (format!("{}:/mail-sink:ro", sink.display()), format!("{}:/out", dir.path().display()));
        // `docker run` has not answered within the patience under parallel
        // suites (runs 42257501972, 42261795000, 42262894435): try again
        // twice, each attempt under a fresh name. A timed-out run may still
        // have created its container late; the names cannot clash, the late
        // one fails to bind the port the winner holds and exits (the sink
        // panics when it cannot listen, `--rm` removes it), and anything
        // left is stopped by release/ci-clean.sh.
        let mut container = Err("no attempt ran".to_string());
        for _ in 0..3 {
            let name = format!("mail-sink-{}", uuid::Uuid::new_v4().as_simple());
            container = docker_result(&[
                "run",
                "-d",
                "--rm",
                "--network",
                "container:magnus-claude-kc",
                "--name",
                &name,
                "-v",
                &sink_mount,
                "-v",
                &out_mount,
                "--entrypoint",
                "/mail-sink",
                &image,
                &port.to_string(),
                "/out",
            ]);
            if container.is_ok() {
                break;
            }
        }
        let container = container.unwrap_or_else(|e| panic!("{e}"));
        let inbox = Inbox { addr: SocketAddr::from(([127, 0, 0, 1], port)), dir, container: Some(container), local: None };
        for _ in 0..300 {
            if inbox.dir.path().join("ready").exists() {
                return inbox;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let logs = std::process::Command::new("timeout")
            .args([DOCKER_PATIENCE, "docker", "logs", inbox.container.as_deref().unwrap_or_default()])
            .output()
            .map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
            .unwrap_or_default();
        panic!("the mail server beside Keycloak did not start: {logs}");
    }

    /// The same sink as a plain child process on loopback: for senders on
    /// this machine (the kernel mailer), with no Keycloak beside it.
    pub async fn start_local() -> Inbox {
        for _ in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let port = 20000 + (uuid::Uuid::new_v4().as_u128() % 20000) as u16;
            let mut child = std::process::Command::new(bin("mail-sink"))
                .arg(port.to_string())
                .arg(dir.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("run mail-sink");
            for _ in 0..100 {
                if dir.path().join("ready").exists() {
                    return Inbox { addr: SocketAddr::from(([127, 0, 0, 1], port)), dir, container: None, local: Some(child) };
                }
                if child.try_wait().unwrap_or(None).is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let _ = child.kill();
        }
        panic!("the local mail sink did not start");
    }

    fn all(&self) -> Vec<Mail> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.dir.path()).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "eml")).collect();
        files.sort();
        files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .map(|data| {
                let to = data.lines().next().and_then(|l| l.strip_prefix("X-Envelope-To: ")).unwrap_or_default().split(',').map(String::from).collect();
                Mail { to, data }
            })
            .collect()
    }

    /// The first mail to `to`, waiting up to a minute for it.
    pub async fn wait_for(&self, to: &str) -> Mail {
        for _ in 0..600 {
            if let Some(m) = self.all().into_iter().find(|m| m.to.iter().any(|t| t.eq_ignore_ascii_case(to))) {
                return m;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("no mail came for {to}; mail that came: {:?}", self.all().iter().map(|m| m.to.clone()).collect::<Vec<_>>());
    }

    pub fn count_for(&self, to: &str) -> usize {
        self.all().iter().filter(|m| m.to.iter().any(|t| t.eq_ignore_ascii_case(to))).count()
    }

    /// How many connections the server has taken: none means the sender
    /// never reached it.
    pub fn connections(&self) -> usize {
        std::fs::metadata(self.dir.path().join("connections")).map(|m| m.len() as usize).unwrap_or(0)
    }
}

/// The text of a mail with its transfer encodings undone: quoted-printable
/// soft breaks and escapes, and base64 parts.
pub fn text_of(data: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    // Quoted-printable: `=\r\n` joins lines, `=XX` is a byte.
    let joined = data.replace("=\r\n", "").replace("=\n", "");
    let b = joined.as_bytes();
    let mut bytes = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                bytes.push(x);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    out.push_str(&String::from_utf8_lossy(&bytes));
    // Base64 parts: each run of base64 lines after a blank line.
    for block in data.split("\r\n\r\n").chain(data.split("\n\n")) {
        let compact: String = block.lines().map(str::trim).collect();
        if compact.len() >= 40 && compact.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
            if let Ok(d) = base64::engine::general_purpose::STANDARD.decode(compact) {
                out.push('\n');
                out.push_str(&String::from_utf8_lossy(&d));
            }
        }
    }
    out
}

/// The first link in a mail whose address contains `part`.
pub fn link_in(data: &str, part: &str) -> Option<String> {
    let t = text_of(data);
    t.split(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>' || c == '\'')
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| w.replace("&amp;", "&"))
        .find(|w| w.contains(part))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_link_is_found_through_quoted_printable_and_base64() {
        let qp = "Content-Transfer-Encoding: quoted-printable\r\n\r\nClick <a href=3D\"http://id.test/realms/r/login-actions/action-token?key=3Dab=\r\ncd&amp;client_id=3Dx\">here</a>\r\n";
        assert_eq!(super::link_in(qp, "action-token").as_deref(), Some("http://id.test/realms/r/login-actions/action-token?key=abcd&client_id=x"));
        use base64::Engine;
        let body = base64::engine::general_purpose::STANDARD.encode("Set it here: http://id.test/realms/r/login-actions/action-token?key=zz9 thanks");
        let b64 = format!("Content-Transfer-Encoding: base64\r\n\r\n{}\r\n{}\r\n", &body[..40], &body[40..]);
        assert_eq!(super::link_in(&b64, "action-token").as_deref(), Some("http://id.test/realms/r/login-actions/action-token?key=zz9"));
    }
}
