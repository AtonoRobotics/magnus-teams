//! One-time codes, password reset links and sign-in links are taken out of
//! mail before it reaches the agent. They are live credentials, and an agent
//! that can read one can be told to use one.

const CODE_REMOVED: &str = "[one-time code removed]";
const LINK_REMOVED: &str = "[sign-in link removed]";
/// Words that make a number on the same line a login code.
const CODE_WORDS: [&str; 12] = ["code", "otp", "one-time", "one time", "passcode", "pin", "2fa", "two-factor", "verification", "verify", "authenticate", "security"];
/// A link holding one of these is a way into an account.
const LINK_WORDS: [&str; 16] =
    ["reset", "password", "passwd", "signin", "sign-in", "sign_in", "login", "log-in", "magic", "verify", "confirm", "activate", "auth", "token", "otp", "unlock"];
const ESCAPED_NEWLINE: &str = "\\n";
const MIN_CODE_CHARS: usize = 4;
const MAX_CODE_CHARS: usize = 10;

fn looks_like_code(word: &str) -> bool {
    (MIN_CODE_CHARS..=MAX_CODE_CHARS).contains(&word.len())
        && word.chars().all(|c| c.is_ascii_alphanumeric())
        && word.chars().filter(|c| c.is_ascii_digit()).count() >= word.len() / 2
}

fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | ')' | ']' | '\\')
}

/// Replaces every link in the line that leads into an account.
fn without_login_links(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find("http").map(|i| i + at) {
        let rest = &lower[found..];
        if !(rest.starts_with("http://") || rest.starts_with("https://")) {
            out.push_str(&line[at..found + 4]);
            at = found + 4;
            continue;
        }
        let end = found + rest.find(|c: char| !is_url_char(c)).unwrap_or(rest.len());
        out.push_str(&line[at..found]);
        let url = &lower[found..end];
        if LINK_WORDS.iter().any(|w| url.contains(w)) {
            out.push_str(LINK_REMOVED);
        } else {
            out.push_str(&line[found..end]);
        }
        at = end;
    }
    out.push_str(&line[at..]);
    out
}

/// Replaces every word that reads as a code, on a line that speaks of one.
fn without_codes(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if !CODE_WORDS.iter().any(|w| lower.contains(w)) {
        return line.to_string();
    }
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if looks_like_code(word) {
            out.push_str(CODE_REMOVED);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in line.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// One line, where a connector's JSON result writes its line breaks as `\\n`.
fn in_line(line: &str) -> String {
    let parts: Vec<String> = line.split(ESCAPED_NEWLINE).map(|l| without_codes(&without_login_links(l))).collect();
    parts.join(ESCAPED_NEWLINE)
}

/// Text as the agent may read it.
pub fn strip(text: &str) -> String {
    let cleaned: Vec<String> = text.split('\n').map(in_line).collect();
    cleaned.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_goes_and_the_message_stays() {
        let out = strip("Your verification code is 482913. It expires soon.\nLunch on Friday works.");
        assert!(!out.contains("482913"), "{out}");
        assert!(out.contains("It expires soon") && out.contains("Lunch on Friday works."), "{out}");
    }

    #[test]
    fn inside_a_json_result_each_line_is_its_own() {
        let out = strip(r#"{"text":"Reset at https://a.example/reset?t=1\nOrder 482913 shipped.\nYour code is 771122."}"#);
        assert!(!out.contains("t=1") && !out.contains("771122"), "{out}");
        assert!(out.contains("Order 482913 shipped."), "{out}");
    }

    #[test]
    fn a_number_with_nothing_about_a_code_stays() {
        assert_eq!(strip("The invoice total is 482913 pounds."), "The invoice total is 482913 pounds.");
    }

    #[test]
    fn reset_and_sign_in_links_go_and_others_stay() {
        let out = strip("Reset: https://a.example/reset?token=abc and https://a.example/magic-login/xyz or https://a.example/blog/post");
        assert!(!out.contains("token=abc") && !out.contains("magic-login"), "{out}");
        assert!(out.contains("https://a.example/blog/post"), "{out}");
    }

    #[test]
    fn a_link_at_the_end_of_a_bracket_is_cut_cleanly() {
        let out = strip("(see https://a.example/password/reset/1)");
        assert_eq!(out, format!("(see {LINK_REMOVED})"));
    }
}
