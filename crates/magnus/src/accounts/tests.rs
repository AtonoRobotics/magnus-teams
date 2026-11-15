//! The accounts store against a real Postgres.
use super::*;
use crate::config::SignIn;

pub(super) fn cfg() -> SignIn {
    SignIn { session_days: 1, max_failures: 5, lockout_minutes: 15, min_password_length: 10, oidc: None }
}

fn server_url() -> String {
    std::env::var("MAGNUS_TEST_PG").unwrap_or_else(|_| "postgres://postgres:magnus_dev@127.0.0.1:54340/postgres".into())
}

/// A fresh database per test, dropped when it ends. Mirrors
/// `magnus_invariants::pg::TestDb`, which this crate cannot reuse
/// (that crate depends on this one).
pub(super) struct TestDb {
    name: String,
}

/// A migrated fresh database and a client on it as the server user.
pub(super) async fn db() -> (TestDb, String) {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("a_{id}");
    let (client, conn) = tokio_postgres::connect(&server_url(), tokio_postgres::NoTls).await.expect("connect to the test Postgres");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client.batch_execute(&format!("CREATE DATABASE {name}")).await.expect("create test database");
    let mut u = url::Url::parse(&server_url()).unwrap();
    u.set_path(&format!("/{name}"));
    let url = u.to_string();
    let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client.batch_execute(&migrate_sql("postgres")).await.unwrap();
    (TestDb { name }, url)
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let name = self.name.clone();
        // Drop runs inside the test's runtime, which cannot be blocked
        // on; the cleanup gets a thread and runtime of its own.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let (client, conn) = tokio_postgres::connect(&server_url(), tokio_postgres::NoTls).await.expect("connect to the test Postgres");
                tokio::spawn(async move {
                    let _ = conn.await;
                });
                client.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await.expect("drop test database");
            })
        })
        .join();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_a_device_ends_its_sessions_and_a_session_without_one_still_loads() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let (_, token) = accounts.open_session("ada", "app", None).await.unwrap();
    accounts.bind_session_device(&token, "dev_this").await.unwrap();
    let (_, other) = accounts.open_session("ada", "app", Some("dev_other")).await.unwrap();
    let ended = accounts.end_device("dev_this").await.unwrap();
    assert_eq!(ended.len(), 1);
    assert!(accounts.session(&token).await.is_none(), "the revoked device's session was still live");
    assert_eq!(accounts.session(&other).await.unwrap().device.as_deref(), Some("dev_other"));
    let (_, plain) = accounts.open_session("ada", "app", None).await.unwrap();
    assert!(accounts.session(&plain).await.unwrap().device.is_none(), "a session with no device did not load");
}

/// Enrolling a second factor asks for the code afterwards: the right
/// password alone answers `NeedsCode`, the password with the current
/// code signs in, and a wrong code is rejected and counted, like a
/// wrong password (#429).
#[tokio::test(flavor = "multi_thread")]
async fn a_second_factor_asks_for_the_code_and_counts_a_wrong_one() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, SignIn { session_days: 1, max_failures: 3, lockout_minutes: 15, min_password_length: 10, oidc: None }).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    assert!(!accounts.has_totp("ada").await);
    let uri = accounts.enroll_totp("ada").await.unwrap();
    assert!(uri.starts_with("otpauth://totp/"), "not an enrollment URI: {uri}");
    assert!(accounts.has_totp("ada").await, "enrollment did not stick");
    let at = chrono::Utc::now();
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).context("the enrollment URI carries no secret").unwrap();
    let secret = crate::totp::base32_decode(secret).unwrap();
    let code = crate::totp::generate(&secret, (at.timestamp().div_euclid(30)) as u64, 6);
    let counter = Accounts::network_counter("ada");
    assert!(
        matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", None, &counter, at).await, Ok(Err(SignInError::NeedsCode))),
        "the password alone signed in past the second factor"
    );
    assert!(accounts.locked(&counter).is_none(), "needing the code counted as a failure");
    assert!(
        matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some("000000"), &counter, at).await, Ok(Err(SignInError::Rejected))),
        "a wrong code did not reject"
    );
    assert!(matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&code), &counter, at).await, Ok(Ok(()))), "the current code did not sign in");
    for _ in 0..3 {
        let _ = accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some("000000"), &counter, at).await;
    }
    assert!(accounts.locked(&counter).is_some(), "wrong codes did not lock the counter");
}

