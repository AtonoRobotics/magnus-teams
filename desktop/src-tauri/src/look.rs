//! Solid window background matching the Antigravity dark/light theme.

use serde::Serialize;
use tauri::window::Color;

/// Antigravity background colors (`theme.js` `bg`), light and dark.
const SOLID_LIGHT: Color = Color(0xF6, 0xF7, 0xF9, 0xFF);
const SOLID_DARK: Color = Color(0x09, 0x0A, 0x0D, 0xFF);

/// What the webview draws under the page: solid in the page's mode.
pub fn webview_background(dark: bool) -> Color {
    if dark { SOLID_DARK } else { SOLID_LIGHT }
}

/// Start the main window solid, in the system's mode, before the page runs.
pub fn start_solid(app: &tauri::App) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let dark = matches!(window.theme(), Ok(tauri::Theme::Dark));
        let _ = window.set_background_color(Some(webview_background(dark)));
    }
}

/// Update the webview background when the person switches between dark and light mode.
#[tauri::command]
pub fn set_background(window: tauri::WebviewWindow, dark: bool) {
    let _ = window.set_background_color(Some(webview_background(dark)));
}

#[derive(Serialize)]
pub struct System {
    native_speech: bool,
}

/// Host capabilities the page cannot read itself.
#[tauri::command]
pub fn system_look() -> System {
    System { native_speech: crate::speech::available() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_webview_is_solid_in_both_modes() {
        assert_eq!(webview_background(false), SOLID_LIGHT);
        assert_eq!(webview_background(true), SOLID_DARK);
        assert_eq!(SOLID_LIGHT.3, 0xFF);
        assert_eq!(SOLID_DARK.3, 0xFF);
    }
}
