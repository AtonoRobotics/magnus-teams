//! The server address a person types, as the app keeps it.

/// An appliance address: scheme and host (and port), nothing else. Plain
/// http sends the password and the session in the clear, so it is taken
/// only for a server on this computer.
pub(crate) fn normalize(raw: &str) -> crate::Res<String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let u = url::Url::parse(&with_scheme).map_err(|_| format!("{raw} is not an address"))?;
    if !matches!(u.scheme(), "https" | "http") || u.host_str().is_none() {
        return Err(format!("{raw} is not an http or https address"));
    }
    let loopback = match u.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if u.scheme() == "http" && !loopback {
        return Err(format!("{raw} is plain http: use the server's https address"));
    }
    Ok(u.origin().ascii_serialization())
}