/// A code signs in once: replaying it within the window rejects and
/// counts, while the next step's code still works; eight tasks racing
/// the same code admit exactly one (#441).
#[tokio::test(flavor = "multi_thread")]
async fn a_code_signs_in_once() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, SignIn { session_days: 1, max_failures: 100, lockout_minutes: 15, min_password_length: 10, oidc: None }).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let uri = accounts.enroll_totp("ada").await.unwrap();
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).unwrap();
    let secret = crate::totp::base32_decode(secret).unwrap();
    let counter = Accounts::network_counter("ada");
    let at = chrono::Utc::now();
    let step = at.timestamp().div_euclid(30) as u64;
    let code = crate::totp::generate(&secret, step, 6);
    assert!(matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&code), &counter, at).await, Ok(Ok(()))), "the first use of the code did not sign in");
    assert!(
        matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&code), &counter, at).await, Ok(Err(SignInError::Rejected))),
        "replaying the code signed in again"
    );
    // Eight tasks race the next step's code: every one of them verifies
    // it (it is live), but the step admits exactly one.
    let accounts = std::sync::Arc::new(accounts);
    let race_code = crate::totp::generate(&secret, step + 1, 6);
    let mut tasks = vec![];
    for _ in 0..8 {
        let (a, c, counter) = (accounts.clone(), race_code.clone(), counter.clone());
        tasks.push(tokio::spawn(async move { matches!(a.sign_in_counted_with_code("ada", "a-long-password", Some(&c), &counter).await, Ok(Ok(()))) }));
    }
    let mut admitted = 0u32;
    for t in tasks {
        admitted += t.await.unwrap() as u32;
    }
    assert_eq!(admitted, 1, "the raced code did not admit exactly one task");
    let next = crate::totp::generate(&secret, step + 2, 6);
    let later = chrono::DateTime::from_timestamp((step + 2) as i64 * 30 + 5, 0).unwrap();
    assert!(matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&next), &counter, later).await, Ok(Ok(()))), "the next step's code did not sign in");
}

/// An invitation activates on accept: invited cannot sign in, the token
/// with a password activates once, and a second use, a bad token or an
/// expired one answers nothing (#429).
#[tokio::test(flavor = "multi_thread")]
async fn invite_accept_activates_and_consumes() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let token = accounts.invite("ada", Some("ada@example.com")).await.unwrap();
    assert_eq!(accounts.status("ada").await, Status::Invited, "an invite did not mark the person invited");
    let counter = Accounts::network_counter("ada");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "whatever-password", None, &counter).await, Ok(Err(SignInError::Rejected))), "an invited person signed in");
    assert_eq!(accounts.accept(&token, "a-long-password").await.unwrap(), Some("ada".to_string()), "accepting the invite did not activate");
    assert_eq!(accounts.status("ada").await, Status::Active, "accepting left the person invited");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await, Ok(Ok(()))), "the accepted person cannot sign in");
    assert_eq!(accounts.accept(&token, "another-long-password").await.unwrap(), None, "the invite worked twice");
    assert_eq!(accounts.accept("deadbeef", "another-long-password").await.unwrap(), None, "a bad token accepted");
    let stale = accounts.invite("ada", None).await.unwrap();
    let past = chrono::Utc::now() + chrono::Duration::days(8);
    assert_eq!(accounts.accept_at(&stale, "another-long-password", past).await.unwrap(), None, "an expired invite accepted");
    let kept = accounts.invite("ada", None).await.unwrap();
    assert!(accounts.accept(&kept, "short").await.is_err(), "a short password accepted");
    assert_eq!(accounts.accept(&kept, "another-long-password").await.unwrap(), Some("ada".to_string()), "the short password burned the invite");
    // A password set directly completes the invitation too — but never a
    // disabling.
    accounts.invite("bea", None).await.unwrap();
    accounts.set_password("bea", "bea-long-password").await.unwrap();
    assert_eq!(accounts.status("bea").await, Status::Active, "a set password left the invitee invited");
    accounts.set_status("bea", Status::Disabled).await.unwrap();
    accounts.set_password("bea", "bea-newer-password").await.unwrap();
    assert_eq!(accounts.status("bea").await, Status::Disabled, "a set password re-enabled a disabled person");
}

/// Turning a person off refuses sign-in and ends their live sessions;
/// turning them back on restores sign-in (#429).
#[tokio::test(flavor = "multi_thread")]
async fn disabled_refuses_and_ends_sessions() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let (session, token) = accounts.open_session("ada", "app", None).await.unwrap();
    let counter = Accounts::network_counter("ada");
    let ended = accounts.set_status("ada", Status::Disabled).await.unwrap();
    assert_eq!(ended, vec![session.id.clone()], "disabling did not end the live session");
    assert!(accounts.session(&token).await.is_none(), "the session survived disabling");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await, Ok(Err(SignInError::Rejected))), "a disabled person signed in");
    accounts.set_status("ada", Status::Active).await.unwrap();
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await, Ok(Ok(()))), "re-enabling did not restore sign-in");
}

/// Inviting someone already active (a resend) hands them a token without
/// deactivating them (#429).
#[tokio::test(flavor = "multi_thread")]
async fn invite_keeps_an_active_person_active() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    assert_eq!(accounts.status("ada").await, Status::Active, "a person with no record is not active");
    let token = accounts.invite("ada", None).await.unwrap();
    assert_eq!(accounts.status("ada").await, Status::Active, "inviting deactivated an active person");
    assert_eq!(accounts.accept(&token, "a-new-long-password").await.unwrap(), Some("ada".to_string()), "the resend token did not accept");
    let counter = Accounts::network_counter("ada");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "a-new-long-password", None, &counter).await, Ok(Ok(()))), "the resent password did not take");
}

