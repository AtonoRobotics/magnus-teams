//! Every look is readable (U7, amended): `desktop/tests/theme-check.mjs`
//! checks each look's tokens (light and dark, solid and glass, each accent)
//! for 4.5:1 text, glass over the worst-case backdrop, and reading surfaces
//! solid; and that the system's reduced transparency and reduced motion win
//! over the person's choice.

use magnus_invariants::appliance::workspace_root;

#[test]
fn every_look_reaches_four_and_a_half_to_one_over_the_worst_backdrop() {
    let out = std::process::Command::new("node").arg(workspace_root().join("desktop/tests/theme-check.mjs")).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}
