//! The people's web app: the desktop app's own frontend, served as is.
use crate::api::err;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::Response;

const UI: &[(&str, &str, &[u8])] = &[
    ("index.html", "text/html; charset=utf-8", include_bytes!("../../../../desktop/ui/index.html")),
    ("app.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/app.css")),
    ("bridge.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/bridge.js")),
    ("theme.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/theme.js")),
    ("uispec.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/uispec.js")),
    ("compose.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/compose.js")),
    ("app.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app.js")),
    ("password.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/password.js")),
    ("dev.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/dev.js")),
    ("rfb.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rfb.js")),
    ("computers.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/computers.js")),
    ("programs.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/programs.js")),
    ("screen.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/screen.js")),
    ("payments.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/payments.js")),
    ("rules.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rules.js")),
    ("teams.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/teams.js")),
    ("message-controls.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/message-controls.js")),
    ("chat-view.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/chat-view.js")),
    ("chat-composer.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/chat-composer.js")),
    ("personal-chat.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/personal-chat.js")),
    ("team-conversation.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-conversation.js")),
    ("team-chat.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-chat.js")),
    ("team-panel.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-panel.js")),
    ("team-command.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-command.js")),
    ("team-people.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-people.js")),
    ("notices.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/notices.js")),
    ("teams.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/teams.css")),
    ("computer.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/computer.css")),
    ("subagents.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/subagents.js")),
    ("connect.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/connect.js")),
    ("asks.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/asks.js")),
    ("evidence.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/evidence.js")),
    ("admin.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin.js")),
    ("admin-spend.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-spend.js")),
    ("admin-agents.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-agents.js")),
    ("admin.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/admin.css")),
    ("admin-overview.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-overview.js")),
    ("admin-people.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-people.js")),
    ("admin-testers.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-testers.js")),
    ("agreements.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/agreements.js")),
    ("admin-workspaces.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-workspaces.js")),
    ("admin-connectors.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-connectors.js")),
    ("admin-rules.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-rules.js")),
    ("admin-security.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-security.js")),
    ("admin-ops.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/admin-ops.js")),
];
pub(crate) const FONTS: &[(&str, &[u8])] = &[
    ("inter-latin-wght-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/inter-latin-wght-normal.woff2")),
    ("inter-latin-ext-wght-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/inter-latin-ext-wght-normal.woff2")),
    ("ibm-plex-mono-latin-400-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/ibm-plex-mono-latin-400-normal.woff2")),
    ("ibm-plex-mono-latin-500-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/ibm-plex-mono-latin-500-normal.woff2")),
];
const UI_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src blob:; font-src 'self'; connect-src 'self'; frame-src 'self' about:; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

fn static_file(media: &str, body: &'static [u8]) -> Response {
    let built = Response::builder()
        .header("content-type", media)
        .header("content-security-policy", UI_CSP)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from(body));
    built.unwrap_or_else(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "the file could not be served"))
}

pub async fn ui_file(Path(file): Path<String>) -> Response {
    match UI.iter().find(|(n, _, _)| *n == file) {
        Some((_, media, body)) => static_file(media, body),
        None => err(StatusCode::NOT_FOUND, "not found"),
    }
}

pub async fn ui_font(Path(file): Path<String>) -> Response {
    match FONTS.iter().find(|(n, _)| *n == file) {
        Some((_, body)) => static_file("font/woff2", body),
        None => err(StatusCode::NOT_FOUND, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_the_page_loads_is_served() {
        let page = std::str::from_utf8(UI[0].2).unwrap();
        let loaded: Vec<&str> = page.split("src=\"").skip(1).filter_map(|p| p.split('"').next()).filter(|p| p.ends_with(".js")).collect();
        assert!(loaded.len() >= 6, "{loaded:?}");
        for script in loaded {
            assert!(UI.iter().any(|(n, _, _)| *n == script), "{script} is not served");
        }
    }
}
