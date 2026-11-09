//! Search bytes for a secret in its common encodings.

use base64::Engine;
use std::path::{Path, PathBuf};

/// The secret as it might appear: raw, base64 (standard and URL-safe), hex.
pub fn encodings(secret: &str) -> Vec<Vec<u8>> {
    let b = secret.as_bytes();
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    vec![b.to_vec(), base64::engine::general_purpose::STANDARD.encode(b).into_bytes(), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b).into_bytes(), hex.into_bytes()]
}

pub fn contains(hay: &[u8], secret: &str) -> bool {
    encodings(secret).iter().any(|n| !n.is_empty() && hay.windows(n.len()).any(|w| w == n.as_slice()))
}

/// Every file under `dir` whose bytes contain `secret` in any encoding.
pub fn files_containing(dir: &Path, secret: &str) -> Vec<PathBuf> {
    let mut hits = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                if let Ok(bytes) = std::fs::read(&p) {
                    if contains(&bytes, secret) {
                        hits.push(p);
                    }
                }
            }
        }
    }
    hits
}
