//! The agent's own computer beyond its screen: a shell as root on the
//! computer's writable system, and installing an app so a rebuilt computer
//! gets it back (spec, the app manifest: "Every app the agent installed:
//! name, version, source, hash, install steps"). Inside the computer is not
//! a boundary action, so the shell passes without review; its network goes
//! through the egress proxy like everything on the computer. An install is
//! a boundary action and is reviewed as one. The per-run sandbox stays as it
//! is for runs and checks (decisions/2026-11-08-agent-computer-rulings.md).

use super::apps::NewApp;
use super::{client, Owner};
use crate::kernel::Kernel;
use crate::tools::obj;
use anyhow::{anyhow, Result};
use magnus_adapter_proto::ToolSpec;
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) const EXEC_TOOL: &str = "computer_exec";
pub(crate) const INSTALL_TOOL: &str = "computer_install_app";
const EXEC_DEFAULT_SECONDS: u64 = 120;
const EXEC_MAX_SECONDS: u64 = 1800;
/// Beyond the command's own limit, for the reply to come back.
const EXEC_REPLY_MARGIN_SECONDS: u64 = 15;
const SECONDS_PER_MINUTE: u64 = 60;

type Done = (String, bool, Option<Value>);

/// Whether `tool` is one of these, not a desktop action.
pub(crate) fn handles(tool: &str) -> bool {
    tool == EXEC_TOOL || tool == INSTALL_TOOL
}

pub(crate) fn specs(d: &dyn Fn(&str) -> String) -> Vec<ToolSpec> {
    let text = json!({ "type": "string" });
    vec![
        ToolSpec { name: EXEC_TOOL.into(), description: d(EXEC_TOOL), input_schema: obj(json!({ "command": text, "timeout_s": { "type": "integer" } }), &["command"]) },
        ToolSpec {
            name: INSTALL_TOOL.into(),
            description: d(INSTALL_TOOL),
            input_schema: obj(
                json!({
                    "name": text, "version": text,
                    "source": { "type": "string", "description": "the https address the installer was downloaded from" },
                    "installer": { "type": "string", "description": "where the downloaded installer is on your computer" },
                    "steps": { "type": "array", "items": text, "description": "shell commands that install it; refer to the installer as $INSTALLER" },
                    "hosts": { "type": "array", "items": text, "description": "hosts the app itself needs to reach, if any" }
                }),
                &["name", "version", "source", "installer", "steps"],
            ),
        },
    ]
}

/// The command's limit in seconds: the default when none is given.
pub(crate) fn exec_seconds(input: &Value) -> u64 {
    input["timeout_s"].as_u64().unwrap_or(EXEC_DEFAULT_SECONDS).clamp(1, EXEC_MAX_SECONDS)
}

/// A command's result as the agent reads it, and whether it failed.
fn shown(v: &Value) -> (String, bool) {
    if let Some(e) = v["error"].as_str() {
        return (e.to_string(), true);
    }
    let mut text = format!("exit code: {}\n", v["exit_code"]);
    if v["timed_out"] == true {
        text.push_str("(timed out and was killed)\n");
    }
    text.push_str(&format!("stdout:\n{}\nstderr:\n{}", v["stdout"].as_str().unwrap_or_default(), v["stderr"].as_str().unwrap_or_default()));
    (text, v["exit_code"] != json!(0))
}

/// The steps, run with the installer the agent downloaded as `$INSTALLER`,
/// the name replay gives the cached copy.
fn step_command(installer: &str, step: &str) -> String {
    format!("INSTALLER='{}'; export INSTALLER; {step}", installer.replace('\'', r"'\''"))
}

