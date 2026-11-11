//! The computer's desktop looks like its mockup (spec rev 145, The desktop;
//! "Mockup. The Desktop and Wallpaper artboards"). A computer from the base
//! image is screenshotted through the normal screenshot path, in dark and
//! then light, and compared with the artboards rendered by
//! `tests/scratch/reference/render-desktop.mjs`: the top bar's place and
//! height, the wallpaper's colors (a perceptual difference under a
//! threshold, not pixel equality, as fonts render differently), and the
//! Magnus menu, the three pinned apps and the clock present. Then the
//! calm rule: one window maximized, two snapped to halves. Needs a real
//! `magnus-computerd` (KVM, the guest images, passwordless sudo), as on
//! Strix Beta. `MAGNUS_DESKTOP_SHOTS=DIR` keeps the screenshots there.

use base64::Engine;
use magnus_invariants::*;
use magnus_scratch::*;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const WIDTH: usize = 1280;
const HEIGHT: usize = 800;
/// The Desktop artboard's top bar, in pixels.
const BAR_PX: usize = 34;
const BAR_SLACK_PX: usize = 2;
/// Channel steps: the bar's color against the app's token and the
/// artboard's, and what counts as something drawn on the bar.
const SAME_COLOR: u8 = 4;
const NEAR_ARTBOARD: u8 = 16;
const DRAWN: u8 = 40;
/// The wallpaper, below the bar and clear of the mark: the mean channel
/// difference from the artboard, and the share of pixels far from it.
const MOST_MEAN_DIFFERENCE: f64 = 4.0;
const MOST_FAR_SHARE: f64 = 0.02;
/// The mark, whose text the two renderers draw differently.
const MOST_MARK_DIFFERENCE: f64 = 14.0;
/// Columns of nothing between two pinned apps' icons.
const GAP_PX: usize = 6;
const SETTLE: Duration = Duration::from_secs(2);

struct Shot {
    px: Vec<[u8; 3]>,
}

impl Shot {
    fn from_png(bytes: &[u8]) -> Shot {
        let mut reader = png::Decoder::new(bytes).read_info().expect("a PNG");
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("the PNG's pixels");
        assert_eq!((info.width as usize, info.height as usize), (WIDTH, HEIGHT), "the screen is not 1280x800");
        let step = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => panic!("a {other:?} PNG"),
        };
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        Shot { px: buf[..info.buffer_size()].chunks(step).map(|c| [c[0], c[1], c[2]]).collect() }
    }

    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        self.px[y * WIDTH + x]
    }

    /// Rows from the top in the bar's color, down a column with nothing on it.
    fn bar_height(&self, x: usize) -> usize {
        let bar = self.at(x, 1);
        (0..HEIGHT).take_while(|&y| near(self.at(x, y), bar, SAME_COLOR)).count()
    }

    /// The column ranges in `xs` where something is drawn on the bar.
    fn drawn_on_bar(&self, xs: std::ops::Range<usize>) -> Vec<(usize, usize)> {
        let bar = self.at(xs.start, 1);
        let mut groups: Vec<(usize, usize)> = vec![];
        for x in xs {
            if (4..BAR_PX - 4).any(|y| !near(self.at(x, y), bar, DRAWN)) {
                match groups.last_mut() {
                    Some((_, end)) if x - *end <= GAP_PX => *end = x,
                    _ => groups.push((x, x)),
                }
            }
        }
        groups
    }
}

fn near(a: [u8; 3], b: [u8; 3], most: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= most)
}

fn hex(s: &str) -> [u8; 3] {
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
    [v(1), v(3), v(5)]
}

/// The mean channel difference over a region, and the share of its pixels
/// more than `DRAWN` away in some channel.
fn difference(a: &Shot, b: &Shot, xs: std::ops::Range<usize>, ys: std::ops::Range<usize>) -> (f64, f64) {
    let (mut sum, mut far, mut n) = (0u64, 0u64, 0u64);
    for y in ys {
        for x in xs.clone() {
            let (p, q) = (a.at(x, y), b.at(x, y));
            sum += (0..3).map(|i| p[i].abs_diff(q[i]) as u64).sum::<u64>();
            far += u64::from(!near(p, q, DRAWN));
            n += 1;
        }
    }
    (sum as f64 / (3 * n) as f64, far as f64 / n as f64)
}

