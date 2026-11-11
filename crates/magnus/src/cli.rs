//! `magnus migrate --config <file> --admin-url <url>`: create the event log
//! and the kernel's database role.
//! `magnus serve --config <file>`: run the appliance.
//! `magnus password --config <file> --person <id>`: set a person's sign-in
//! password, read from standard input.
//! `magnus kill --config <file> [--release]`: stop everything on the server
//! (or release the switch), from the server itself (`crate::kill_cli`).

use crate::config::Config;
use anyhow::{Context, Result};
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// A run's nested tool calls are deep futures: the default 2 MiB overflowed and
/// took the whole process with it.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

pub fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_env("MAGNUS_LOG").unwrap_or_else(|_| "info".into())).with_writer(std::io::stderr).init();
    let args: Vec<String> = std::env::args().collect();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(WORKER_STACK_BYTES).build().expect("tokio runtime");
    let r = rt.block_on(async {
        match args.get(1).map(String::as_str) {
            Some("migrate") => migrate(&args).await,
            Some("serve") => serve(&args).await,
            Some("conformance") => conformance(&args).await,
            Some("password") => password(&args).await,
            Some("totp-enroll") => totp_enroll(&args).await,
            Some("invite") => invite(&args).await,
            Some("person-status") => person_status(&args).await,
            Some("person-role") => person_role(&args).await,
            Some("person-add") => person_add(&args).await,
            Some("person-remove") => person_remove(&args).await,
            Some("sign-in-code") => sign_in_code(&args).await,
            Some("restore") => restore(&args).await,
            Some("check") => check(&args).await,
            Some("kill") => kill(&args).await,
            Some("version") => {
                println!("{}", crate::update::version());
                Ok(())
            }
            _ => {
                eprintln!("usage: magnus migrate --config <file> --admin-url <url>\n       magnus serve --config <file>\n       magnus password --config <file> --person <id>   (password on stdin)\n       magnus totp-enroll --config <file> --person <id>   (prints the enrollment URI)\n       magnus invite --config <file> --person <id> [--email <address>]   (prints the invite token)\n       magnus person-status --config <file> --person <id> --status active|disabled\n       magnus person-role --config <file> --person <id> --role admin|member\n       magnus person-add --config <file> --person <id> [--name <name>] [--email <address>]\n       magnus person-remove --config <file> --person <id>\n       magnus conformance --config <file> --adapter <id> [--model <id>]\n       magnus restore --config <file> --admin-url <url> --from <backup file>\n       magnus check --config <file>   (reads the log and the vault, as a new release must)\n       magnus kill --config <file> [--release]   (stops everything on the server, from the server itself)\n       magnus version");
                std::process::exit(2);
            }
        }
    });
    if let Err(e) = r {
        eprintln!("magnus: {e:#}");
        std::process::exit(1);
    }
}

/// `magnus kill --config <file> [--release]`: the server's kill switch from
/// the server itself, through its control socket (`crate::kill_cli`).
async fn kill(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let release = args.iter().any(|a| a == "--release");
    println!("{}", crate::kill_cli::kill(&cfg.appliance.state_dir, &crate::kill_cli::operator(), release).await?);
    Ok(())
}

fn config(args: &[String]) -> Result<Config> {
    let path = PathBuf::from(arg(args, "--config").context("--config is required")?);
    Config::load(&path).with_context(|| format!("load {}", path.display()))
}

/// Put a backup back into an empty event log and state directory (K10).
async fn restore(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let admin = arg(args, "--admin-url").context("--admin-url is required")?;
    let from = PathBuf::from(arg(args, "--from").context("--from is required")?);
    let (n, last) = crate::backup::restore(&cfg, &admin, &from).await?;
    println!("restored {n} events through {last} and the vault from {}", from.display());
    Ok(())
}

/// A new release proves it can read this appliance before it is switched to.
async fn check(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    if let Some(browser) = &cfg.browser {
        crate::browser::check_jail(&browser.bwrap)?;
    }
    println!("{}", crate::update::check(&cfg).await?);
    Ok(())
}

async fn migrate(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let admin = arg(args, "--admin-url").context("--admin-url is required")?;
    crate::log::migrate(&admin, &cfg.database.url, None).await?;
    // Once: the JSON accounts files into the new tables, so the cutover
    // keeps everyone signing in; a rerun changes nothing (#466).
    let mut kernel = crate::log::connect(&cfg.database.url).await?;
    let n = crate::accounts::import_json(&mut kernel, &cfg.appliance.state_dir).await?;
    if n > 0 {
        eprintln!("migrated {n} account rows from the JSON files");
    }
    Ok(())
}

