//! The computer's desktop as the kernel sees it (spec rev 145, The desktop):
//! its look follows its person's app theme, and an app the agent installs
//! goes in its menu. The desktop itself is the base image's `magnus-desktop`
//! (sandbox/desktop).

use super::{client, Change, Owner, Role, Status, Theme};
use crate::config::Person;
use crate::controls::Refused;
use crate::kernel::Kernel;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::path::Path;

/// Seconds, for the guest's own limit on a desktop command.
const DESKTOP_COMMAND_SECONDS: u64 = 30;

/// The guest command that sets the look. An image without the desktop
/// program has no look to set.
pub(crate) fn theme_command(theme: Theme) -> String {
    format!("command -v magnus-desktop >/dev/null || exit 0; magnus-desktop theme {}", theme.name())
}

/// The guest command that puts an installed app in the menu.
pub(crate) fn app_command(name: &str) -> String {
    format!("command -v magnus-desktop >/dev/null || exit 0; magnus-desktop app-installed {}", shell_quote(name))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

async fn run(socket: &Path, id: &str, command: String) -> Result<()> {
    let done = client::guest(socket, id, json!({ "op": "exec", "command": command, "timeout_s": DESKTOP_COMMAND_SECONDS })).await?;
    ensure!(done["exit_code"] == json!(0), "{}", done["stderr"].as_str().unwrap_or_default());
    Ok(())
}

impl Kernel {
    /// A person's app theme, for their own computer's desktop. A team's
    /// computer keeps the default look: it has many people and no one theme.
    pub async fn set_desktop_theme(&self, who: &Person, id: &str, theme: Theme) -> Result<Value, Refused> {
        let c =
            self.read(|s| s.computers.get(id).filter(|c| c.role == Role::Computer && c.owner == Owner::Person(who.id.clone())).cloned()).ok_or(Refused::NotFound("computer"))?;
        if c.theme != theme {
            self.record_computer(who, Change::DesktopTheme { id: id.into(), theme }).await?;
        }
        if c.status == Status::Running {
            if let Ok(socket) = self.computer_socket() {
                if let Err(e) = run(&socket, id, theme_command(theme)).await {
                    tracing::warn!("computer {id}'s desktop did not change its look: {e:#}");
                }
            }
        }
        Ok(json!({ "theme": theme }))
    }

    /// After a wake: the look it should have, which may have changed while
    /// it slept.
    pub(super) async fn apply_desktop_theme(&self, socket: &Path, id: &str) {
        let Some(theme) = self.read(|s| s.computers.get(id).map(|c| c.theme)) else { return };
        if let Err(e) = run(socket, id, theme_command(theme)).await {
            tracing::warn!("computer {id}'s desktop did not take its look: {e:#}");
        }
    }

    /// An installed app, put in the desktop's menu.
    pub(super) async fn add_app_to_menu(&self, socket: &Path, id: &str, name: &str) {
        if let Err(e) = run(socket, id, app_command(name)).await {
            tracing::warn!("computer {id} could not put {name} in its menu: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_commands_name_the_look_and_quote_the_app() {
        assert!(theme_command(Theme::Light).ends_with("magnus-desktop theme light"));
        assert!(theme_command(Theme::Dark).ends_with("magnus-desktop theme dark"));
        assert!(app_command("it's; rm -rf /").ends_with("app-installed 'it'\\''s; rm -rf /'"));
    }
}