/// A disabling from another handle (the CLI's shape: its own `Accounts`
/// on the same database) ends this handle's live sessions for good:
/// re-enabling restores sign-in, never the ended sessions (#448, #469).
#[tokio::test(flavor = "multi_thread")]
async fn disabling_elsewhere_revokes_live_sessions() {
    let (_db, url) = db().await;
    let server = Accounts::open(&url, cfg()).await.unwrap();
    server.set_password("ada", "a-long-password").await.unwrap();
    let (_, token) = server.open_session("ada", "app", None).await.unwrap();
    assert!(server.session(&token).await.is_some(), "the session did not resolve before disabling");
    let cli = Accounts::open(&url, cfg()).await.unwrap();
    cli.set_status("ada", Status::Disabled).await.unwrap();
    assert!(server.session(&token).await.is_none(), "the session survived a disabling from another handle");
    cli.set_status("ada", Status::Active).await.unwrap();
    assert!(server.session(&token).await.is_none(), "re-enabling resurrected the ended session");
    let counter = Accounts::network_counter("ada");
    assert!(matches!(server.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await, Ok(Ok(()))), "re-enabling did not restore sign-in");
}

/// Removing a person from another handle ends this handle's sessions
/// for good: re-adding the id never resurrects the stale tokens, and
/// the store holds no session for them (#469).
#[tokio::test(flavor = "multi_thread")]
async fn removal_elsewhere_ends_sessions_and_readding_resurrects_nothing() {
    let (_db, url) = db().await;
    let server = Accounts::open(&url, cfg()).await.unwrap();
    server.add_person("ada", "Ada", None).await.unwrap();
    server.set_password("ada", "a-long-password").await.unwrap();
    let (_, token) = server.open_session("ada", "app", None).await.unwrap();
    assert!(server.session(&token).await.is_some(), "the session did not resolve before removal");
    let cli = Accounts::open(&url, cfg()).await.unwrap();
    cli.remove_person("ada").await.unwrap();
    assert!(server.session(&token).await.is_none(), "the session survived a removal from another handle");
    cli.add_person("ada", "A New Ada", None).await.unwrap();
    assert!(server.session(&token).await.is_none(), "re-adding the id resurrected the ended session");
    assert!(server.sessions_of("ada", &token).await.is_empty(), "the store still holds the ended session");
}

/// An outstanding invitation cannot undo a disabling: accept refuses
/// alike without consuming the token, which stays usable if the person
/// is turned back on (#449).
#[tokio::test(flavor = "multi_thread")]
async fn an_invite_does_not_reenable() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let token = accounts.invite("ada", None).await.unwrap();
    accounts.set_status("ada", Status::Disabled).await.unwrap();
    assert_eq!(accounts.accept(&token, "a-long-password").await.unwrap(), None, "accepting re-enabled a disabled person");
    assert_eq!(accounts.status("ada").await, Status::Disabled, "accepting flipped the status");
    accounts.set_status("ada", Status::Active).await.unwrap();
    assert_eq!(accounts.accept(&token, "a-long-password").await.unwrap(), Some("ada".to_string()), "the kept token did not survive the refusal");
}

/// Racing accepts of one invitation admit exactly one, round after
/// round: the token is single-use under concurrency too (#450).
#[tokio::test(flavor = "multi_thread")]
async fn racing_accepts_admit_exactly_one() {
    let (_db, url) = db().await;
    let accounts = std::sync::Arc::new(Accounts::open(&url, cfg()).await.unwrap());
    for round in 0..10 {
        let person = format!("ada{round}");
        let token = accounts.invite(&person, None).await.unwrap();
        let mut tasks = vec![];
        for i in 0..8 {
            let (a, t) = (accounts.clone(), token.clone());
            tasks.push(tokio::spawn(async move {
                let pw = format!("a-long-password-{i}");
                matches!(a.accept(&t, &pw).await, Ok(Some(_)))
            }));
        }
        let mut admitted = 0u32;
        for t in tasks {
            admitted += t.await.unwrap() as u32;
        }
        assert_eq!(admitted, 1, "round {round}: racing accepts did not admit exactly one");
    }
}

/// Local roles default to member, set explicitly, and read back (#429).
#[tokio::test(flavor = "multi_thread")]
async fn roles_default_member_and_set_explicitly() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    assert_eq!(accounts.role("ada").await, Role::Member, "an unrecorded person is not a member");
    accounts.set_role("ada", Role::Admin).await.unwrap();
    assert_eq!(accounts.role("ada").await, Role::Admin, "setting admin did not stick");
    accounts.set_role("bea", Role::Member).await.unwrap();
    assert_eq!(accounts.role("bea").await, Role::Member, "setting member did not stick");
}

