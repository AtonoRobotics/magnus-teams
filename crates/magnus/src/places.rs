//! Where an app's write lands, when no recipient names it: a GitHub
//! commit, pull request or comment goes to one repository. The manifest
//! names the place as a template over the call's own fields
//! (`place = "github.com/{owner}/{repo}"`), and the place is the action's
//! destination wherever one is asked about: case 3 of the asks, an Allow's
//! scope, a remembered destination and the person's rules.
//! (decisions/2026-11-19-a-repository-is-a-destination.md)

use serde_json::Value;

/// The place `template` names for this call's `input`, lowercased; None
/// when a field it names is missing, empty, or not a plain name.
pub fn place(template: &str, input: &Value) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}')? + open;
        let value = input.get(&rest[open + 1..close])?.as_str()?.trim();
        let plain = !value.is_empty() && value.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)) && !value.starts_with('.');
        if !plain {
            return None;
        }
        out.push_str(value);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out.to_lowercase())
}

/// Whether the person's own words name this place: its path after the
/// host (`owner/repo`), as a whole name, or the place itself in a link.
/// Injected text can no more choose a repository the person named than a
/// recipient they wrote (boundary, recipient provenance).
pub fn named_in(said: &[String], place: &str) -> bool {
    let Some((_, path)) = place.split_once('/') else { return false };
    let part = |c: char| c.is_alphanumeric() || "-_.".contains(c);
    said.iter().any(|text| {
        let text = text.to_lowercase();
        text.match_indices(path).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + path.len()..].chars().next();
            let edge_before = before.is_none_or(|c| !part(c) && c != '/') || text[..at].ends_with(&place[..place.len() - path.len()]);
            // A sentence's full stop ends the name; `site.git` is another word.
            let edge_after = after.is_none_or(|c| !part(c) || (c == '.' && text[at + path.len() + 1..].chars().next().is_none_or(|c| !part(c))));
            edge_before && edge_after
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const GITHUB: &str = "github.com/{owner}/{repo}";

    #[test]
    fn a_write_lands_in_the_repository_its_call_names() {
        assert_eq!(place(GITHUB, &json!({ "owner": "Acme", "repo": "Site", "title": "x" })).as_deref(), Some("github.com/acme/site"));
        assert_eq!(place(GITHUB, &json!({ "owner": "acme" })), None);
        assert_eq!(place(GITHUB, &json!({ "owner": "acme", "repo": "" })), None);
        assert_eq!(place(GITHUB, &json!({ "owner": "acme", "repo": "a/b" })), None);
        assert_eq!(place(GITHUB, &json!({ "owner": "acme", "repo": ".." })), None);
        assert_eq!(place(GITHUB, &json!({ "owner": "acme", "repo": 3 })), None);
        assert_eq!(place("github.com/{owner", &json!({ "owner": "acme" })), None);
    }

    /// Without a place a GitHub write had no destination, so the Rule of
    /// Two asked before every one and no Allow could be remembered.
    #[test]
    fn a_github_write_goes_to_its_repository() {
        let a = crate::boundary::Action {
            run: "r".into(),
            call_id: "c".into(),
            step_id: "s".into(),
            tool: "github__push_files".into(),
            input: json!({ "owner": "acme", "repo": "site", "branch": "fix", "files": [], "message": "Fix the header" }),
            issuer: crate::boundary::Issuer::Model,
        };
        assert_eq!(crate::rules::destinations_of(&a, Some(GITHUB), None), ["github.com/acme/site"]);
        assert!(crate::rules::destinations_of(&a, None, None).is_empty());
        let m: crate::catalog::Manifest = toml::from_str(include_str!("../../../catalog/github.toml")).unwrap();
        assert_eq!(m.place.as_deref(), Some(GITHUB));
    }

    #[test]
    fn the_person_names_a_repository_by_its_path_or_its_link() {
        let p = "github.com/acme/site";
        let said = |t: &str| vec![t.to_string()];
        assert!(named_in(&said("Fix the header in acme/site and open a PR."), p));
        assert!(named_in(&said("see https://github.com/Acme/Site/issues/4"), p));
        assert!(named_in(&said("acme/site"), p));
        assert!(!named_in(&said("Fix acme/site-old"), p));
        assert!(!named_in(&said("Fix bigacme/site"), p));
        assert!(!named_in(&said("Fix gitlab.com/x/acme/site"), p));
        assert!(!named_in(&said("Fix the site"), p));
        assert!(!named_in(&[], p));
    }
}
