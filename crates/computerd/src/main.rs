//! `magnus-computerd serve | kill-all | release [--config PATH]`, as root.
//! `kill-all` works with the service down and Magnus gone.

use anyhow::{bail, ensure, Result};
use magnus_computerd::computers::{kill_every_computer, Computers, CGROUP_ROOT};
use magnus_computerd::config::Config;
use std::path::{Path, PathBuf};

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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(unsafe { libc::geteuid() } == 0, "magnus-computerd runs as root");
    let config = args.iter().position(|a| a == "--config").and_then(|i| args.get(i + 1)).map_or_else(|| PathBuf::from(DEFAULT_CONFIG), PathBuf::from);
    match args.first().map(String::as_str) {
        Some("serve") => magnus_computerd::server::serve(Config::load(&config)?),
        Some("kill-all") => kill_all(&config),
        // Started by the service for one connection; see `relay`.
        Some("relay") => match (args.get(1), args.get(2).map(|u| u.parse::<u32>())) {
            (Some(root), Some(Ok(uid))) => magnus_computerd::relay::run(Path::new(root), uid),
            _ => bail!("usage: magnus-computerd relay ROOT UID"),
        },
        Some("release") => {
            Computers::new(Config::parse(&config)?).release()?;
            println!("computers may start again; none was started");
            Ok(())
        }
        _ => bail!("usage: magnus-computerd serve|kill-all|release [--config PATH]"),
    }
}
