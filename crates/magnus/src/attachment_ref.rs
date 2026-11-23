//! Which attachment from outside a tool's `name` means
//! (decisions/2026-11-23-held-attachments-known-by-their-contents.md).
//!
//! Mail reuses names (`image0.jpeg`, `invite.ics`), so an attachment is known
//! by its contents: its reference is its name and the first hex of its
//! SHA-256 (`image0.jpeg#1a2b3c4d`, `attachment_hold::reference`). A
//! reference finds that attachment; a bare name finds one only when no other
//! attachment of the task shares it, and is refused, with the references,
//! when several do.
use crate::attachment_hold::reference;
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// What a name or a reference finds among a task's attachments.
#[derive(Debug, PartialEq)]
pub enum Found {
    One(Value),
    None,
    /// Several attachments share the name: their references.
    Ambiguous(Vec<String>),
}

/// An attachment's SHA-256: recorded for a fetched one, worked out from
/// the bytes of one the task started with.
fn sha256_of(file: &Value) -> String {
    match file["sha256"].as_str() {
        Some(sha) => sha.to_string(),
        None => {
            let bytes = base64::engine::general_purpose::STANDARD.decode(file["data_b64"].as_str().unwrap_or_default()).unwrap_or_default();
            hex::encode(Sha256::digest(&bytes))
        }
    }
}

/// The attachment among `files` that `asked` names.
pub fn pick(files: &[Value], asked: &str) -> Found {
    let refd: Vec<(String, &Value)> = files.iter().map(|f| (reference(f["name"].as_str().unwrap_or_default(), &sha256_of(f)), f)).collect();
    if let Some((_, f)) = refd.iter().find(|(r, _)| r == asked) {
        return Found::One((*f).clone());
    }
    let named: Vec<&(String, &Value)> = refd.iter().filter(|(_, f)| f["name"] == asked).collect();
    let mut refs: Vec<String> = named.iter().map(|(r, _)| r.clone()).collect();
    refs.sort();
    refs.dedup();
    match (refs.len(), named.first()) {
        (1, Some((_, f))) => Found::One((*f).clone()),
        (0, _) | (_, None) => Found::None,
        _ => Found::Ambiguous(refs),
    }
}

/// What the model is told when a bare name fits several attachments.
pub fn ambiguous(asked: &str, refs: &[String]) -> String {
    format!("Several attachments from outside are called {asked}: {}. Name the one you mean by its reference.", refs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(name: &str, b64: &str) -> Value {
        json!({ "name": name, "media_type": "image/jpeg", "data_b64": b64, "source": "email" })
    }

    /// Two attachments called image0.jpeg with different bytes: each opens
    /// as its own by reference, and the bare name is refused with both.
    #[test]
    fn a_reference_finds_its_own_attachment_and_a_shared_name_is_refused() {
        let first = crate::attachment_hold::hold("run-ref-1", vec![file("image0.jpeg", "Rmlyc3Q=")]);
        let second = crate::attachment_hold::hold("run-ref-1", vec![file("image0.jpeg", "U2Vjb25k")]);
        let files = vec![first[0].clone(), second[0].clone(), file("plan.txt", "cGxhbg==")];
        for (kept, bytes) in [(&first[0], "Rmlyc3Q="), (&second[0], "U2Vjb25k")] {
            let asked = kept["ref"].as_str().unwrap_or_default();
            let Found::One(f) = pick(&files, asked) else { panic!("{asked} found nothing") };
            assert_eq!(f["sha256"], kept["sha256"], "{asked} found another attachment");
            let held = crate::attachment_hold::held("run-ref-1", f["sha256"].as_str().unwrap_or_default());
            assert_eq!(held.as_deref(), Some(bytes), "{asked} opened as another attachment's bytes");
        }
        let Found::Ambiguous(refs) = pick(&files, "image0.jpeg") else { panic!("a shared name was taken as one attachment") };
        assert_eq!(refs.len(), 2);
        assert!(ambiguous("image0.jpeg", &refs).contains(&refs[0]) && ambiguous("image0.jpeg", &refs).contains(&refs[1]));
        crate::attachment_hold::release("run-ref-1");
    }

    /// A name no other attachment shares still finds it, one the task
    /// started with too, and the same attachment fetched twice is one.
    #[test]
    fn a_name_of_its_own_still_finds_it() {
        let plan = file("plan.txt", "cGxhbg==");
        let files = vec![plan.clone(), plan.clone()];
        assert_eq!(pick(&files, "plan.txt"), Found::One(plan.clone()));
        let asked = reference("plan.txt", &sha256_of(&plan));
        assert_eq!(pick(&files, &asked), Found::One(plan));
        assert_eq!(pick(&files, "other.txt"), Found::None);
        assert_eq!(pick(&files, "plan.txt#00000000"), Found::None, "a wrong reference found an attachment");
    }
}
