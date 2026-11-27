//! The people's web app: the desktop app's own frontend, served as is.
use crate::api::err;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::io::Write;
use std::sync::LazyLock;

const UI: &[(&str, &str, &[u8])] = &[
    ("index.html", "text/html; charset=utf-8", include_bytes!("../../../../desktop/ui/index.html")),
    ("manifest.webmanifest", "application/manifest+json; charset=utf-8", include_bytes!("../../../../desktop/ui/manifest.webmanifest")),
    ("mobile-approve.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/mobile-approve.js")),
    ("base.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/base.css")),
    ("layout.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/layout.css")),
    ("chat.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/chat.css")),
    ("pages.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/pages.css")),
    ("bridge.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/bridge.js")),
    ("server.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/server.js")),
    ("theme.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/theme.js")),
    ("uispec.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/uispec.js")),
    ("html-frame.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/html-frame.js")),
    ("rich-text.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rich-text.js")),
    ("compose.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/compose.js")),
    ("app-logos.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-logos.js")),
    ("side-panel.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/side-panel.js")),
    ("aurora-core.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-core.js")),
    ("aurora-boxes.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-boxes.js")),
    ("aurora-stage-view.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-stage-view.js")),
    ("aurora-stage.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-stage.js")),
    ("ui-store.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/ui-store.js")),
    ("aurora-reasoning.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-reasoning.js")),
    ("aurora-alerts.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-alerts.js")),
    ("aurora-explore.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-explore.js")),
    ("aurora-rail.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-rail.js")),
    ("aurora-compact.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-compact.js")),
    ("aurora-pages.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-pages.js")),
    ("aurora-avatar.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-avatar.js")),
    ("aurora-orb.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-orb.js")),
    ("connect-web.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/connect-web.js")),
    ("web-update.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/web-update.js")),
    ("app.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app.js")),
    ("app-shell.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-shell.js")),
    ("app-files.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-files.js")),
    ("app-polling.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-polling.js")),
    ("app-chat.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-chat.js")),
    ("app-inbox.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-inbox.js")),
    ("app-panel.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-panel.js")),
    ("app-library.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-library.js")),
    ("app-settings.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-settings.js")),
    ("app-boot.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/app-boot.js")),
    ("composed-frames.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/composed-frames.js")),
    ("signin.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/signin.js")),
    ("tasks-schedules.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/tasks-schedules.js")),
    ("settings-sections.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/settings-sections.js")),
    ("projects.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/projects.js")),
    ("password.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/password.js")),
    ("dev.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/dev.js")),
    ("rfb.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rfb.js")),
    ("computers.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/computers.js")),
    ("programs.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/programs.js")),
    ("outside-agents.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/outside-agents.js")),
    ("screen.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/screen.js")),
    ("payments.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/payments.js")),
    ("rules.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rules.js")),
    ("user-avatar.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/user-avatar.js")),
    ("coedit.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/coedit.js")),
    ("team-rooms.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/team-rooms.js")),
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
    ("shared-canvas.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/shared-canvas.js")),
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
    ("onboarding.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/onboarding.js")),
    ("onboarding.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/onboarding.css")),
    ("aurora.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora.css")),
    ("aurora-widgets.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-widgets.css")),
    ("aurora-stage.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-stage.css")),
    ("aurora-rail.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-rail.css")),
    ("aurora-compact.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora-compact.css")),
    ("nova.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/nova.css")),
    ("nova-stage.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-stage.css")),
    ("nova-shell.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-shell.css")),
    ("nova-widgets.css", "text/css; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-widgets.css")),
    ("aurora.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/aurora.js")),
    ("nova-avatars.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-avatars.js")),
    ("task-replay.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/task-replay.js")),
    ("plan-surface.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/plan-surface.js")),
    ("nova-ux.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-ux.js")),
    ("nova-frame.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-frame.js")),
    ("release-notes.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/release-notes.js")),
    ("nova-pages.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/nova-pages.js")),
    ("explore-ideas.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/explore-ideas.js")),
    ("discover.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/discover.js")),
    ("hero-antigravity.webp", "image/webp", include_bytes!("../../../../desktop/ui/hero-antigravity.webp")),
    ("hero-gemini.webp", "image/webp", include_bytes!("../../../../desktop/ui/hero-gemini.webp")),
    ("card-inbox.webp", "image/webp", include_bytes!("../../../../desktop/ui/card-inbox.webp")),
    ("card-research.webp", "image/webp", include_bytes!("../../../../desktop/ui/card-research.webp")),
    ("card-monitor.webp", "image/webp", include_bytes!("../../../../desktop/ui/card-monitor.webp")),
    ("card-eng.webp", "image/webp", include_bytes!("../../../../desktop/ui/card-eng.webp")),
];
pub(crate) const FONTS: &[(&str, &[u8])] = &[
    ("inter-latin-wght-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/inter-latin-wght-normal.woff2")),
    ("inter-latin-ext-wght-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/inter-latin-ext-wght-normal.woff2")),
    ("ibm-plex-mono-latin-400-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/ibm-plex-mono-latin-400-normal.woff2")),
    ("ibm-plex-mono-latin-500-normal.woff2", include_bytes!("../../../../desktop/ui/fonts/ibm-plex-mono-latin-500-normal.woff2")),
];
const UI_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src blob:; font-src 'self'; connect-src 'self'; frame-src 'self' about:; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