async fn serve(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    crate::update::on_start(&cfg)?;
    // A panic while starting is a failed start too, not a process that is
    // gone with the switch unrecorded.
    use futures::FutureExt;
    let r = match std::panic::AssertUnwindSafe(run(cfg.clone())).catch_unwind().await {
        Ok(r) => r,
        Err(p) => Err(anyhow::anyhow!("panicked: {}", p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
    };
    // A release just switched to that cannot serve goes back to the one before.
    if let Err(e) = &r {
        if let Some(p) = crate::update::pending(&cfg) {
            crate::update::roll_back_now(&cfg, &p, &format!("{e:#}"))?;
        }
    }
    r
}

/// Whether `configured` is the address `bound` already listens on. Port 0
/// is the harness asking for whatever the socket was given.
pub(crate) fn use_inherited_listen(configured: &str, bound: &str) -> bool {
    configured == bound || configured.ends_with(":0")
}

fn set_cloexec(fd: i32, on: bool) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return;
    }
    let flags = if on { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
    unsafe { libc::fcntl(fd, libc::F_SETFD, flags) };
}

/// The listening socket a test harness passed as `MAGNUS_LISTEN_FD`, when
/// this process should serve on it. The numbered fd stays open and
/// close-on-exec, so a later exec (an update, a rollback) still has it and
/// child processes do not; the listener returned is a duplicate. A
/// configured address that is neither port 0 nor this socket is bound by
/// the caller (a release under test that must fail to listen).
pub(crate) fn take_listen_fd(configured: &str) -> Result<Option<tokio::net::TcpListener>> {
    use std::os::unix::io::{AsRawFd, FromRawFd};
    let Ok(raw) = std::env::var("MAGNUS_LISTEN_FD") else { return Ok(None) };
    let fd: i32 = raw.parse().with_context(|| format!("MAGNUS_LISTEN_FD={raw}"))?;
    set_cloexec(fd, true);
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        return Err(std::io::Error::last_os_error()).context("duplicate MAGNUS_LISTEN_FD");
    }
    // SAFETY: `dup` is a new fd from libc::dup of the harness socket, not
    // owned by anything else in this process. Closing it closes only the copy.
    let stdl = unsafe { std::net::TcpListener::from_raw_fd(dup) };
    stdl.set_nonblocking(true).context("listen socket nonblocking")?;
    set_cloexec(stdl.as_raw_fd(), true);
    let bound = stdl.local_addr().context("inherited listen address")?.to_string();
    if !use_inherited_listen(configured, &bound) {
        drop(stdl);
        return Ok(None);
    }
    Ok(Some(tokio::net::TcpListener::from_std(stdl).context("inherited listen socket")?))
}

/// Clear close-on-exec on the harness socket so the process that replaces
/// this one (an update or a rollback) inherits it. The variable stays set.
pub(crate) fn release_listen_socket_for_exec() {
    let Ok(raw) = std::env::var("MAGNUS_LISTEN_FD") else { return };
    let Ok(fd) = raw.parse::<i32>() else { return };
    set_cloexec(fd, false);
}

