//! Supabase's connector, against Supabase: no fakes (owner, 2026-11-01).
//! The catalog's Supabase is in every Library already set up, read only,
//! and Magnus registers itself at Supabase's sign-in when a person
//! connects: nothing for anyone to set up.

#[tokio::test(flavor = "multi_thread")]
async fn supabase_is_in_every_library_read_only_and_magnus_registers_itself_at_its_sign_in() {
    let u = magnus_scratch::catalog::registers_itself_and_signs_in_at("supabase", "api.supabase.com").await;
    assert_eq!(u.path(), "/v1/oauth/authorize", "{u}");
    // The sign-in is for the read-only server: every query as Supabase's
    // read-only database user, none of its tools that change anything.
    let resource = u.query_pairs().find(|(k, _)| k == "resource").map(|(_, v)| v.into_owned()).unwrap();
    assert!(url::Url::parse(&resource).unwrap().query_pairs().any(|(k, v)| k == "read_only" && v == "true"), "{u}");
}
