//! What the proxy carried to each computer, for an installer's provenance
//! (`decisions/2026-11-23-installer-provenance.md`). A download is known by
//! the sha256 of its decoded body, the bytes the computer saves, under the
//! host the computer asked and every host a redirect to it came through, so
//! a release link that redirects to a file host still names its source. The
//! record is kept on disk, one small file per computer, so a restart forgets
//! nothing.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Downloads kept per computer, to check a manifest entry against.
const MAX_DOWNLOADS_REMEMBERED: usize = 512;
/// Redirects kept per computer, newest last: a download follows its
/// redirect at once.
const MAX_REDIRECTS_REMEMBERED: usize = 64;
/// The longest chunk-size or trailer line read before the framing is taken
/// as broken.
const MAX_CHUNK_LINE_BYTES: usize = 4096;
const FOLDER: &str = "computer-downloads";

/// The downloads of every computer, on disk under the server's state.
pub(in crate::computer) struct Carried {
    dir: PathBuf,
}

/// One writer at a time, so two downloads finishing together keep both.
static WRITING: Mutex<()> = Mutex::new(());

impl Carried {
    pub(in crate::computer) fn at(state_dir: &Path) -> Carried {
        Carried { dir: state_dir.join(FOLDER) }
    }

    fn file(&self, computer: &str) -> PathBuf {
        self.dir.join(format!("{computer}.json"))
    }

    fn read(&self, computer: &str) -> VecDeque<(String, String)> {
        std::fs::read(self.file(computer)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Records that this file reached `computer` from each of `hosts`.
    pub(super) fn saw(&self, computer: &str, hosts: &[String], sha256: &str) -> Result<()> {
        let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let mut mine = self.read(computer);
        for host in hosts {
            let entry = (host.to_ascii_lowercase(), sha256.to_string());
            if mine.contains(&entry) {
                continue;
            }
            if mine.len() >= MAX_DOWNLOADS_REMEMBERED {
                mine.pop_front();
            }
            mine.push_back(entry);
        }
        std::fs::create_dir_all(&self.dir).context("make the downloads folder")?;
        let part = self.dir.join(format!("{computer}.json.part"));
        std::fs::write(&part, serde_json::to_vec(&mine)?).context("record a download")?;
        std::fs::rename(&part, self.file(computer)).context("record a download")
    }

    /// Whether the proxy carried this file from this host to this computer.
    pub(in crate::computer) fn carried(&self, computer: &str, host: &str, sha256: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.read(computer).iter().any(|(h, s)| *h == host && s == sha256)
    }

    /// A computer that is gone takes its downloads with it.
    pub(in crate::computer) fn forget(&self, computer: &str) {
        let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(self.file(computer));
    }
}

type Redirects = HashMap<String, VecDeque<(String, String)>>;

fn redirects() -> &'static Mutex<Redirects> {
    static SEEN: OnceLock<Mutex<Redirects>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
}

/// `from` (a full URL) answered with a redirect to `location`, which may be
/// relative. The host it first came from goes with it.
pub(super) fn redirected(computer: &str, from: &str, location: &str) {
    let Some(to) = url::Url::parse(from).ok().and_then(|base| base.join(location).ok()) else { return };
    let origin = origins(computer, from).pop().or_else(|| url::Url::parse(from).ok().and_then(|u| u.host_str().map(str::to_string)));
    let Some(origin) = origin else { return };
    let mut seen = redirects().lock().unwrap_or_else(|e| e.into_inner());
    let mine = seen.entry(computer.to_string()).or_default();
    if mine.len() >= MAX_REDIRECTS_REMEMBERED {
        mine.pop_front();
    }
    mine.push_back((to.to_string(), origin.to_ascii_lowercase()));
}

/// The hosts a redirect to `url` came from, oldest first.
pub(super) fn origins(computer: &str, url: &str) -> Vec<String> {
    let Ok(url) = url::Url::parse(url) else { return vec![] };
    let seen = redirects().lock().unwrap_or_else(|e| e.into_inner());
    seen.get(computer).map(|mine| mine.iter().filter(|(to, _)| *to == url.as_str()).map(|(_, from)| from.clone()).collect()).unwrap_or_default()
}

/// The sha256 of a response body as the computer saves it: with a chunked
/// transfer's framing taken off.
pub(super) struct BodyHash {
    hash: Sha256,
    chunks: Option<Dechunk>,
}

impl BodyHash {
    pub(super) fn new(chunked: bool) -> BodyHash {
        BodyHash { hash: Sha256::new(), chunks: chunked.then(Dechunk::default) }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) {
        match &mut self.chunks {
            Some(d) => d.push(bytes, &mut self.hash),
            None => self.hash.update(bytes),
        }
    }

