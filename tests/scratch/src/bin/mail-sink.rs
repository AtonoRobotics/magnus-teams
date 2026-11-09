//! An SMTP server for tests, run beside the test Keycloak in its own network
//! (`docker run --network container:magnus-claude-kc`): the build box lets no
//! container reach the host, so the identity provider's real email is taken
//! here, on 127.0.0.1 as Keycloak sees it, and written to a directory the
//! test reads (`magnus_scratch::mail`). Plain SMTP, no TLS, no sign-in; the
//! standard library only, so it runs on the Keycloak image as built here.
//!
//! mail-sink PORT DIR
//!   DIR/ready         once it listens
//!   DIR/connections   one byte per connection taken
//!   DIR/N.eml         each message: "X-Envelope-To: a,b" then the data

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(port), Some(dir)) = (args.get(1).and_then(|p| p.parse::<u16>().ok()), args.get(2)) else {
        eprintln!("usage: mail-sink PORT DIR");
        std::process::exit(2);
    };
    let dir = PathBuf::from(dir);
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| panic!("listen on 127.0.0.1:{port}: {e}"));
    std::fs::write(dir.join("ready"), b"").expect("write ready");
    let n = Arc::new(AtomicUsize::new(0));
    for s in l.incoming().flatten() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("connections")) {
            let _ = f.write_all(b".");
        }
        let (dir, n) = (dir.clone(), n.clone());
        std::thread::spawn(move || {
            let _ = session(s, &dir, &n);
        });
    }
}

fn session(s: TcpStream, dir: &Path, n: &AtomicUsize) -> std::io::Result<()> {
    let mut w = s.try_clone()?;
    let mut r = BufReader::new(s);
    w.write_all(b"220 inbox.test ESMTP\r\n")?;
    let mut to: Vec<String> = vec![];
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let cmd = line.trim_end().to_ascii_uppercase();
        if cmd.starts_with("EHLO") {
            w.write_all(b"250-inbox.test\r\n250 8BITMIME\r\n")?;
        } else if cmd.starts_with("RCPT TO:") {
            to.push(line.trim_end()[8..].trim().trim_matches(['<', '>']).to_string());
            w.write_all(b"250 OK\r\n")?;
        } else if cmd == "DATA" {
            w.write_all(b"354 go on\r\n")?;
            let mut data = format!("X-Envelope-To: {}\r\n", to.join(","));
            loop {
                line.clear();
                if r.read_line(&mut line)? == 0 {
                    return Ok(());
                }
                if line == ".\r\n" || line == ".\n" {
                    break;
                }
                data.push_str(if line.starts_with("..") { &line[1..] } else { &line });
            }
            let i = n.fetch_add(1, Ordering::SeqCst);
            let tmp = dir.join(format!("{i}.tmp"));
            std::fs::write(&tmp, data)?;
            std::fs::rename(&tmp, dir.join(format!("{i}.eml")))?;
            to.clear();
            w.write_all(b"250 OK\r\n")?;
        } else if cmd == "QUIT" {
            w.write_all(b"221 bye\r\n")?;
            return Ok(());
        } else {
            // HELO, MAIL FROM, RSET, NOOP
            w.write_all(b"250 OK\r\n")?;
        }
    }
}