/// The last enabled admin keeps the role and the sign-in: demoting or
/// disabling them refuses, a disabled admin record does not count, and a
/// second admin unlocks both (#429).
#[tokio::test(flavor = "multi_thread")]
async fn last_admin_keeps_role_and_signin() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    accounts.set_password("bea", "bea-long-password").await.unwrap();
    accounts.set_role("ada", Role::Admin).await.unwrap();
    let e = accounts.set_role("ada", Role::Member).await.unwrap_err();
    assert!(format!("{e:#}").contains("last admin"), "demoting the last admin refused wrongly: {e:#}");
    assert_eq!(accounts.role("ada").await, Role::Admin, "the refused demotion landed");
    let e = accounts.set_status("ada", Status::Disabled).await.unwrap_err();
    assert!(format!("{e:#}").contains("last admin"), "disabling the last admin refused wrongly: {e:#}");
    assert_eq!(accounts.status("ada").await, Status::Active, "the refused disabling landed");
    // A disabled admin on record does not free the switch: nobody
    // enabled would be left to administer.
    accounts.set_role("bea", Role::Admin).await.unwrap();
    accounts.set_status("bea", Status::Disabled).await.unwrap();
    assert!(accounts.set_role("ada", Role::Member).await.is_err(), "a disabled admin record freed the last enabled admin's role");
    accounts.set_status("bea", Status::Active).await.unwrap();
    accounts.set_role("ada", Role::Member).await.unwrap();
    accounts.set_status("ada", Status::Disabled).await.unwrap();
    assert_eq!(accounts.role("ada").await, Role::Member, "a second admin did not unlock the demotion");
    assert_eq!(accounts.status("ada").await, Status::Disabled, "a second admin did not unlock the disabling");
}

/// With no admin records the switch is unmanaged: disabling works, so
/// the guard never locks an appliance that never assigned roles (#429).
#[tokio::test(flavor = "multi_thread")]
async fn no_admin_records_leaves_the_switch_unguarded() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    accounts.set_status("ada", Status::Disabled).await.unwrap();
    assert_eq!(accounts.status("ada").await, Status::Disabled, "disabling with no admin records refused");
}

/// Local people round-trip and remove cleanly: adding records them,
/// removing drops every local trace (person, password, code, status,
/// role, invites, used steps), never touching another's; adding twice
/// or with a wild id refuses, as does removing the last admin (#429).
#[tokio::test(flavor = "multi_thread")]
async fn local_people_round_trip_and_remove_cleanly() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    assert!(!accounts.is_local("ada"), "nobody local yet");
    accounts.add_person("ada", "Ada", Some("ada@example.com")).await.unwrap();
    assert!(accounts.is_local("ada"), "adding did not stick");
    let ada = accounts.local_person("ada").unwrap();
    assert_eq!((ada.name.as_str(), ada.email.as_deref()), ("Ada", Some("ada@example.com")), "the record came back wrong");
    assert!(accounts.add_person("ada", "Ada", None).await.is_err(), "adding twice worked");
    let long = "a".repeat(101);
    for wild in ["../x", "", "a/b", long.as_str()] {
        assert!(accounts.add_person(wild, "X", None).await.is_err(), "a wild id worked: {wild}");
    }
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let uri = accounts.enroll_totp("ada").await.unwrap();
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).unwrap();
    let now = chrono::Utc::now();
    let code = crate::totp::generate(&crate::totp::base32_decode(secret).unwrap(), (now.timestamp() / 30) as u64, 6);
    accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&code), "console", now).await.unwrap();
    let ada_token = accounts.invite("ada", None).await.unwrap();
    let bea_token = accounts.invite("bea", None).await.unwrap();
    accounts.set_role("ada", Role::Admin).await.unwrap();
    accounts.add_person("bea", "Bea", None).await.unwrap();
    accounts.set_role("bea", Role::Admin).await.unwrap();
    assert_eq!(accounts.accept(&bea_token, "bea-long-password").await.unwrap(), Some("bea".to_string()), "bea's invite did not work");
    accounts.remove_person("ada").await.unwrap();
    assert!(!accounts.is_local("ada"), "removing left the person");
    assert!(!accounts.has_password("ada").await, "removing left the password");
    assert!(!accounts.has_totp("ada").await, "removing left the code");
    assert_eq!(accounts.status("ada").await, Status::Active, "removing left the status");
    assert_eq!(accounts.role("ada").await, Role::Member, "removing left the role");
    assert_eq!(accounts.accept(&ada_token, "another-long-password").await.unwrap(), None, "removing left the invite");
    assert!(accounts.has_password("bea").await, "removing ada burned bea's account");
    let left: i64 = {
        let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        client.query_one("SELECT COUNT(*) FROM public.account_totp_steps WHERE person = 'ada'", &[]).await.unwrap().get(0)
    };
    assert_eq!(left, 0, "removing left used steps");
    let e = accounts.remove_person("bea").await.unwrap_err();
    assert!(format!("{e:#}").contains("last admin"), "removing the last admin refused wrongly: {e:#}");
    // A second admin unlocks the removal.
    accounts.add_person("ada", "Ada", None).await.unwrap();
    accounts.set_role("ada", Role::Admin).await.unwrap();
    accounts.remove_person("bea").await.unwrap();
    assert!(!accounts.is_local("bea"), "a second admin did not unlock the removal");
    assert!(accounts.remove_person("ghost").await.is_err(), "removing nobody worked");
}

