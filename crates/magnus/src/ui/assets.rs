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
    ("dev.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/dev.js")),
    ("rfb.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/rfb.js")),
    ("computers.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/computers.js")),
    ("payments.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/payments.js")),
    ("teams.js", "text/javascript; charset=utf-8", include_bytes!("../../../../desktop/ui/teams.js")),
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