async fn run(cfg: Config) -> Result<()> {
    use crate::api::{router, Transport};
    use std::net::SocketAddr;
    let listen = cfg.appliance.listen.clone();
    let tls = match &cfg.appliance.tls_listen {
        Some(addr) => {
            let (server, fingerprint) = crate::tls::load(&cfg.appliance.state_dir, cfg.appliance.tls_cert.as_deref(), cfg.appliance.tls_key.as_deref())?;
            Some((addr.clone(), server, fingerprint))
        }
        None => None,
    };
    // Before anything this process spawns: the inherited socket must not be
    // handed to an adapter, and the harness is already holding its own copy.
    let inherited = take_listen_fd(&listen)?;
    crate::reap::reap_inherited();
    let k = crate::kernel::Kernel::boot(cfg).await?;
    let plain = match inherited {
        Some(l) => l,
        None => tokio::net::TcpListener::bind(&listen).await.with_context(|| format!("listen on {listen}"))?,
    };
    // The bound address, which differs from `listen` when it names port 0.
    let bound = plain.local_addr()?;
    let tls = match tls {
        Some((addr, server, fingerprint)) => Some((crate::tls::Listener::bind(&addr, server).await?, addr, fingerprint)),
        None => None,
    };
    // Serving: an update that brought this release here is done.
    k.after_start().await?;
    crate::backup::schedule(k.clone());
    crate::trash::schedule(k.clone());
    k.watch_sessions();
    crate::update::schedule(k.clone());
    crate::computer::schedule(k.clone());
    crate::unattended::schedule(k.clone());
    crate::team::schedule(k.clone());
    crate::spend::schedule(k.clone());
    tokio::spawn(k.clone().sweep());
    // The admin console's way to the kernel: a local socket, never the
    // network, listening before the network is answered.
    match crate::control::bind(&k.cfg.appliance.state_dir) {
        Ok(l) => {
            let k = k.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::control::serve(k, l).await {
                    tracing::error!("control socket: {e:#}");
                }
            });
        }
        Err(e) => tracing::error!("control socket: {e:#}"),
    }
    tracing::info!("magnus {} listening on http://{bound}", crate::update::version());
    let plain = axum::serve(plain, router(k.clone(), Transport::Plain).into_make_service_with_connect_info::<SocketAddr>());
    match tls {
        Some((l, _, fingerprint)) => {
            use axum::serve::{Listener, ListenerExt};
            // The bound address, as for the plain listener: it differs from
            // `tls_listen` when that names port 0.
            let bound = l.local_addr()?;
            // tap_io gives the TLS listener axum's connect-info for its peer address.
            let l = l.tap_io(|_| {});
            tracing::info!("magnus listening on https://{bound}, certificate SHA-256 {fingerprint}");
            let secure = axum::serve(l, router(k, Transport::Tls).into_make_service_with_connect_info::<SocketAddr>());
            tokio::try_join!(async { plain.await }, async { secure.await })?;
        }
        None => plain.await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_inherited_socket_is_used_only_for_the_address_it_already_is() {
        assert!(super::use_inherited_listen("127.0.0.1:0", "127.0.0.1:40059"));
        assert!(super::use_inherited_listen("127.0.0.1:40059", "127.0.0.1:40059"));
        // A release under test that names another port must bind that port
        // and fail, not serve on the socket the harness is holding.
        assert!(!super::use_inherited_listen("127.0.0.1:40060", "127.0.0.1:40059"));
        assert!(!super::use_inherited_listen("127.0.0.1:10", "127.0.0.1:40059"));
    }
}

/// Open the accounts store for a person managed here: in the config, or
/// kept by the appliance itself (#429).
async fn managed_accounts(cfg: &crate::config::Config, person: &str) -> Result<crate::accounts::Accounts> {
    let accounts = crate::accounts::Accounts::open(&cfg.database.url, cfg.sign_in.clone()).await?;
    anyhow::ensure!(cfg.person(person).is_some() || accounts.is_local(person), "{person} is not a person here");
    Ok(accounts)
}

/// Whether the person signs in with a local password: anyone the
/// appliance itself keeps, or a config person allowed one.
fn local_password(cfg: &crate::config::Config, accounts: &crate::accounts::Accounts, person: &str) -> bool {
    accounts.is_local(person) || cfg.may_use_password(person)
}

/// Add a person to the appliance's own directory: they sign in with a
/// local password once invited or given one (#429). Refuses an id the
/// config already has, which would never resolve to them.
async fn person_add(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let name = arg(args, "--name").unwrap_or_else(|| person.clone());
    // The store keeps ids lowercase, so `SAM` would land on `sam`: refuse
    // against the config case-insensitively, or the local record shadows
    // the config person and its later removal burns their credentials (#470).
    let person = person.to_lowercase();
    anyhow::ensure!(!cfg.people.iter().any(|p| p.id.to_lowercase() == person), "{person} is already a person in the config");
    let accounts = crate::accounts::Accounts::open(&cfg.database.url, cfg.sign_in.clone()).await?;
    accounts.add_person(&person, &name, arg(args, "--email").as_deref()).await?;
    eprintln!("added {person}");
    Ok(())
}

/// Remove a person from the appliance's own directory, with every local
/// trace: password, authenticator, status, role, invites, sessions.
/// Refuses the organisation's last enabled admin.
async fn person_remove(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = crate::accounts::Accounts::open(&cfg.database.url, cfg.sign_in.clone()).await?;
    let ended = accounts.remove_person(&person).await?;
    if ended.is_empty() {
        eprintln!("removed {person}");
    } else {
        eprintln!("removed {person}; ended sessions: {}", ended.join(", "));
    }
    Ok(())
}

/// Set a person's password from standard input (one line), on the server's
/// own machine: the owner's way back in, and anyone's when no recovery email
/// can reach them. Every session of theirs ends, as a recovery's does.
async fn password(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = managed_accounts(&cfg, &person).await?;
    anyhow::ensure!(local_password(&cfg, &accounts, &person), "{person} signs in through the identity provider; only the owner keeps a password on the appliance, as a backup");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let pw = line.trim_end_matches(['\r', '\n']);
    accounts.set_password(&person, pw).await?;
    let ended = accounts.end_all(&person).await?;
    if ended.is_empty() {
        eprintln!("password set for {person}");
    } else {
        eprintln!("password set for {person}; ended sessions: {}", ended.join(", "));
    }
    Ok(())
}