impl Kernel {
    /// The shell or an install on `owner`'s computer, made ready first as
    /// the desktop tools make it. `None` on a server without computers.
    pub(crate) async fn agent_computer_tool(&self, owner: &Owner, actor: &str, tool: &str, input: &Value) -> Result<Option<Done>> {
        let id = match self.computer_for(owner).await {
            Ok(Some(id)) => id,
            Ok(None) => return Ok(None),
            Err(e) => return Ok(Some((format!("Your computer could not be made ready: {e}"), true, None))),
        };
        if self.read(|s| s.computers.get(&id).is_some_and(|c| c.controller.is_some())) {
            return Ok(Some(("A person has control of this computer, so this was refused. Wait until they hand it back.".to_string(), true, None)));
        }
        let room = Duration::from_secs(super::wake::ROOM_WAIT_MINUTES * SECONDS_PER_MINUTE);
        if let Err(e) = self.wake_in_turn(&id, actor, room).await {
            return Ok(Some((format!("Your computer could not wake: {e}"), true, None)));
        }
        super::idle::touch(&id);
        let socket = self.computer_socket().map_err(|e| anyhow!("{e}"))?;
        if tool == EXEC_TOOL {
            let command = input["command"].as_str().unwrap_or_default();
            let seconds = exec_seconds(input);
            let v = client::guest_for(&socket, &id, json!({ "op": "exec", "command": command, "timeout_s": seconds }), Duration::from_secs(seconds + EXEC_REPLY_MARGIN_SECONDS))
                .await?;
            let (text, failed) = shown(&v);
            return Ok(Some((text, failed, Some(v))));
        }
        let new: NewApp = match serde_json::from_value(input.clone()) {
            Ok(n) => n,
            Err(e) => return Ok(Some((format!("The app was not installed: {e}"), true, None))),
        };
        for step in new.steps() {
            let v = client::guest_for(
                &socket,
                &id,
                json!({ "op": "exec", "command": step_command(new.installer(), step), "timeout_s": EXEC_MAX_SECONDS }),
                Duration::from_secs(EXEC_MAX_SECONDS + EXEC_REPLY_MARGIN_SECONDS),
            )
            .await?;
            let (text, failed) = shown(&v);
            if failed {
                return Ok(Some((format!("The step `{step}` failed, so the app was not recorded.\n{text}"), true, None)));
            }
        }
        let name = new.name().to_string();
        Ok(Some(match self.record_app(actor, &id, new).await {
            Ok(v) => (format!("Installed {name} and added it to your computer's apps, so it comes back if the computer is rebuilt."), false, Some(v)),
            Err(e) => (format!("{name} was installed but not recorded: {e}"), true, None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_has_a_shell_and_an_install_on_its_computer() {
        let names: Vec<String> = specs(&|n: &str| n.to_string()).into_iter().map(|s| s.name).collect();
        assert_eq!(names, [EXEC_TOOL, INSTALL_TOOL]);
        assert!(handles(EXEC_TOOL) && handles(INSTALL_TOOL) && !handles("computer_click"));
    }

    #[test]
    fn a_commands_limit_is_bounded() {
        assert_eq!(exec_seconds(&json!({})), EXEC_DEFAULT_SECONDS);
        assert_eq!(exec_seconds(&json!({ "timeout_s": 0 })), 1);
        assert_eq!(exec_seconds(&json!({ "timeout_s": 99999 })), EXEC_MAX_SECONDS);
    }

    #[test]
    fn an_install_step_names_the_installer_as_replay_does() {
        assert_eq!(step_command("/tmp/a b.deb", "dpkg -i \"$INSTALLER\""), "INSTALLER='/tmp/a b.deb'; export INSTALLER; dpkg -i \"$INSTALLER\"");
        assert_eq!(step_command("/tmp/it's", "x"), r"INSTALLER='/tmp/it'\''s'; export INSTALLER; x");
    }

    #[test]
    fn a_failed_command_says_so() {
        assert!(shown(&json!({ "exit_code": 1, "stdout": "", "stderr": "no" })).1);
        assert!(!shown(&json!({ "exit_code": 0, "stdout": "ok", "stderr": "" })).1);
        assert!(shown(&json!({ "error": "the computer is not running" })).1);
    }
}
