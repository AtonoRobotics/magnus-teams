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

/// Aurora's motion plays once for what arrives, never for a repaint, in a
/// real browser (`desktop/tests/aurora-motion.mjs`).
#[test]
fn aurora_motion_plays_for_what_arrives_not_for_a_repaint() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-motion.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora draws the column beside the conversation as the mockup's stage:
/// cards that move, and an artifact canvas they fold beside
/// (`desktop/tests/aurora-stage.mjs`).
#[test]
fn aurora_draws_the_panel_as_a_stage_of_cards() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-stage.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}
