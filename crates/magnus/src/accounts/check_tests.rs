//! The one password check, against a real Postgres.
use super::super::tests::{cfg, db};
use super::*;
use crate::identity::{Begun, Rights};
use chrono::Duration;

const RIGHT: &str = "ada-right-password";
const WRONG: &str = "not-the-password";

/// How a provider answers a password.
type Answer = fn() -> Result<Polled>;

/// A provider that answers every password the same way.
struct Says(Answer);

#[async_trait::async_trait]
impl Identity for Says {
    fn label(&self) -> String {
        "Keycloak".into()
    }
    async fn begin(&self, _: &str, _: Option<&str>, _: bool) -> Result<Begun> {
        anyhow::bail!("not in this test")
    }
    async fn finish(&self, _: &str, _: &str, _: Option<&str>) -> Result<Polled> {
        anyhow::bail!("not in this test")
    }
    async fn password(&self, _: &str, _: &str, _: Option<&str>, _: bool) -> Result<Polled> {
        (self.0)()
    }
    async fn active(&self, _: &str, _: Option<&str>) -> Result<bool> {
        Ok(true)
    }
    async fn in_directory(&self, _: &str) -> Result<bool> {
        Ok(false)
    }
}

fn done(who: &str) -> Polled {
    Polled::Done { person: who.into(), app: true, rights: Rights::default() }
}
fn up_ada() -> Result<Polled> {
    Ok(done("ada"))
}
fn up_eve() -> Result<Polled> {
    Ok(done("eve"))
}
fn refused() -> Result<Polled> {
    Ok(Polled::Refused("The name or password is wrong.".into()))
}
fn needs_code() -> Result<Polled> {
    Ok(Polled::NeedsCode)
}
fn browser() -> Result<Polled> {
    Ok(Polled::UseBrowser("Sign in on the page.".into()))
}
fn down() -> Result<Polled> {
    anyhow::bail!("connection refused")
}

#[derive(Debug, PartialEq)]
enum Is {
    Yes,
    NeedsCode,
    MustChoose,
    Wrong,
    Locked,
    Elsewhere,
    Unavailable,
}

fn is(c: &Checked) -> Is {
    match c {
        Checked::Yes(_) => Is::Yes,
        Checked::NeedsCode => Is::NeedsCode,
        Checked::MustChoose => Is::MustChoose,
        Checked::Wrong(_) => Is::Wrong,
        Checked::Locked(_) => Is::Locked,
        Checked::Elsewhere(_) => Is::Elsewhere,
        Checked::Unavailable(_) => Is::Unavailable,
    }
}

/// The machine code an attempt brings.
#[derive(Clone, Copy)]
enum Machine {
    None,
    Fresh,
    Bogus,
}

struct Row {
    what: &'static str,
    provider: Option<Answer>,
    known: bool,
    backup_allowed: bool,
    person: &'static str,
    password: &'static str,
    machine: Machine,
    via: Via,
    expect: Is,
    /// Each counter's count after the attempt.
    counts: &'static [(&'static str, u32)],
}

fn count(a: &Accounts, counter: &str) -> u32 {
    a.failures.lock().unwrap().count(counter)
}

fn keeper(provider: Option<Answer>, known: bool, backup_allowed: bool) -> Keeper {
    Keeper { provider: provider.map(|f| Arc::new(Says(f)) as Arc<dyn Identity>), known, backup_allowed }
}

async fn attempt(a: &Accounts, k: &Keeper, person: &str, password: &str, machine: Machine, via: Via) -> Checked {
    let code = match machine {
        Machine::None => None,
        Machine::Fresh => Some(a.new_machine_code().await.unwrap()),
        Machine::Bogus => Some("000000".to_string()),
    };
    a.check_password(k, &Attempt { person, password, code: None, machine_code: code.as_deref(), via, app: true }).await.unwrap()
}