fn reference(theme: &str) -> Shot {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("reference/desktop-{theme}.png"));
    Shot::from_png(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

async fn screen(alice: &Api, id: &str) -> Vec<u8> {
    let image: String = eventually("a screenshot of the running computer", PATIENCE, || async {
        let (st, v) = alice.get(&format!("/v1/computers/{id}/screen")).await;
        (st.is_success() && v["last"] != true).then(|| v["image_b64"].as_str().map(String::from)).flatten()
    })
    .await;
    base64::engine::general_purpose::STANDARD.decode(image).expect("base64")
}

/// The look of one theme, against its artboard and the app's colors.
fn looks_like_the_mockup(theme: &str, png: &[u8], bar_token: &str) {
    if let Ok(dir) = std::env::var("MAGNUS_DESKTOP_SHOTS") {
        let _ = std::fs::write(Path::new(&dir).join(format!("desktop-{theme}.png")), png);
    }
    let (ours, art) = (Shot::from_png(png), reference(theme));
    let clear = 700;
    let (h, art_h) = (ours.bar_height(clear), art.bar_height(clear));
    assert!(h.abs_diff(BAR_PX) <= BAR_SLACK_PX && h.abs_diff(art_h) <= BAR_SLACK_PX, "{theme}: the top bar is {h} pixels, the artboard's {art_h}");
    let bar = ours.at(clear, 1);
    assert!(near(bar, hex(bar_token), SAME_COLOR), "{theme}: the bar is {bar:?}, not the app's {bar_token}");
    assert!(near(bar, art.at(clear, 1), NEAR_ARTBOARD), "{theme}: the bar is {bar:?}, the artboard's {:?}", art.at(clear, 1));
    let (mean, far) = difference(&ours, &art, 300..WIDTH, 60..700);
    assert!(mean <= MOST_MEAN_DIFFERENCE && far <= MOST_FAR_SHARE, "{theme}: the wallpaper differs from the artboard: mean {mean:.2}, {:.2}% far", far * 100.0);
    let (mark, _) = difference(&ours, &art, 40..220, 715..765);
    assert!(mark <= MOST_MARK_DIFFERENCE, "{theme}: the wallpaper's mark differs from the artboard's: mean {mark:.2}");
    assert!(!ours.drawn_on_bar(0..76).is_empty(), "{theme}: no Magnus menu on the bar");
    let pinned = ours.drawn_on_bar(80..200);
    assert!(pinned.len() >= 3, "{theme}: the pinned apps are not on the bar: {pinned:?}");
    assert!(!ours.drawn_on_bar(1150..WIDTH).is_empty(), "{theme}: no clock on the bar");
}

/// The normal windows' geometry: (x, y, width, height), oldest first.
fn windows(computerd: &Computerd, id: &str) -> Vec<(i64, i64, i64, i64)> {
    computerd
        .exec(id, "DISPLAY=:0 wmctrl -lG")
        .lines()
        .filter(|l| !l.contains("xfce4-panel") && !l.ends_with(" Desktop"))
        .filter_map(|l| {
            let f: Vec<i64> = l.split_whitespace().skip(2).take(4).filter_map(|n| n.parse().ok()).collect();
            (f.len() == 4).then(|| (f[0], f[1], f[2], f[3]))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_desktop_looks_like_its_mockup_in_dark_and_light_and_places_windows_calmly() {
    let m = start_checked(script(|_c| async { complete("Done.") })).await;
    let computerd = Computerd::start();
    let app = Appliance::start(Spec::default().computer(&computerd.socket), &m).await;
    let alice = app.api("alice");
    let made: Value = eventually("the server made alice's computer", PATIENCE, || async {
        let (_, v) = alice.get("/v1/computers").await;
        v["computers"].as_array().into_iter().flatten().find(|c| c["owner"]["person"] == "alice").cloned()
    })
    .await;
    let id = made["id"].as_str().unwrap_or_default().to_string();
    assert_eq!(made["theme"], "dark", "a new computer's desktop is not dark");
    let (st, v) = alice.post(&format!("/v1/computers/{id}/start"), json!({})).await;
    assert!(st.is_success(), "start: {v}");

    looks_like_the_mockup("dark", &screen(&alice, &id).await, "#19191C");

    let (st, v) = alice.put(&format!("/v1/computers/{id}/theme"), json!({ "theme": "light" })).await;
    assert!(st.is_success(), "theme: {v}");
    assert_eq!(alice.get(&format!("/v1/computers/{id}")).await.1["theme"], "light");
    tokio::time::sleep(SETTLE).await;
    looks_like_the_mockup("light", &screen(&alice, &id).await, "#FAFAF8");

    // Calm: a new window opens maximized, and two share the screen.
    computerd.exec(&id, "export DISPLAY=:0 DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/magnus-desktop/bus; (xfce4-terminal --working-directory=/workspace >/dev/null 2>&1 &)");
    let one = eventually("the Terminal fills the screen", PATIENCE, || async {
        let w = windows(&computerd, &id);
        (w.len() == 1 && w[0].2 >= WIDTH as i64 - 4 && w[0].1 >= BAR_PX as i64).then_some(w)
    })
    .await;
    computerd.exec(&id, "export DISPLAY=:0 DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/magnus-desktop/bus; (thunar /workspace >/dev/null 2>&1 &)");
    let two = eventually("Terminal and Files share the screen half and half", PATIENCE, || async {
        let mut w = windows(&computerd, &id);
        w.sort();
        let half = WIDTH as i64 / 2;
        (w.len() == 2 && w[0].0 <= 2 && w[0].2 <= half && w[1].0 >= half - 2 && w[1].0 + w[1].2 >= WIDTH as i64 - 4).then_some(w)
    })
    .await;
    println!("one window {one:?}, two {two:?}");
    tokio::time::sleep(SETTLE).await;
    let both = screen(&alice, &id).await;
    if let Ok(dir) = std::env::var("MAGNUS_DESKTOP_SHOTS") {
        let _ = std::fs::write(Path::new(&dir).join("desktop-two-windows.png"), &both);
    }
    // The Terminal the agent started, on the left, takes the light look's
    // colors too, not the terminal's own black.
    let inside = Shot::from_png(&both).at(WIDTH / 4, HEIGHT / 2);
    assert!(near(inside, hex("#FFFFFF"), SAME_COLOR), "the agent's Terminal is {inside:?}, not the light look's #FFFFFF");

    // An app installed without its own menu entry gets one.
    let entry = computerd.exec(
        &id,
        "printf '#!/bin/sh\\n' > /usr/local/bin/hello-tool && chmod +x /usr/local/bin/hello-tool && magnus-desktop app-installed 'Hello Tool' >/dev/null && cat /usr/local/share/applications/magnus-app-hello-tool.desktop",
    );
    assert!(entry.contains("Name=Hello Tool") && entry.contains("Exec=/usr/local/bin/hello-tool"), "{entry}");
}
