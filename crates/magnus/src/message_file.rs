//! One message as a Markdown file, the same from a person's own chat and
//! from a team channel: who said it and when, then what they said, under a
//! name made of its first words.

/// How many of a message's first words name its file.
const NAME_WORDS: usize = 6;

/// The file's text: `**who** · at`, a blank line, the words.
pub fn markdown(who: &str, at: &str, text: &str) -> String {
    format!("**{who}** · {at}\n\n{text}\n")
}

/// A file name from the message's first words, with nothing a name may not
/// hold; `message.md` when it has none.
pub fn file_name(text: &str) -> String {
    let words: String = text.split_whitespace().take(NAME_WORDS).collect::<Vec<_>>().join(" ");
    let clean: String = words.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { ' ' }).collect();
    let name = clean.split_whitespace().collect::<Vec<_>>().join("-");
    format!("{}.md", if name.is_empty() { "message" } else { &name })
}

/// A time as the app's messages carry it.
pub fn at_text(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_named_by_its_first_words_and_never_climbs_out() {
        assert_eq!(file_name("The launch moves to Friday, says Dana today"), "The-launch-moves-to-Friday-says.md");
        assert_eq!(file_name("../../etc/passwd"), "etc-passwd.md");
        assert_eq!(file_name("  "), "message.md");
    }

    #[test]
    fn the_markdown_names_who_and_when() {
        assert_eq!(markdown("Mo", "2026-11-13T10:00:00Z", "hi"), "**Mo** · 2026-11-13T10:00:00Z\n\nhi\n");
    }

    #[test]
    fn a_time_reads_as_the_app_has_it() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-13T10:00:00Z").unwrap().with_timezone(&chrono::Utc);
        assert_eq!(at_text(at), serde_json::to_value(at).unwrap().as_str().unwrap());
    }
}
