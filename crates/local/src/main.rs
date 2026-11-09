//! `magnus-local --server URL --name NAME --mode ask|allow|never --folder PATH...`
//! connects this computer to a person's Magnus. The token comes from
//! `MAGNUS_LOCAL_TOKEN`, so it is not in the process list.
use anyhow::{bail, Context, Result};
use magnus_local::{serve, Config, Folders};
use std::path::PathBuf;

const DEFAULT_BWRAP: &str = "bwrap";

fn config() -> Result<Config> {
    let (mut server, mut name, mut mode, mut folders, mut bwrap) = (None, None, "ask".to_string(), vec![], PathBuf::from(DEFAULT_BWRAP));
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--server" => server = Some(value),
            "--name" => name = Some(value),
            "--mode" => mode = value,
            "--folder" => folders.push(PathBuf::from(value)),
            "--bwrap" => bwrap = PathBuf::from(value),
            other => bail!("unknown option {other}"),
        }
    }
    if !["ask", "allow", "never"].contains(&mode.as_str()) {
        bail!("--mode is ask, allow or never");
    }
    Ok(Config {
        server: server.context("--server is required")?,
        token: std::env::var("MAGNUS_LOCAL_TOKEN").context("MAGNUS_LOCAL_TOKEN is required")?,
        name: name.context("--name is required")?,
        mode,
        folders: Folders::new(&folders)?,
        bwrap,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    serve(config()?).await
}
