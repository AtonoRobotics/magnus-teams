//! The provider credentials a connector's contract test needs for the part
//! that runs against the owner's test account
//! (decisions/2026-11-13-ci-and-test-hygiene.md).
//!
//! Where contracts are required, on CI (`CI`, which GitHub Actions sets) or
//! with `MAGNUS_REQUIRE_CONTRACTS`, a missing credential fails the test.
//! Elsewhere the test skips that part and says `SKIPPED: <reason>` on the
//! process's own stderr, which libtest does not capture, so a skip is seen
//! even when the test is reported `ok`.

use std::io::Write;

/// The variables that make a missing credential a failure.
pub const REQUIRE_VARS: [&str; 2] = ["CI", "MAGNUS_REQUIRE_CONTRACTS"];

/// The values of `vars`, or `None` after saying `SKIPPED` when one is missing
/// and contracts are not required. Panics when one is missing and they are.
/// `what` names the part that needs them, `account` whose they are.
pub fn credentials<const N: usize>(what: &str, account: &str, vars: [&str; N]) -> Option<[String; N]> {
    let got = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    match decide(what, account, vars, got, required(|k| std::env::var(k).ok())) {
        Ok(v) => Some(v),
        Err(Missing::Skip(reason)) => {
            let _ = writeln!(std::io::stderr(), "SKIPPED: {reason}");
            None
        }
        Err(Missing::Fail(reason)) => panic!("{reason}"),
    }
}

/// Whether a missing credential fails: one of `REQUIRE_VARS` is set to
/// anything but empty, `0` or `false`.
fn required(get: impl Fn(&str) -> Option<String>) -> bool {
    REQUIRE_VARS.iter().any(|k| get(k).is_some_and(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "0" | "false")))
}

#[derive(Debug, PartialEq)]
enum Missing {
    Skip(String),
    Fail(String),
}

fn decide<const N: usize>(what: &str, account: &str, vars: [&str; N], get: impl Fn(&str) -> Option<String>, required: bool) -> Result<[String; N], Missing> {
    let missing: Vec<&str> = vars.iter().copied().filter(|k| get(k).is_none()).collect();
    if missing.is_empty() {
        return Ok(vars.map(|k| get(k).unwrap_or_default()));
    }
    let missing = missing.join(", ");
    if required {
        Err(Missing::Fail(format!(
            "{what} needs {account}, and {missing} is not set; contracts are required here ({} is set), so a missing credential fails instead of skipping",
            REQUIRE_VARS.join(" or ")
        )))
    } else {
        Err(Missing::Skip(format!("{what} needs {account}; {missing} is not set, so it was not run")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned().filter(|v| !v.is_empty())
    }

    #[test]
    fn ci_or_the_flag_makes_contracts_required() {
        assert!(!required(env(&[])));
        assert!(required(env(&[("CI", "true")])));
        assert!(required(env(&[("MAGNUS_REQUIRE_CONTRACTS", "1")])));
        assert!(!required(env(&[("CI", "false"), ("MAGNUS_REQUIRE_CONTRACTS", "0")])));
    }

    #[test]
    fn a_missing_credential_skips_here_and_fails_where_required() {
        let e = env(&[("A", "a"), ("B", "")]);
        match decide("Gmail's contract test", "the test account", ["A", "B"], &e, false) {
            Err(Missing::Skip(r)) => assert!(r.contains("B is not set") && !r.contains("A,"), "{r}"),
            other => panic!("{other:?}"),
        }
        match decide("Gmail's contract test", "the test account", ["A", "B"], &e, true) {
            Err(Missing::Fail(r)) => assert!(r.contains("B is not set") && r.contains("MAGNUS_REQUIRE_CONTRACTS"), "{r}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(decide("x", "y", ["A"], &e, true), Ok(["a".to_string()]));
    }
}
