//! `magnus-computerd serve | kill-all | release | self-update | release-key
//! [--config PATH]`, as root.
//! `kill-all` works with the service down and Magnus gone.

use anyhow::{bail, ensure, Result};
use magnus_computerd::computers::{kill_every_computer, Computers, CGROUP_ROOT};
use magnus_computerd::config::Config;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DEFAULT_CONFIG: &str = "/etc/magnus/computerd.toml";

fn kill_all(path: &Path) -> Result<()> {
    // The config can be wrong or gone: the VMs still die, only the block on
    // new starts is lost, and the owner is told so.
    match Config::parse(path) {
        Ok(cfg) => println!("killed {} computers; new starts are blocked until `magnus-computerd release`", Computers::new(cfg).kill_all()?),
        Err(e) => {
            let killed = kill_every_computer(Path::new(CGROUP_ROOT))?;
            println!("killed {killed} computers; new starts are NOT blocked, because the config could not be read: {e:#}");
        }
    }
    Ok(())
}

fn open_snapshot(key: &str, from: &str, to: &str) -> Result<()> {
    let key: [u8; magnus_computerd::seal::KEY_BYTES] = std::fs::read(key)?.try_into().map_err(|_| anyhow::anyhow!("not a snapshot key"))?;
    magnus_computerd::seal::open(&key, &mut std::fs::File::open(from)?, &mut std::fs::File::create(to)?)
}

fn self_update(config: &Path) -> Result<()> {
    use magnus_computerd::self_update::{run, run_installer, Outcome, Paths, Settings, PROGRAM_DIR, RELEASE_KEY, SETTINGS};
    let settings = Settings::load(Path::new(SETTINGS), 0)?;
    let cfg = Config::load(config)?;
    let paths = Paths {
        releases: settings.releases.clone(),
        links_from: settings.links_from.clone(),
        key: PathBuf::from(RELEASE_KEY),
        program_dir: PathBuf::from(PROGRAM_DIR),
        images_dir: cfg.state_dir.join("images"),
        staging: cfg.state_dir.join("update"),
        owner: Some(0),
    };
    match run(&paths, |s| run_installer(&settings.user, s))? {
        Outcome::UpToDate { installed, latest } => println!("magnus-computerd {installed} is up to date (latest release {latest})"),
        Outcome::Installed { from, to } => println!("magnus-computerd updated from {from} to {to}"),
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(unsafe { libc::geteuid() } == 0, "magnus-computerd runs as root");
    let config = args.iter().position(|a| a == "--config").and_then(|i| args.get(i + 1)).map_or_else(|| PathBuf::from(DEFAULT_CONFIG), PathBuf::from);
    match args.first().map(String::as_str) {
        Some("serve") => magnus_computerd::server::serve(Config::load(&config)?),
        Some("kill-all") => kill_all(&config),
        // Started by the service for one connection; see `relay`.
        Some("api") => match (args.get(1), args.get(2).map(|u| u.parse::<u32>()), args.get(3)) {
            (Some(root), Some(Ok(uid)), Some(what)) => magnus_computerd::relay::api(Path::new(root), uid, what),
            _ => bail!("usage: magnus-computerd api ROOT UID WHAT"),
        },
        // For checking what is stored, by hand or in a test: root only.
        Some("purge") => match (args.get(1), args.get(2).map(|s| s.parse::<u64>())) {
            (Some(config), Some(Ok(at))) => {
                magnus_computerd::computers::Computers::new(magnus_computerd::config::Config::parse(Path::new(config))?).purge(SystemTime::UNIX_EPOCH + Duration::from_secs(at))
            }
            _ => bail!("usage: magnus-computerd purge CONFIG UNIX_SECONDS"),
        },
        Some("forward") => match (args.get(1), args.get(2).map(|u| u.parse::<u32>()), args.get(3).map(|g| g.parse::<u32>()), args.get(4), args.get(5)) {
            (Some(ns), Some(Ok(uid)), Some(Ok(gid)), Some(proxy), Some(pids)) => {
                let pids: Result<Vec<i32>, _> = pids.split(',').map(str::parse).collect();
                magnus_computerd::forward::run(Path::new(ns), uid, gid, Path::new(proxy), &pids?)
            }
            _ => bail!("usage: magnus-computerd forward NETNS UID GID PROXY_SOCKET PIDS"),
        },
        Some("open-backups") => match (args.get(1), args.get(2)) {
            (Some(dir), Some(to)) => magnus_computerd::computers::backup::open_all_chunks(Path::new(dir), &mut std::fs::File::create(to)?),
            _ => bail!("usage: magnus-computerd open-backups DIR TO"),
        },
        Some("open-snapshot") => match (args.get(1), args.get(2), args.get(3)) {
            (Some(key), Some(from), Some(to)) => open_snapshot(key, from, to),
            _ => bail!("usage: magnus-computerd open-snapshot KEY FROM TO"),
        },
        Some("relay") => match (args.get(1), args.get(2).map(|u| u.parse::<u32>()), args.get(3).map_or(Ok(magnus_computerd::relay::GUEST_PORT), |p| p.parse::<u32>())) {
            (Some(root), Some(Ok(uid)), Ok(port)) => magnus_computerd::relay::run(Path::new(root), uid, port),
            _ => bail!("usage: magnus-computerd relay ROOT UID [PORT]"),
        },
        // Run by magnus-computerd-update.service, which the installer sets up.
        Some("self-update") => self_update(&config),
        // For the installer's first copy of the key root checks releases with.
        Some("release-key") => {
            println!("{}", magnus_computerd::self_update::BUILT_IN_KEY.trim());
            Ok(())
        }
        Some("release") => {
            Computers::new(Config::parse(&config)?).release()?;
            println!("computers may start again; none was started");
            Ok(())
        }
        _ => bail!("usage: magnus-computerd serve|kill-all|release|self-update|release-key [--config PATH]"),
    }
}
