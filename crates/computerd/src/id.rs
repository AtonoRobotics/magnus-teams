//! A computer id: the only thing from Magnus that reaches a path.

use anyhow::{ensure, Result};

const MAX_ID_CHARS: usize = 32;

/// Lowercase letters and digits only, so an id cannot name a parent
/// directory, a separator or a command-line flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerId(String);

impl ComputerId {
    pub fn parse(raw: &str) -> Result<ComputerId> {
        ensure!(!raw.is_empty() && raw.len() <= MAX_ID_CHARS, "a computer id has 1 to {MAX_ID_CHARS} characters");
        ensure!(raw.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "a computer id has only lowercase letters and digits");
        Ok(ComputerId(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::ComputerId;

    #[test]
    fn an_id_cannot_name_a_path_or_a_flag() {
        for bad in ["", "..", "../x", "a/b", "/etc", "-x", "--help", "A", "a b", "a.b", "a\0", "\u{e9}", &"a".repeat(33)] {
            assert!(ComputerId::parse(bad).is_err(), "{bad:?} was accepted");
        }
        assert_eq!(ComputerId::parse("abc123").unwrap().as_str(), "abc123");
    }
}
