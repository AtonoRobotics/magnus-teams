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

/// Aurora's Explore is the mockup's bento: a wide image card, then pairs,
/// and a filter change that moves (`desktop/tests/aurora-explore.mjs`).
#[test]
fn aurora_explore_is_the_mockups_bento() {
    let out = std::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/aurora-explore.mjs"))
        .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
        .output()
        .expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora's decisions and alerts are placed on its stage, not over it: a
/// decision beside its conversation or in its box, a message as a box, an
/// answer said where it was given (`desktop/tests/aurora-alerts.mjs`).
#[test]
fn aurora_alerts_are_part_of_the_stage() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-alerts.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora says what Magnus is doing where the person looks: the reasoning
/// pill on an answer being written, and the state above the composer
/// (`desktop/tests/aurora-reasoning.mjs`).
#[test]
fn aurora_says_what_magnus_is_doing_as_it_does_it() {
    let out = std::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/aurora-reasoning.mjs"))
        .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
        .output()
        .expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora's sizes move: the bento grows and folds over time, the
/// conversation above it gives up and takes back its room with it, and a
/// conversation read to its end stays there (`desktop/tests/aurora-sizes.mjs`).
#[test]
fn aurora_sizes_move() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-sizes.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora's composition: one frame for the conversation and the canvas, the
/// boxes just above the input, and what comes before the page is drawn
/// placed once it is (`desktop/tests/aurora-composition.mjs`).
#[test]
fn aurora_composition_is_one_frame_with_boxes_above_the_input() {
    let out = std::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/aurora-composition.mjs"))
        .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
        .output()
        .expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Magnus is an avatar, not an image: replaceable, following the state in
/// colour, shape and words, and still with reduced motion
/// (`desktop/tests/aurora-avatar.mjs`).
#[test]
fn aurora_magnus_is_an_avatar_that_follows_the_state() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-avatar.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// A decision on Aurora's canvas is built around the decision: its question
/// the title, its answers at the foot, the rest behind one disclosure
/// (`desktop/tests/aurora-canvas.mjs`).
#[test]
fn aurora_canvas_is_built_around_the_decision() {
    let out =
        std::process::Command::new("node").arg(workspace_root().join("desktop/tests/aurora-canvas.mjs")).env("MAGNUS_CHROMIUM", magnus_scratch::chromium()).output().expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}

/// Aurora, compact: thin scrollbars in the theme's colours, the model and
/// reasoning as one chip, and the mic through the server's voice input
/// (`desktop/tests/aurora-compact.mjs`).
#[test]
fn aurora_is_compact_and_keeps_voice_in_reach() {
    let out = std::process::Command::new("node")
        .arg(workspace_root().join("desktop/tests/aurora-compact.mjs"))
        .env("MAGNUS_CHROMIUM", magnus_scratch::chromium())
        .output()
        .expect("node");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{said}");
    println!("{said}");
}