/// Enroll a person's authenticator app for local password sign-in: print
/// the enrollment URI (scan it or type its secret into the app). Only
/// someone with a shell on the appliance's machine can run this, so the
/// secret it prints never crosses the network.
async fn totp_enroll(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = managed_accounts(&cfg, &person).await?;
    anyhow::ensure!(local_password(&cfg, &accounts, &person), "{person} signs in through the identity provider; only local passwords take a second factor");
    println!("{}", accounts.enroll_totp(&person).await?);
    eprintln!("enrolled {person}: add the URI above to an authenticator app");
    Ok(())
}

/// Invite a person to local password sign-in: print the single-use token
/// (good for 7 days) to pass on; they accept it with their new password
/// at `/v1/invites/accept`.
async fn invite(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = managed_accounts(&cfg, &person).await?;
    anyhow::ensure!(local_password(&cfg, &accounts, &person), "{person} signs in through the identity provider; only local passwords are invited");
    println!("{}", accounts.invite(&person, arg(args, "--email").as_deref()).await?);
    eprintln!("invited {person}: pass them the token above within 7 days");
    Ok(())
}

/// Turn a local-password person on or off. Disabling ends their live
/// sessions at once; the ids of those ended are printed.
async fn person_status(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = managed_accounts(&cfg, &person).await?;
    anyhow::ensure!(local_password(&cfg, &accounts, &person), "{person} signs in through the identity provider; only local sign-in is switched here");
    let status = match arg(args, "--status").as_deref() {
        Some("active") => crate::accounts::Status::Active,
        Some("disabled") => crate::accounts::Status::Disabled,
        x => anyhow::bail!("--status is active or disabled, not {}", x.unwrap_or("(missing)")),
    };
    let ended = accounts.set_status(&person, status).await?;
    if ended.is_empty() {
        eprintln!("{person} is now {}", if status == crate::accounts::Status::Disabled { "disabled" } else { "active" });
    } else {
        eprintln!("{person} is now disabled; ended sessions: {}", ended.join(", "));
    }
    Ok(())
}

/// Set a local-password person's console role: admins enter the console,
/// members do not. Refuses to demote the organisation's last enabled
/// admin.
async fn person_role(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let person = arg(args, "--person").context("--person is required")?;
    let accounts = managed_accounts(&cfg, &person).await?;
    anyhow::ensure!(local_password(&cfg, &accounts, &person), "{person} signs in through the identity provider; only local sign-in takes a local role");
    let role = match arg(args, "--role").as_deref() {
        Some("admin") => crate::accounts::Role::Admin,
        Some("member") => crate::accounts::Role::Member,
        x => anyhow::bail!("--role is admin or member, not {}", x.unwrap_or("(missing)")),
    };
    accounts.set_role(&person, role).await?;
    eprintln!("{person} is now {}", if role == crate::accounts::Role::Admin { "an admin" } else { "a member" });
    Ok(())
}

/// A one-time code for the owner's backup password, printed on the
/// appliance's own machine: with it, the backup works when the identity
/// provider is down.
async fn sign_in_code(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let accounts = crate::accounts::Accounts::open(&cfg.database.url, cfg.sign_in.clone()).await?;
    println!("{}", accounts.new_machine_code().await?);
    eprintln!("good for one sign-in in the next 10 minutes");
    Ok(())
}

/// Run an adapter's conformance suite with the appliance's vault, for release
/// checks. Prints the report; exits 1 unless every check passes.
async fn conformance(args: &[String]) -> Result<()> {
    let cfg = config(args)?;
    let id = arg(args, "--adapter").context("--adapter is required")?;
    let model = match arg(args, "--model") {
        Some(m) => m,
        None if cfg.model.adapter == id => cfg.model.name.clone(),
        None => anyhow::bail!("--model is required: the configured model belongs to adapter {}", cfg.model.adapter),
    };
    let vault = std::sync::Arc::new(crate::vault::Vault::open(&cfg.appliance.state_dir)?);
    let a = cfg.adapters.iter().find(|a| a.id == id).context("no such adapter in the config")?.clone();
    let adapters = crate::adapters::Adapters::new(&cfg.appliance.state_dir, vault, &cfg.adapters);
    let r = adapters.launch(&a).await?;
    let report = adapters.conformance(&r, &model).await;
    println!("{}", serde_json::to_string_pretty(&report)?);
    anyhow::ensure!(report.passed, "conformance failed");
    Ok(())
}