const NO_STORE_CACHE: &str = "private, no-store, no-cache, must-revalidate, max-age=0";
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

fn gzip_bytes(raw: &[u8]) -> Option<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(raw).ok()?;
    enc.finish().ok()
}

static UI_GZIP: LazyLock<HashMap<&'static str, Vec<u8>>> = LazyLock::new(|| {
    UI.iter().filter(|(name, _, _)| name.ends_with(".js") || name.ends_with(".css")).filter_map(|(name, _, body)| gzip_bytes(body).map(|gz| (*name, gz))).collect()
});

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|part| part.split(';').next().is_some_and(|enc| enc.trim().eq_ignore_ascii_case("gzip"))))
}

fn static_file(media: &str, cache_control: &str, gzipped: bool, vary_encoding: bool, body: impl Into<axum::body::Body>) -> Response {
    let mut builder = Response::builder()
        .header("content-type", media)
        .header("content-security-policy", UI_CSP)
        .header("x-frame-options", "DENY")
        .header("x-content-type-options", "nosniff")
        .header("cache-control", cache_control);
    if vary_encoding {
        builder = builder.header("vary", "accept-encoding");
    }
    if gzipped {
        builder = builder.header("content-encoding", "gzip");
    }
    builder.body(body.into()).unwrap_or_else(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "the file could not be served"))
}

pub async fn ui_index(headers: HeaderMap) -> Response {
    ui_file(headers, Path("index.html".into())).await
}

pub async fn ui_file(headers: HeaderMap, Path(file): Path<String>) -> Response {
    match UI.iter().find(|(n, _, _)| *n == file) {
        Some(("index.html", media, body)) => {
            let v = crate::update::version();
            let html = String::from_utf8_lossy(body).replace(".css\"", &format!(".css?v={v}\"")).replace(".js\"", &format!(".js?v={v}\""));
            if accepts_gzip(&headers) {
                if let Some(gz) = gzip_bytes(html.as_bytes()) {
                    return static_file(media, NO_STORE_CACHE, true, true, gz);
                }
            }
            static_file(media, NO_STORE_CACHE, false, true, html)
        }
        Some((name, media, body)) => {
            let cache = if *name == "manifest.webmanifest" { NO_STORE_CACHE } else { IMMUTABLE_CACHE };
            let compressible = name.ends_with(".js") || name.ends_with(".css");
            if compressible && accepts_gzip(&headers) {
                if let Some(gz) = UI_GZIP.get(name) {
                    return static_file(media, cache, true, true, gz.clone());
                }
            }
            static_file(media, cache, false, compressible, *body)
        }
        None => err(StatusCode::NOT_FOUND, "not found"),
    }
}

pub async fn ui_font(Path(file): Path<String>) -> Response {
    match FONTS.iter().find(|(n, _)| *n == file) {
        Some((_, body)) => static_file("font/woff2", IMMUTABLE_CACHE, false, false, *body),
        None => err(StatusCode::NOT_FOUND, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use flate2::read::GzDecoder;
    use std::io::Read;

    #[test]
    fn everything_the_page_loads_is_served() {
        let page = std::str::from_utf8(UI[0].2).unwrap();
        let loaded: Vec<&str> = page.split("src=\"").skip(1).filter_map(|p| p.split('"').next()).filter(|p| p.ends_with(".js")).collect();
        assert!(loaded.len() >= 6, "{loaded:?}");
        for script in loaded {
            assert!(UI.iter().any(|(n, _, _)| *n == script), "{script} is not served");
        }
    }

    #[tokio::test]
    async fn cache_control_and_gzip_headers_on_ui_assets() {
        let mut gz_headers = HeaderMap::new();
        gz_headers.insert(axum::http::header::ACCEPT_ENCODING, "gzip, deflate, br".parse().unwrap());

        let idx = ui_file(HeaderMap::new(), Path("index.html".into())).await;
        assert_eq!(idx.headers().get("cache-control").unwrap(), NO_STORE_CACHE);
        assert!(idx.headers().get("content-encoding").is_none());

        let js_raw = ui_file(HeaderMap::new(), Path("app.js".into())).await;
        assert_eq!(js_raw.headers().get("cache-control").unwrap(), IMMUTABLE_CACHE);
        assert_eq!(js_raw.headers().get("vary").unwrap(), "accept-encoding");
        assert!(js_raw.headers().get("content-encoding").is_none());
        let raw_bytes = to_bytes(js_raw.into_body(), usize::MAX).await.unwrap();

        let js_gz = ui_file(gz_headers, Path("app.js".into())).await;
        assert_eq!(js_gz.headers().get("cache-control").unwrap(), IMMUTABLE_CACHE);
        assert_eq!(js_gz.headers().get("content-encoding").unwrap(), "gzip");
        assert_eq!(js_gz.headers().get("vary").unwrap(), "accept-encoding");
        let gz_bytes = to_bytes(js_gz.into_body(), usize::MAX).await.unwrap();
        assert!(gz_bytes.len() < raw_bytes.len(), "gzip should shrink app.js");
        let mut dec = GzDecoder::new(&gz_bytes[..]);
        let mut roundtrip = Vec::new();
        dec.read_to_end(&mut roundtrip).unwrap();
        assert_eq!(roundtrip, raw_bytes.as_ref());

        let font = ui_font(Path("inter-latin-wght-normal.woff2".into())).await;
        assert_eq!(font.headers().get("cache-control").unwrap(), IMMUTABLE_CACHE);
    }
}
