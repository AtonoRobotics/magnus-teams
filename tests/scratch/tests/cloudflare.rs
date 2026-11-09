//! Cloudflare's connector, against Cloudflare: no fakes (owner,
//! 2026-11-01). The catalog's Cloudflare is in every Library already set
//! up, and Magnus registers itself at Cloudflare's server
//! (`mcp.cloudflare.com`) when a person connects: nothing for anyone to set
//! up.

#[tokio::test(flavor = "multi_thread")]
async fn cloudflare_is_in_every_library_and_magnus_registers_itself_at_its_sign_in() {
    let u = magnus_scratch::catalog::registers_itself_and_signs_in_at("cloudflare", "mcp.cloudflare.com").await;
    assert_eq!(u.path(), "/authorize", "{u}");
}