    /// The hash, or None when the chunked framing was broken or cut short.
    pub(super) fn finish(self) -> Option<String> {
        if self.chunks.as_ref().is_some_and(|d| d.state != Step::Done) {
            return None;
        }
        Some(hex::encode(self.hash.finalize()))
    }
}

#[derive(Default, PartialEq, Eq, Debug)]
enum Step {
    /// Reading a chunk-size line.
    #[default]
    Size,
    /// Reading this many bytes of a chunk.
    Data(u64),
    /// The CRLF after a chunk's data.
    DataEnd,
    /// Trailer lines after the last chunk, until an empty one.
    Trailer,
    Done,
    Broken,
}

#[derive(Default)]
struct Dechunk {
    state: Step,
    line: Vec<u8>,
}

impl Dechunk {
    fn push(&mut self, mut bytes: &[u8], out: &mut Sha256) {
        while !bytes.is_empty() {
            match self.state {
                Step::Data(left) => {
                    let n = usize::try_from(left).unwrap_or(usize::MAX).min(bytes.len());
                    out.update(&bytes[..n]);
                    bytes = &bytes[n..];
                    let left = left - n as u64;
                    self.state = if left == 0 { Step::DataEnd } else { Step::Data(left) };
                }
                Step::Done | Step::Broken => return,
                Step::Size | Step::DataEnd | Step::Trailer => {
                    let Some(end) = bytes.iter().position(|b| *b == b'\n') else {
                        self.line.extend_from_slice(bytes);
                        if self.line.len() > MAX_CHUNK_LINE_BYTES {
                            self.state = Step::Broken;
                        }
                        return;
                    };
                    self.line.extend_from_slice(&bytes[..end]);
                    bytes = &bytes[end + 1..];
                    let line = std::mem::take(&mut self.line);
                    let line = line.strip_suffix(b"\r").unwrap_or(&line);
                    self.state = self.after_line(line);
                }
            }
        }
    }

    fn after_line(&self, line: &[u8]) -> Step {
        match self.state {
            Step::DataEnd if line.is_empty() => Step::Size,
            Step::Size => {
                let text = String::from_utf8_lossy(line);
                let size = text.split(';').next().unwrap_or_default().trim();
                match u64::from_str_radix(size, 16) {
                    Ok(0) => Step::Trailer,
                    Ok(n) => Step::Data(n),
                    Err(_) => Step::Broken,
                }
            }
            Step::Trailer if line.is_empty() => Step::Done,
            Step::Trailer => Step::Trailer,
            _ => Step::Broken,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    #[test]
    fn a_chunked_download_is_known_by_its_decoded_body_however_it_arrives() {
        let framed = b"5;ext=1\r\nhello\r\n7\r\n, world\r\n0\r\nX-Trailer: y\r\n\r\n";
        for split in [1, 2, 3, 7, framed.len()] {
            let mut h = BodyHash::new(true);
            for piece in framed.chunks(split) {
                h.push(piece);
            }
            assert_eq!(h.finish(), Some(sha(b"hello, world")), "split every {split} bytes");
        }
        let mut cut = BodyHash::new(true);
        cut.push(b"5\r\nhel");
        assert_eq!(cut.finish(), None, "a body cut short was taken as whole");
        let mut plain = BodyHash::new(false);
        plain.push(b"hello");
        assert_eq!(plain.finish(), Some(sha(b"hello")));
    }

    #[test]
    fn a_download_through_a_redirect_names_where_it_was_asked_for() {
        let computer = "c-redirect-test";
        redirected(computer, "https://github.com/o/r/releases/download/v1/app.deb", "https://objects.example/abc?sig=1");
        redirected(computer, "https://objects.example/abc?sig=1", "/final.deb");
        assert_eq!(origins(computer, "https://objects.example/abc?sig=1"), ["github.com"]);
        assert_eq!(origins(computer, "https://objects.example/final.deb"), ["github.com"], "a second hop lost the first source");
        assert!(origins("another", "https://objects.example/final.deb").is_empty());
    }

    #[test]
    fn what_was_carried_is_still_known_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        Carried::at(dir.path()).saw("c1", &["github.com".into(), "Objects.Example".into()], "abc").unwrap();
        let after_restart = Carried::at(dir.path());
        assert!(after_restart.carried("c1", "github.com", "abc"));
        assert!(after_restart.carried("c1", "objects.example", "abc"));
        assert!(!after_restart.carried("c1", "github.com", "abd"));
        assert!(!after_restart.carried("c2", "github.com", "abc"));
        after_restart.forget("c1");
        assert!(!Carried::at(dir.path()).carried("c1", "github.com", "abc"));
    }
}