/// Without an enrolled second factor, the code path behaves exactly as
/// the password path: the feature is off until enrolled (#429).
#[tokio::test(flavor = "multi_thread")]
async fn no_second_factor_behaves_as_passwords_alone() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let counter = Accounts::network_counter("ada");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await, Ok(Ok(()))), "a password without a second factor stopped signing in");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "wrong-password", None, &counter).await, Ok(Err(SignInError::Rejected))), "a wrong password stopped rejecting");
    assert!(matches!(accounts.sign_in_counted_with_code("nobody", "whatever-password", None, &counter).await, Ok(Err(SignInError::Rejected))), "an unknown name stopped rejecting");
}

/// Each counter is its scope then the whole person id, so no person's
/// network counter is another's local or backup one, whatever their ids
/// (review of #154: `local:carol` from the network locked carol locally).
#[tokio::test(flavor = "multi_thread")]
async fn one_persons_counter_is_never_anothers() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    for _ in 0..5 {
        accounts.failed(&Accounts::network_counter("local:carol"));
        accounts.failed(&Accounts::network_counter("backup:carol"));
    }
    assert!(accounts.locked(&Accounts::network_counter("local:carol")).is_some());
    assert!(accounts.locked(&Accounts::local_counter("carol")).is_none(), "local:carol's network failures locked carol on this machine");
    assert!(accounts.locked(&Accounts::backup_counter("carol")).is_none(), "backup:carol's network failures locked carol's backup password");
}

/// Person ids are lowercase-canonical: sign-in lowercases at entry,
/// so creating `Ada` keeps `ada`, and any casing signs in, holds the
/// role and status, and collides on re-adding (#470).
#[tokio::test(flavor = "multi_thread")]
async fn person_ids_are_lowercase_canonical() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.add_person("Ada", "Ada", None).await.unwrap();
    assert!(accounts.is_local("ada"), "adding Ada did not keep ada");
    assert!(accounts.is_local("ADA"), "the casing did not resolve");
    assert!(accounts.add_person("ada", "ada", None).await.is_err(), "ada re-added beside Ada");
    accounts.set_password("ADA", "a-long-password").await.unwrap();
    accounts.set_role("aDa", Role::Admin).await.unwrap();
    for who in ["ada", "Ada", "ADA"] {
        assert!(matches!(accounts.sign_in(who, "a-long-password").await.unwrap(), Ok(())), "{who} cannot sign in");
        assert_eq!(accounts.role(who).await, Role::Admin, "{who} lost the role");
    }
    let token = accounts.invite("Bo", None).await.unwrap();
    assert_eq!(accounts.accept(&token, "bo-long-password").await.unwrap(), Some("bo".to_string()), "the invite did not canonicalize");
    assert!(matches!(accounts.sign_in("BO", "bo-long-password").await.unwrap(), Ok(())), "BO cannot sign in");
    accounts.set_status("bO", Status::Disabled).await.unwrap();
    assert_eq!(accounts.status("bo").await, Status::Disabled, "the disabling missed");
}

/// `magnus migrate` brings the JSON files' contents into the tables
/// once, so the cutover keeps everyone signing in; a rerun changes
/// nothing (#466).
#[tokio::test(flavor = "multi_thread")]
async fn migrate_imports_the_json_files_once() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("accounts");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("people.json"), r#"{"ada":{"name":"Ada","email":"ada@example.com"},"Bea":{"name":"Bea"}}"#).unwrap();
    let phc = phc::hash("a-long-password").unwrap();
    std::fs::write(d.join("passwords.json"), serde_json::to_string(&BTreeMap::from([("ada".to_string(), phc)])).unwrap()).unwrap();
    std::fs::write(d.join("status.json"), r#"{"ada":"active"}"#).unwrap();
    std::fs::write(d.join("roles.json"), r#"{"ada":"admin"}"#).unwrap();
    let (_db, url) = db().await;
    let (mut client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let n = import_json(&mut client, dir.path()).await.unwrap();
    assert_eq!(n, 5, "the import wrote {n} rows, not the two people, password, status and role");
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    assert!(accounts.is_local("ada"), "the person did not survive the import");
    assert!(accounts.local_people().iter().all(|(id, _)| *id == id.to_lowercase()), "the import kept an uppercase key");
    assert_eq!(accounts.role("ada").await, Role::Admin, "the role did not survive the import");
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Ok(())), "the password did not survive the import");
    assert_eq!(import_json(&mut client, dir.path()).await.unwrap(), 0, "a rerun changed rows");
    // A removal after the import stays removed: the next migrate never
    // resurrects the person from the untouched files (review P1: every
    // deploy runs migrate).
    accounts.set_role("bea", Role::Admin).await.unwrap();
    accounts.remove_person("ada").await.unwrap();
    assert_eq!(import_json(&mut client, dir.path()).await.unwrap(), 0, "a migrate after a removal changed rows");
    assert!(!accounts.is_local("ada"), "the migrate resurrected the removed person");
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Err(_)), "the resurrected password signs in");
}