fn row(
    what: &'static str,
    provider: Option<Answer>,
    (known, backup_allowed): (bool, bool),
    (person, password, machine, via): (&'static str, &'static str, Machine, Via),
    expect: Is,
    counts: &'static [(&'static str, u32)],
) -> Row {
    Row { what, provider, known, backup_allowed, person, password, machine, via, expect, counts }
}

/// Every way a password is answered, through the one check: the
/// provider up, down, wanting the code or its page, refusing, or
/// signing in someone else; the store here, from the network and from
/// this machine; the owner's backup; and who is counted where.
#[tokio::test(flavor = "multi_thread")]
async fn every_answer_counts_where_it_should() {
    use Machine::{Bogus, Fresh, None as NoCode};
    use Via::{Network, ThisMachine};
    let (_db, url) = db().await;
    let a = Accounts::open(&url, cfg()).await.unwrap();
    a.set_password("ada", RIGHT).await.unwrap();
    let ours = (true, false);
    let owner = (true, true);
    let stranger = (false, false);
    let rows = [
        row("provider says yes", Some(up_ada), ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
        row("provider signs in someone else", Some(up_eve), ours, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
        row("provider refuses", Some(refused), ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
        row("provider wants the code", Some(needs_code), ours, ("ada", RIGHT, NoCode, Network), Is::NeedsCode, &[("net:ada", 0)]),
        row("provider wants its page", Some(browser), ours, ("ada", RIGHT, NoCode, Network), Is::Elsewhere, &[("net:ada", 0)]),
        row("provider down", Some(down), ours, ("ada", RIGHT, NoCode, Network), Is::Unavailable, &[("net:ada", 0)]),
        row("provider refuses on this machine", Some(refused), ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
        row("provider refuses a stranger", Some(refused), stranger, ("nobody", WRONG, NoCode, Network), Is::Wrong, &[("net:nobody", 0)]),
        row("store says yes", None, ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
        row("store refuses", None, ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
        row("store refuses on this machine", None, ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
        row("store ignores a machine code", None, ours, ("ada", RIGHT, Bogus, ThisMachine), Is::Yes, &[("local:ada", 0)]),
        row("store does not know them", None, stranger, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 0)]),
        row("backup says yes", Some(down), owner, ("ada", RIGHT, Fresh, ThisMachine), Is::Yes, &[("backup:ada", 0)]),
        row("backup, wrong machine code", Some(down), owner, ("ada", RIGHT, Bogus, ThisMachine), Is::Wrong, &[("backup:ada", 1)]),
        row("backup, wrong password", Some(down), owner, ("ada", WRONG, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 1), ("local:ada", 0)]),
        row("backup from the network", Some(down), owner, ("ada", RIGHT, Fresh, Network), Is::Wrong, &[("backup:ada", 0)]),
        row("backup for someone else", Some(down), ours, ("ada", RIGHT, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 0)]),
    ];
    for r in rows {
        a.failures.lock().unwrap().clear_all();
        let k = keeper(r.provider, r.known, r.backup_allowed);
        let got = attempt(&a, &k, r.person, r.password, r.machine, r.via).await;
        assert_eq!(is(&got), r.expect, "{}", r.what);
        for (counter, n) in r.counts {
            assert_eq!(count(&a, counter), *n, "{}: {counter}", r.what);
        }
    }
}

/// Past the threshold the counter locks, whoever judges the password:
/// even the right one is refused, and every locked attempt extends the
/// lock up to the longest wait. Once it lapses the right one signs in
/// and clears it.
#[tokio::test(flavor = "multi_thread")]
async fn the_lockout_holds_extends_and_lapses() {
    let (_db, url) = db().await;
    let a = Accounts::open(&url, cfg()).await.unwrap();
    a.set_password("ada", RIGHT).await.unwrap();
    let mut p = a.policy().await.unwrap();
    p.lockout_failures = 3;
    p.lockout_wait_minutes = 5;
    p.lockout_max_wait_minutes = 6;
    a.set_policy(p).await.unwrap();
    let cases: [(&str, Option<Answer>, Option<Answer>); 2] = [("provider", Some(refused), Some(up_ada)), ("store", None, None)];
    for (what, wrong, right) in cases {
        a.failures.lock().unwrap().clear_all();
        let (wrong, right) = (keeper(wrong, true, false), keeper(right, true, false));
        for n in 1..3 {
            assert_eq!(is(&attempt(&a, &wrong, "ada", WRONG, Machine::None, Via::Network).await), Is::Wrong, "{what}: wrong {n}");
        }
        assert_eq!(is(&attempt(&a, &wrong, "ada", WRONG, Machine::None, Via::Network).await), Is::Wrong, "{what}: the third wrong");
        let first = a.locked("net:ada").unwrap_or_else(|| panic!("{what}: three wrong did not lock"));
        let Checked::Locked(later) = attempt(&a, &right, "ada", RIGHT, Machine::None, Via::Network).await else { panic!("{what}: the lock lapsed") };
        assert!(later > first, "{what}: a locked attempt did not extend the lock: {first} to {later}");
        assert!(later - Utc::now() <= Duration::minutes(6), "{what}: the lock passed the longest wait: {later}");
        assert_eq!(is(&attempt(&a, &right, "ada", RIGHT, Machine::None, Via::ThisMachine).await), Is::Yes, "{what}: the network's lock held this machine");
        a.failures.lock().unwrap().set_until("net:ada", Utc::now() - Duration::seconds(1));
        assert_eq!(is(&attempt(&a, &right, "ada", RIGHT, Machine::None, Via::Network).await), Is::Yes, "{what}: a lapsed lock still refused");
        assert_eq!(count(&a, "net:ada"), 0, "{what}: the right password did not clear the counter");
    }
}

/// The backup's own lock: wrong machine codes lock it, the provider's
/// counter is untouched, and a locked attempt extends it.
#[tokio::test(flavor = "multi_thread")]
async fn the_backup_locks_apart() {
    let (_db, url) = db().await;
    let a = Accounts::open(&url, cfg()).await.unwrap();
    a.set_password("ada", RIGHT).await.unwrap();
    let mut p = a.policy().await.unwrap();
    p.lockout_failures = 2;
    p.lockout_wait_minutes = 5;
    p.lockout_max_wait_minutes = 6;
    a.set_policy(p).await.unwrap();
    let k = keeper(Some(down), true, true);
    for _ in 0..2 {
        assert_eq!(is(&attempt(&a, &k, "ada", RIGHT, Machine::Bogus, Via::ThisMachine).await), Is::Wrong);
    }
    let first = a.locked("backup:ada").expect("wrong machine codes did not lock the backup");
    let Checked::Locked(later) = attempt(&a, &k, "ada", RIGHT, Machine::Fresh, Via::ThisMachine).await else { panic!("the backup's lock lapsed") };
    assert!(later > first, "a locked backup attempt did not extend the lock");
    assert_eq!(count(&a, "local:ada") + count(&a, "net:ada"), 0, "the backup counted on the provider's counters");
}

/// The same check over the in-memory store: no Postgres needed.
mod table {
    use super::super::super::credentials::table::Table;
    use super::super::super::lockout::Lockout;
    use super::*;
    use crate::accounts::Policy;
    use std::sync::Mutex;

    /// A store with ada's password, fresh counters and the rules.
    struct Bench {
        table: Table,
        lockout: Mutex<Lockout>,
        policy: Policy,
    }

    impl Bench {
        fn new(change: impl FnOnce(&mut Policy)) -> Bench {
            let mut policy = Policy::seed(&cfg());
            change(&mut policy);
            Bench { table: Table::default().with_password("ada", RIGHT), lockout: Mutex::new(Lockout::default()), policy }
        }

        fn checker(&self) -> Checker<'_, Table> {
            Checker { creds: &self.table, lockout: &self.lockout, policy: self.policy }
        }

        fn count(&self, counter: &str) -> u32 {
            self.lockout.lock().unwrap().count(counter)
        }

        async fn attempt(&self, k: &Keeper, person: &str, password: &str, machine: Machine, via: Via) -> Checked {
            let code = match machine {
                Machine::None => None,
                Machine::Fresh => Some(self.table.new_machine_code()),
                Machine::Bogus => Some("000000".to_string()),
            };
            self.checker().check_password(k, &Attempt { person, password, code: None, machine_code: code.as_deref(), via, app: true }).await.unwrap()
        }
    }

    /// `every_answer_counts_where_it_should`, over the table.
    #[tokio::test]
    async fn every_answer_counts_where_it_should_without_a_database() {
        use Machine::{Bogus, Fresh, None as NoCode};
        use Via::{Network, ThisMachine};
        let b = Bench::new(|_| {});
        let ours = (true, false);
        let owner = (true, true);
        let stranger = (false, false);
        let rows = [
            row("provider says yes", Some(up_ada), ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
            row("provider signs in someone else", Some(up_eve), ours, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("provider refuses", Some(refused), ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("provider wants the code", Some(needs_code), ours, ("ada", RIGHT, NoCode, Network), Is::NeedsCode, &[("net:ada", 0)]),
            row("provider wants its page", Some(browser), ours, ("ada", RIGHT, NoCode, Network), Is::Elsewhere, &[("net:ada", 0)]),
            row("provider down", Some(down), ours, ("ada", RIGHT, NoCode, Network), Is::Unavailable, &[("net:ada", 0)]),
            row("provider refuses on this machine", Some(refused), ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
            row("provider refuses a stranger", Some(refused), stranger, ("nobody", WRONG, NoCode, Network), Is::Wrong, &[("net:nobody", 0)]),
            row("store says yes", None, ours, ("ada", RIGHT, NoCode, Network), Is::Yes, &[("net:ada", 0)]),
            row("store refuses", None, ours, ("ada", WRONG, NoCode, Network), Is::Wrong, &[("net:ada", 1)]),
            row("store refuses on this machine", None, ours, ("ada", WRONG, NoCode, ThisMachine), Is::Wrong, &[("local:ada", 1), ("net:ada", 0)]),
            row("store ignores a machine code", None, ours, ("ada", RIGHT, Bogus, ThisMachine), Is::Yes, &[("local:ada", 0)]),
            row("store does not know them", None, stranger, ("ada", RIGHT, NoCode, Network), Is::Wrong, &[("net:ada", 0)]),
            row("backup says yes", Some(down), owner, ("ada", RIGHT, Fresh, ThisMachine), Is::Yes, &[("backup:ada", 0)]),
            row("backup, wrong machine code", Some(down), owner, ("ada", RIGHT, Bogus, ThisMachine), Is::Wrong, &[("backup:ada", 1)]),
            row("backup, wrong password", Some(down), owner, ("ada", WRONG, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 1), ("local:ada", 0)]),
            row("backup from the network", Some(down), owner, ("ada", RIGHT, Fresh, Network), Is::Wrong, &[("backup:ada", 0)]),
            row("backup for someone else", Some(down), ours, ("ada", RIGHT, Fresh, ThisMachine), Is::Wrong, &[("backup:ada", 0)]),
        ];
        for r in rows {
            b.lockout.lock().unwrap().clear_all();
            let k = keeper(r.provider, r.known, r.backup_allowed);
            let got = b.attempt(&k, r.person, r.password, r.machine, r.via).await;
            assert_eq!(is(&got), r.expect, "{}", r.what);
            for (counter, n) in r.counts {
                assert_eq!(b.count(counter), *n, "{}: {counter}", r.what);
            }
        }
    }

    /// `the_lockout_holds_extends_and_lapses`, over the table.
    #[tokio::test]
    async fn the_lockout_holds_extends_and_lapses_without_a_database() {
        let b = Bench::new(|p| {
            p.lockout_failures = 3;
            p.lockout_wait_minutes = 5;
            p.lockout_max_wait_minutes = 6;
        });
        let cases: [(&str, Option<Answer>, Option<Answer>); 2] = [("provider", Some(refused), Some(up_ada)), ("store", None, None)];
        for (what, wrong, right) in cases {
            b.lockout.lock().unwrap().clear_all();
            let (wrong, right) = (keeper(wrong, true, false), keeper(right, true, false));
            for n in 1..=3 {
                assert_eq!(is(&b.attempt(&wrong, "ada", WRONG, Machine::None, Via::Network).await), Is::Wrong, "{what}: wrong {n}");
            }
            let first = b.lockout.lock().unwrap().locked("net:ada", Utc::now(), &b.policy).unwrap_or_else(|| panic!("{what}: three wrong did not lock"));
            let Checked::Locked(later) = b.attempt(&right, "ada", RIGHT, Machine::None, Via::Network).await else { panic!("{what}: the lock lapsed") };
            assert!(later > first, "{what}: a locked attempt did not extend the lock: {first} to {later}");
            assert!(later - Utc::now() <= Duration::minutes(6), "{what}: the lock passed the longest wait: {later}");
            assert_eq!(is(&b.attempt(&right, "ada", RIGHT, Machine::None, Via::ThisMachine).await), Is::Yes, "{what}: the network's lock held this machine");
            b.lockout.lock().unwrap().set_until("net:ada", Utc::now() - Duration::seconds(1));
            assert_eq!(is(&b.attempt(&right, "ada", RIGHT, Machine::None, Via::Network).await), Is::Yes, "{what}: a lapsed lock still refused");
            assert_eq!(b.count("net:ada"), 0, "{what}: the right password did not clear the counter");
        }
    }

    /// `the_backup_locks_apart`, over the table.
    #[tokio::test]
    async fn the_backup_locks_apart_without_a_database() {
        let b = Bench::new(|p| {
            p.lockout_failures = 2;
            p.lockout_wait_minutes = 5;
            p.lockout_max_wait_minutes = 6;
        });
        let k = keeper(Some(down), true, true);
        for _ in 0..2 {
            assert_eq!(is(&b.attempt(&k, "ada", RIGHT, Machine::Bogus, Via::ThisMachine).await), Is::Wrong);
        }
        let first = b.lockout.lock().unwrap().locked("backup:ada", Utc::now(), &b.policy).expect("wrong machine codes did not lock the backup");
        let Checked::Locked(later) = b.attempt(&k, "ada", RIGHT, Machine::Fresh, Via::ThisMachine).await else { panic!("the backup's lock lapsed") };
        assert!(later > first, "a locked backup attempt did not extend the lock");
        assert_eq!(b.count("local:ada") + b.count("net:ada"), 0, "the backup counted on the provider's counters");
    }

    /// A code signs in once, and a later step's code still does: the
    /// table's step moves only forward, as the store's does (#441).
    #[tokio::test]
    async fn a_code_signs_in_once_without_a_database() {
        let b = Bench::new(|_| {});
        let raw = [7u8; 20];
        b.table.totp.lock().unwrap().insert("ada".into(), crate::totp::base32_encode(&raw));
        let at = Utc::now();
        let step = at.timestamp().div_euclid(30) as u64;
        let code = crate::totp::generate(&raw, step, 6);
        let c = b.checker();
        assert!(matches!(c.sign_in_counted_with_code_at("ada", RIGHT, None, "net:ada", at).await, Ok(Err(SignInError::NeedsCode))), "the password alone passed the code");
        assert!(matches!(c.sign_in_counted_with_code_at("ada", RIGHT, Some(&code), "net:ada", at).await, Ok(Ok(()))), "the code did not sign in");
        assert!(matches!(c.sign_in_counted_with_code_at("ada", RIGHT, Some(&code), "net:ada", at).await, Ok(Err(SignInError::Rejected))), "the code signed in twice");
        assert_eq!(b.count("net:ada"), 1, "the replay was not counted");
        b.table.statuses.lock().unwrap().insert("ada".into(), crate::accounts::Status::Disabled);
        assert!(matches!(c.sign_in_counted_with_code_at("ada", RIGHT, None, "net:ada", at).await, Ok(Err(SignInError::Rejected))), "a disabled person was asked for the code");
    }
}
