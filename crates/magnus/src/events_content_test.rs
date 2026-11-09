/// Every kind and field the content list names is one `Ev` has, so a
/// misspelling cannot leave content in the log; and each has a default,
/// so an event whose content was deleted still reads.
#[test]
fn the_content_list_names_real_kinds_and_fields() {
    let src = include_str!("events.rs");
    let src = &src[src.find("pub enum Ev {").unwrap()..src.find("pub const CONTENT").unwrap()];
    for (kind, fields) in super::CONTENT {
        let variant: String = kind.split('_').map(|w| w[..1].to_uppercase() + &w[1..]).collect();
        let start = src.find(&format!("\n    {variant} {{")).unwrap_or_else(|| panic!("no event kind {kind}"));
        let body = &src[start..start + src[start..].find("\n    },").unwrap()];
        for f in *fields {
            let top = f.split('.').next().unwrap();
            assert!(body.contains(&format!(" {top}: ")), "{kind} has no field {top}");
            if !f.contains('.') {
                assert!(body.contains(&format!("#[serde(default)]\n        {top}: ")), "{kind}.{top} has no default");
            }
        }
    }
}