/// The rules seed from the config at first open and round-trip whole:
/// a code optional, the config's lockout, no escalation (#508).
#[tokio::test(flavor = "multi_thread")]
async fn the_sign_in_rules_seed_from_the_config_and_round_trip() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let p = accounts.policy().await.unwrap();
    assert_eq!(
        (p.code_required, p.idle_minutes, p.max_hours, p.lockout_enabled, p.lockout_failures, p.lockout_wait_minutes, p.lockout_max_wait_minutes, p.lockout_permanent),
        (false, 30, 24, true, 5, 15, 15, false),
        "{p:?}"
    );
    let set = Policy {
        code_required: true,
        idle_minutes: 45,
        max_hours: 8,
        lockout_enabled: true,
        lockout_failures: 3,
        lockout_wait_minutes: 2,
        lockout_max_wait_minutes: 10,
        lockout_permanent: false,
    };
    accounts.set_policy(set).await.unwrap();
    assert_eq!(accounts.policy().await.unwrap(), set, "the rules did not round-trip");
    // A second handle reads what the first wrote, not the seed.
    let again = Accounts::open(&url, cfg()).await.unwrap();
    assert_eq!(again.policy().await.unwrap(), set, "the seed overwrote the set rules");
}

/// With a code required, a password alone signs no one in — answered
/// as a wrong password, and uncounted — while an enrolled code still
/// does; optional again, the password works (#508).
#[tokio::test(flavor = "multi_thread")]
async fn a_required_code_refuses_a_password_alone() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.add_person("ada", "Ada", None).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let counter = Accounts::network_counter("ada");
    let mut p = accounts.policy().await.unwrap();
    p.code_required = true;
    accounts.set_policy(p).await.unwrap();
    assert!(
        matches!(accounts.sign_in_counted_with_code("ada", "a-long-password", None, &counter).await.unwrap(), Err(SignInError::Rejected)),
        "a password alone signed in with a code required"
    );
    assert!(accounts.locked(&counter).is_none(), "the required-code refusal counted");
    let uri = accounts.enroll_totp("ada").await.unwrap();
    let secret = uri.split("secret=").nth(1).and_then(|s| s.split('&').next()).unwrap();
    let secret = crate::totp::base32_decode(secret).unwrap();
    let at = chrono::Utc::now();
    let code = crate::totp::generate(&secret, (at.timestamp().div_euclid(30)) as u64, 6);
    assert!(
        matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", None, &counter, at).await.unwrap(), Err(SignInError::NeedsCode)),
        "no code asked after enrolling"
    );
    assert!(matches!(accounts.sign_in_counted_with_code_at("ada", "a-long-password", Some(&code), &counter, at).await.unwrap(), Ok(())), "the enrolled code did not sign in");
    p.code_required = false;
    accounts.set_policy(p).await.unwrap();
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Ok(())), "with a code optional, the password alone does not sign in");
}

/// A temporary password never signs in: the check answers `MustChoose`,
/// choosing with it sets the permanent password and activates, and a
/// temp for an account in use is refused (#508).
#[tokio::test(flavor = "multi_thread")]
async fn a_temporary_password_only_chooses() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.add_person("ada", "Ada", None).await.unwrap();
    accounts.invite("ada", None).await.unwrap();
    accounts.set_temporary_password("ada", "a-temporary-password").await.unwrap();
    assert_eq!(accounts.status("ada").await, Status::Invited, "the temp completed the invitation");
    let counter = Accounts::network_counter("ada");
    assert!(matches!(accounts.sign_in("ada", "a-temporary-password").await.unwrap(), Err(SignInError::MustChoose)), "the temp signed in");
    assert!(
        matches!(accounts.sign_in_counted_with_code("ada", "a-temporary-password", None, &counter).await.unwrap(), Err(SignInError::MustChoose)),
        "the temp signed in past the code check"
    );
    assert!(matches!(accounts.choose_password("ada", "wrong-temp-password", "a-long-password", Via::Network).await.unwrap(), Err(SignInError::Rejected)), "a wrong temp chose");
    assert!(accounts.locked(&counter).is_none(), "one wrong temp locked");
    assert!(matches!(accounts.choose_password("ada", "a-temporary-password", "a-long-password", Via::Network).await.unwrap(), Ok(())), "choosing with the temp failed");
    assert_eq!(accounts.status("ada").await, Status::Active, "choosing did not activate");
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Ok(())), "the chosen password does not sign in");
    assert!(matches!(accounts.sign_in("ada", "a-temporary-password").await.unwrap(), Err(SignInError::Rejected)), "the spent temp still answers MustChoose");
    assert!(accounts.set_temporary_password("ada", "another-temp-password").await.is_err(), "a temp landed on an account in use");
}

