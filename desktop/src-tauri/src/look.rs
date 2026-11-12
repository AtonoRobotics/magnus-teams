//! The window's material (U7, amended): glass uses the system's own window
//! material where it has one (Mica on Windows 11, Acrylic on Windows 10),
//! and the page draws its backdrop where it has none (Linux). Reading
//! surfaces are the page's and stay solid either way.

use serde::Serialize;
use tauri::window::Color;

/// The page's solid background (theme.js `bg`), light and dark. On Windows
/// the window is created able to show the system's glass, and WebView2 draws
/// such a window black wherever the page is not painted: before the page's
/// scripts set its colors, or on a computer that cannot composite a
/// transparent webview at all (remote desktop, some virtual machines and
/// graphics drivers). So the webview starts solid and turns clear only once
/// the system's material is behind it.
const SOLID_LIGHT: Color = Color(0xF3, 0xF3, 0xF0, 0xFF);
const SOLID_DARK: Color = Color(0x16, 0x16, 0x18, 0xFF);
const CLEAR: Color = Color(0, 0, 0, 0);

/// What the webview draws under the page: clear only over the system's own
/// material, solid in the page's mode otherwise.
pub fn webview_background(native: bool, dark: bool) -> Color {
    match (native, dark) {
        (true, _) => CLEAR,
        (false, true) => SOLID_DARK,
        (false, false) => SOLID_LIGHT,
    }
}

/// Start the main window solid, in the system's mode, before the page runs.
pub fn start_solid(app: &tauri::App) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let dark = matches!(window.theme(), Ok(tauri::Theme::Dark));
        let _ = window.set_background_color(Some(webview_background(false, dark)));
    }
}

#[derive(Serialize)]
pub struct Material {
    /// Whether the system draws the glass behind the page; when not, the
    /// page draws its own backdrop.
    native: bool,
}

/// Put the window on glass or take it off. `dark` picks the material's
/// dark variant where the system has one; `tint` (r, g, b, alpha 0-255) is
/// the page's tint for a material Magnus colors itself (Acrylic).
#[tauri::command]
pub fn set_material(window: tauri::WebviewWindow, glass: bool, dark: bool, tint: (u8, u8, u8, u8)) -> Material {
    let native = apply(&window, glass, dark, tint);
    let _ = window.set_background_color(Some(webview_background(native, dark)));
    Material { native }
}

#[cfg(target_os = "windows")]
fn apply(window: &tauri::WebviewWindow, glass: bool, dark: bool, tint: (u8, u8, u8, u8)) -> bool {
    use window_vibrancy::{apply_acrylic, apply_mica, clear_acrylic, clear_mica};
    let _ = clear_mica(window);
    let _ = clear_acrylic(window);
    if !glass {
        return false;
    }
    // Mica on Windows 11; Acrylic, in the page's tint, on Windows 10.
    apply_mica(window, Some(dark)).is_ok() || apply_acrylic(window, Some(tint)).is_ok()
}

#[cfg(not(target_os = "windows"))]
fn apply(_window: &tauri::WebviewWindow, _glass: bool, _dark: bool, _tint: (u8, u8, u8, u8)) -> bool {
    false
}

#[derive(Serialize)]
pub struct System {
    /// The person turned transparency effects off in the system's settings.
    reduce_transparency: bool,
    native_speech: bool,
}

/// The system's settings the page cannot read itself. Reduced motion
/// reaches the page as a media query on every system; reduced transparency
/// does not reach WebView2, so it is read here (Windows: Personalize,
/// EnableTransparency = 0).
#[tauri::command]
pub fn system_look() -> System {
    System { reduce_transparency: reduce_transparency(), native_speech: crate::speech::available() }
}

#[cfg(target_os = "windows")]
fn reduce_transparency() -> bool {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "/v", "EnableTransparency"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().last() == Some("0x0"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn reduce_transparency() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The webview is clear only over the system's material; without it, it
    /// is solid, so the window is never black where the page is not painted.
    #[test]
    fn the_webview_is_clear_only_over_the_system_material() {
        assert_eq!(webview_background(true, false).3, 0);
        assert_eq!(webview_background(true, true).3, 0);
        assert_eq!(webview_background(false, false), SOLID_LIGHT);
        assert_eq!(webview_background(false, true), SOLID_DARK);
        assert_eq!(SOLID_LIGHT.3, 0xFF);
        assert_eq!(SOLID_DARK.3, 0xFF);
    }
}
