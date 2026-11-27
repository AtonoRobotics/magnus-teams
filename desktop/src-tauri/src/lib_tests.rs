/// A cancelled provider sign-in stops waiting at once, so the browser
/// finishing it later brings back nothing that is kept.
#[tokio::test]
async fn a_cancelled_browser_sign_in_stops_waiting() {
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    let forever = async {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        Ok::<_, String>(())
    };
    drop(tx);
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), super::unless_cancelled(forever, &mut rx)).await;
    assert_eq!(r, Ok(Err(super::CANCELLED.to_string())), "the sign-in kept waiting after it was cancelled");
}

/// Only the latest sign-in, not cancelled, finishes: a Cancel at any
/// point, even before the appliance answered the first request, stops it,
/// and an older one never takes a newer one's place (review of #160).
#[test]
fn only_the_latest_uncancelled_sign_in_finishes() {
    let mut s = super::BrowserSignIn::default();
    let (first, mut rx1) = s.begin();
    s.cancel();
    assert!(rx1.try_recv().is_err_and(|e| e == tokio::sync::oneshot::error::TryRecvError::Closed), "a Cancel before the first answer did not reach the sign-in");
    assert!(!s.finish(first), "a cancelled sign-in finished");

    let (older, mut rx2) = s.begin();
    let (newer, mut rx3) = s.begin();
    assert!(rx2.try_recv().is_err_and(|e| e == tokio::sync::oneshot::error::TryRecvError::Closed), "a newer sign-in did not end the older one");
    assert!(!s.finish(older), "an older sign-in finished in a newer one's place");
    assert_eq!(rx3.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty), "the older one's finish ended the newer one");
    assert!(s.finish(newer));
    s.cancel();
    assert!(!s.finish(newer), "a sign-in finished twice");
}

use super::*;

#[test]
fn the_page_cannot_reach_beyond_the_api_or_take_a_session() {
    for bad in [
        "/v1/sign-in",
        "/v1/sign-in/device",
        "/v1/sign-in/device/signin_1",
        "/v1/sign-out",
        "/v1/devices",
        "/v1/devices/dev_1",
        "/v1/../etc",
        "https://evil.example/v1/x",
        "/other",
        "/v1/x#y",
    ] {
        assert!(allowed(bad).is_err(), "{bad} was allowed");
    }
    // The request URL is normalised before it is sent: a dot segment or
    // an encoded dot or slash must not hide a sign-in call.
    for bad in ["/v1/./sign-in/device", "/v1/%2e/sign-in", "/v1/%2E/sign-out", "/v1/x/%2e%2e/devices", "/v1/x/.%2E/sign-in", "/v1/%2fsign-in", "/v1\\sign-in", "/v1/x?y#z"] {
        assert!(allowed(bad).is_err(), "{bad} was allowed");
    }
    assert!(allowed("/v1/inbox").is_ok());
    assert!(allowed("/v1/tasks?status=open").is_ok());
}

#[test]
fn a_registered_sign_in_address_is_listened_on_exactly() {
    assert_eq!(sign_in_listen(None).unwrap(), "127.0.0.1:0");
    assert_eq!(sign_in_listen(Some("")).unwrap(), "127.0.0.1:0");
    assert_eq!(sign_in_listen(Some("http://127.0.0.1:17321/callback")).unwrap(), "127.0.0.1:17321");
    assert!(sign_in_listen(Some("http://localhost:17321/callback")).is_err());
    assert!(sign_in_listen(Some("https://magnus.pyrallon.ai/oauth/slack/callback")).is_err());
}

#[test]
fn an_address_is_reduced_to_its_origin() {
    assert_eq!(normalize("magnus.local:7443").unwrap(), "https://magnus.local:7443");
    assert_eq!(normalize("https://10.0.0.5:7443/some/path").unwrap(), "https://10.0.0.5:7443");
    assert_eq!(normalize("http://127.0.0.1:7700/").unwrap(), "http://127.0.0.1:7700");
    assert!(normalize("http://evil.example").is_err(), "plain http to another host would send the password in the clear");
    assert!(normalize("http://192.168.0.5:7700").is_err());
    assert_eq!(normalize("http://localhost:7700").unwrap(), "http://localhost:7700");
    assert!(normalize("file:///etc/passwd").is_err());
}

#[test]
fn the_public_name_is_not_pinned_and_the_device_key_stays_here() {
    assert!(pin::is_public_appliance("https://magnus.pyrallon.ai"));
    assert!(pin::is_public_appliance("https://Magnus.Pyrallon.ai/v1/me"));
    assert!(!pin::is_public_appliance("https://10.1.1.5:7700"));
    assert!(!pin::is_public_appliance("https://magnus.local:7700"));
    assert!(!pin::is_public_appliance("https://strix.tailnet.ts.net"));
    assert!(!pin::is_public_appliance("http://magnus.pyrallon.ai"));
    let (key, csr) = pin::begin_device().unwrap();
    assert!(key.contains("PRIVATE KEY"));
    assert!(csr.contains("BEGIN CERTIFICATE REQUEST"));
    assert!(!csr.contains("PRIVATE KEY"), "the request sent to the server carried the key");
}