/// Lockouts extend while failures continue, cap at the longest wait,
/// and a permanent one holds until an admin's enable clears it; with
/// the lockout off, failures never lock (#508).
#[tokio::test(flavor = "multi_thread")]
async fn lockouts_extend_cap_and_end() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let mut p = accounts.policy().await.unwrap();
    p.lockout_failures = 2;
    p.lockout_wait_minutes = 5;
    p.lockout_max_wait_minutes = 6;
    accounts.set_policy(p).await.unwrap();
    accounts.set_password("ada", "a-long-password").await.unwrap();
    let counter = Accounts::network_counter("ada");
    for _ in 0..2 {
        let _ = accounts.sign_in_counted("ada", "wrong-password", &counter).await;
    }
    let first = accounts.locked(&counter).expect("two failures did not lock");
    assert!((first - chrono::Utc::now()).num_minutes() >= 4, "the first wait is short: {first}");
    let _ = accounts.sign_in_counted("ada", "wrong-password", &counter).await;
    let longer = accounts.locked(&counter).expect("the lock lapsed");
    assert!(longer > first, "failing while locked did not extend: {first} to {longer}");
    assert!((longer - chrono::Utc::now()).num_minutes() <= 6, "the lock passed the longest wait: {longer}");
    // Permanent: still held far out, then an enable clears it.
    p.lockout_permanent = true;
    accounts.set_policy(p).await.unwrap();
    accounts.failed(&counter);
    let held = accounts.locked(&counter).expect("the permanent lock did not hold");
    assert!((held - chrono::Utc::now()).num_days() > 365, "the permanent lock ends: {held}");
    accounts.set_status("ada", Status::Active).await.unwrap();
    assert!(accounts.locked(&counter).is_none(), "enabling did not clear the lock");
    // Off: failures never lock.
    p.lockout_permanent = false;
    p.lockout_enabled = false;
    accounts.set_policy(p).await.unwrap();
    for _ in 0..5 {
        let _ = accounts.sign_in_counted("ada", "wrong-password", &counter).await;
    }
    assert!(accounts.locked(&counter).is_none(), "failures locked with the lockout off");
}

/// Choosing with a temporary password spends the invitation: the
/// shown token cannot reset the new password afterwards — and neither
/// can one after a set password completes the invitation (#527).
#[tokio::test(flavor = "multi_thread")]
async fn finishing_an_invitation_spends_its_token() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.add_person("ada", "Ada", None).await.unwrap();
    let token = accounts.invite("ada", None).await.unwrap();
    accounts.set_temporary_password("ada", "a-temporary-password").await.unwrap();
    assert!(matches!(accounts.choose_password("ada", "a-temporary-password", "a-long-password", Via::Network).await.unwrap(), Ok(())), "choosing failed");
    assert!(accounts.accept("deadbeef-dead-beef-dead-beefdeadbeef", "another-long-password").await.unwrap().is_none(), "a made-up token accepts");
    assert!(accounts.accept(&token, "a-reset-password").await.unwrap().is_none(), "the spent invitation still resets the password");
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Ok(())), "the password is not the chosen one");
    // The same through a set password, which completes invitations too.
    accounts.add_person("bea", "Bea", None).await.unwrap();
    let token = accounts.invite("bea", None).await.unwrap();
    accounts.set_password("bea", "bea-long-password").await.unwrap();
    assert!(accounts.accept(&token, "a-reset-password").await.unwrap().is_none(), "the invitation outlived the set password");
}

/// Accepting installs a permanent password even when a temporary one
/// came first: the chosen password signs in, never MustChoose (#527).
#[tokio::test(flavor = "multi_thread")]
async fn accepting_clears_a_temporary_password() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    accounts.add_person("ada", "Ada", None).await.unwrap();
    let token = accounts.invite("ada", None).await.unwrap();
    accounts.set_temporary_password("ada", "a-temporary-password").await.unwrap();
    assert_eq!(accounts.accept(&token, "a-long-password").await.unwrap().as_deref(), Some("ada"), "accepting failed");
    assert!(matches!(accounts.sign_in("ada", "a-long-password").await.unwrap(), Ok(())), "the accepted password does not sign in");
}

