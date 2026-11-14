//! The methods the page may ask the app to send to the server for it. The
//! page's own client (desktop/ui/server.js) sends each of these; a method
//! missing here fails only in the installed app, which is how PATCH did
//! (desktop/tests/methods-check.mjs keeps the two lists together).

/// Every method the app sends for the page, as the page names it.
pub(crate) const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/// The method named `name`, or the message the page shows for another.
pub(crate) fn method(name: &str) -> Result<reqwest::Method, String> {
    match name {
        "GET" => Ok(reqwest::Method::GET),
        "POST" => Ok(reqwest::Method::POST),
        "PUT" => Ok(reqwest::Method::PUT),
        "PATCH" => Ok(reqwest::Method::PATCH),
        "DELETE" => Ok(reqwest::Method::DELETE),
        other => Err(format!("{other} is not a method this app uses")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_method_is_sent_as_itself() {
        for name in METHODS {
            assert_eq!(method(name).map(|m| m.as_str().to_string()), Ok(name.to_string()), "{name}");
        }
    }

    #[test]
    fn patch_is_sent() {
        // Admin changes a role, a connector and a provider with PATCH.
        assert_eq!(method("PATCH"), Ok(reqwest::Method::PATCH));
    }

    #[test]
    fn another_method_is_refused() {
        assert_eq!(method("TRACE"), Err("TRACE is not a method this app uses".to_string()));
        assert!(method("patch").is_err());
    }
}