/// A password recovery round trip: the holder's address mints a token
/// once a minute, unknown addresses and quiet people answer nothing,
/// and confirming sets the password permanently, clears the
/// authenticator, ends sessions, and spends the token; a bad, spent
/// or expired token answers nothing (#533).
#[tokio::test(flavor = "multi_thread")]
async fn reset_round_trip_sets_clears_and_consumes() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let invite = accounts.invite_person("ada", "Ada", Some("ada@example.com"), Role::Member).await.unwrap();
    accounts.accept(&invite, "ada-old-password").await.unwrap();
    accounts.enroll_totp("ada").await.unwrap();
    let counter = Accounts::network_counter("ada");

    let first = accounts.request_reset("ada@example.com").await.unwrap().expect("no token for the holder");
    assert!(first.send && !first.token.is_empty(), "the first request did not mint");
    let again = accounts.request_reset(" ADA@example.com ").await.unwrap().expect("no answer for a repeat");
    assert!(!again.send, "the repeat resent inside the minute");
    assert!(accounts.request_reset("nobody@example.com").await.unwrap().is_none(), "an unknown address minted");
    assert!(accounts.request_reset("").await.unwrap().is_none(), "an empty address minted");
    accounts.set_status("ada", Status::Disabled).await.unwrap();
    assert!(accounts.request_reset("ada@example.com").await.unwrap().is_none(), "a disabled person minted");
    accounts.set_status("ada", Status::Active).await.unwrap();
    let (session, token) = accounts.open_session("ada", "app", None).await.unwrap();

    assert!(accounts.confirm_reset("deadbeef", "ada-new-password").await.unwrap().is_none(), "a bad token confirmed");
    assert!(accounts.confirm_reset(&first.token, "short").await.is_err(), "a short password confirmed");
    assert!(accounts.reset_live(&first.token).await, "the short password burned the token");
    let (person, ended) = accounts.confirm_reset(&first.token, "ada-new-password").await.unwrap().expect("the token did not confirm");
    assert_eq!(person, "ada");
    assert_eq!(ended, vec![session.id.clone()], "confirming did not end the live session");
    assert!(accounts.session(&token).await.is_none(), "the session survived confirming");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "ada-new-password", None, &counter).await, Ok(Ok(()))), "the new password does not sign in without a code");
    assert!(matches!(accounts.sign_in_counted_with_code("ada", "ada-old-password", None, &counter).await, Ok(Err(_))), "the old password still signs in");
    assert_eq!(accounts.confirm_reset(&first.token, "ada-newer-password").await.unwrap(), None, "the token worked twice");

    let minute_later = chrono::Utc::now() + chrono::Duration::seconds(61);
    let stale = accounts.request_reset_at("ada@example.com", minute_later).await.unwrap().expect("no token after the minute");
    assert!(stale.send, "the later request did not mint");
    let past = chrono::Utc::now() + chrono::Duration::hours(2);
    assert_eq!(accounts.confirm_reset_at(&stale.token, "ada-newer-password", past).await.unwrap(), None, "an expired token confirmed");
}

/// Removing a person spends their recovery: the token row dies with
/// them, and if the id is recreated within the hour the old link
/// confirms nothing (#541).
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_person_spends_their_recovery() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let invite = accounts.invite_person("ada", "Ada", Some("ada@example.com"), Role::Member).await.unwrap();
    accounts.accept(&invite, "ada-old-password").await.unwrap();
    let first = accounts.request_reset("ada@example.com").await.unwrap().expect("no token for the holder");
    assert!(first.send, "the request did not mint");
    accounts.remove_person("ada").await.unwrap();
    assert!(!accounts.reset_live(&first.token).await, "the token survived the removal");
    assert_eq!(accounts.confirm_reset(&first.token, "ada-newer-password").await.unwrap(), None, "the removed token confirmed");
    accounts.add_person("ada", "Ada", Some("ada@example.com")).await.unwrap();
    assert_eq!(accounts.confirm_reset(&first.token, "ada-newest-password").await.unwrap(), None, "the old token took over the recreated account");
}

/// One call makes an invited admin with a token: person, invited
/// status, role and token commit together, so an unfinished
/// invitation never counts as an enabled admin (#527).
#[tokio::test(flavor = "multi_thread")]
async fn an_invited_admin_is_made_in_one_call() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let token = accounts.invite_person("dee", "Dee Ray", Some("dee@acme.test"), Role::Admin).await.unwrap();
    assert_eq!(accounts.status("dee").await, Status::Invited, "not invited");
    assert_eq!(accounts.role("dee").await, Role::Admin, "not an admin");
    assert_eq!(accounts.accept(&token, "dee-long-password").await.unwrap().as_deref(), Some("dee"), "the token does not accept");
    assert!(accounts.invite_person("dee", "Dee Two", None, Role::Member).await.is_err(), "a held id invites twice");
}

/// Sessions last the rules' longest session, and end when idle past
/// the rules' idle minutes; a live session's last-seen stamp moves
/// (#508).
#[tokio::test(flavor = "multi_thread")]
async fn sessions_last_the_rules_longest_and_end_when_idle() {
    let (_db, url) = db().await;
    let accounts = Accounts::open(&url, cfg()).await.unwrap();
    let mut p = accounts.policy().await.unwrap();
    p.idle_minutes = 30;
    p.max_hours = 8;
    accounts.set_policy(p).await.unwrap();
    let (s, token) = accounts.open_session("ada", "app", None).await.unwrap();
    assert!((s.expires - chrono::Utc::now()).num_hours() >= 7, "the session is short: {}", s.expires);
    assert!(accounts.session(&token).await.is_some(), "a fresh session is not live");
    // Idle past the rules: over.
    let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client.execute("UPDATE public.account_sessions SET seen = now() - '31 minutes'::interval", &[]).await.unwrap();
    assert!(accounts.session(&token).await.is_none(), "an idle session is still live");
    // Used again, the stamp moves.
    client.execute("UPDATE public.account_sessions SET seen = now() - '90 seconds'::interval", &[]).await.unwrap();
    assert!(accounts.session(&token).await.is_some(), "a lately used session is not live");
    let seen: chrono::DateTime<chrono::Utc> = client.query_one("SELECT seen FROM public.account_sessions", &[]).await.unwrap().get(0);
    assert!((chrono::Utc::now() - seen).num_seconds() < 90, "the last-seen stamp did not move: {seen}");
}
