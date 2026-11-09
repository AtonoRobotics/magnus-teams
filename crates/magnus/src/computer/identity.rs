//! Browser VMs: one per identity, and a throwaway per run for untrusted
//! reading. The agent reaches them only through the broker here, by commands
//! and an accessibility snapshot; the guest serves nothing else.
use super::egress_surrogate as surrogate;
use super::service::{failed, new_id, wait_for_guest};
use super::{client, Change, Computer, KillMode, Owner, Role, Status};
use crate::config::Person;
use crate::controls::Refused;
use crate::events::Ev;
use crate::kernel::Kernel;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// A page can take this long: a slow site after a cold browser start.
const BROKER_WAIT: Duration = Duration::from_secs(120);
const BROWSER_IDLE: Duration = Duration::from_secs(10 * 60);
const SEAL_KEY_BYTES: usize = 32;
const MAX_FORM_VALUE_BYTES: usize = 8 * 1024;
const SURROGATE_RANDOM_BYTES: usize = 32;
const THROWAWAY_PAGE_NOTE: &str = "(read in a throwaway browser with no login)";

/// One VM boot at a time: two tool calls must not start the same VM twice.
static BOOT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn seal_key_name(id: &str) -> String {
    format!("browser-profile/{id}")
}
fn password_name(id: &str) -> String {
    format!("identity/{id}/password")
}
fn username_name(id: &str) -> String {
    format!("identity/{id}/username")
}

/// `Debug` is deliberately not derived: this holds the secret itself.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecureForm {
    target: String,
    value: String,
    #[serde(default)]
    submit: bool,
    #[serde(default)]
    save: bool,
    #[serde(default)]
    username: Option<String>,
}

impl Kernel {
    fn profile_key(&self, id: &str) -> Result<String> {
        self.vault.get(&seal_key_name(id))?.map(|s| s.expose().to_string()).ok_or_else(|| anyhow!("the profile's key is not in the vault"))
    }

    async fn new_browser(&self, actor: &str, person: &str, role: Role, extra: Vec<Change>) -> Result<String> {
        let socket = self.computer_socket().map_err(|e| anyhow!("{e:?}"))?;
        let id = new_id();
        client::ask(&socket, json!({ "op": "create", "id": id })).await?;
        let owner = Owner::Person(person.to_string());
        let mut changes = vec![Change::BrowserCreated { id: id.clone(), owner, role }];
        changes.extend(extra);
        self.emit_all(changes.into_iter().map(|c| (actor, Ev::Computer(c))).collect()).await?;
        Ok(id)
    }

    /// One account at one site. Its VM reaches that site and what its pages
    /// load, and its profile lives only there.
    pub async fn create_identity(&self, who: &Person, site: &str) -> Result<Value, Refused> {
        let host = super::service::checked_host(site)?;
        if host.starts_with("*.") {
            return Err(Refused::Invalid("an identity is for one site".into()));
        }
        use rand::RngCore;
        let mut raw = [0u8; SEAL_KEY_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let id_key = base64::engine::general_purpose::STANDARD.encode(raw);
        let actor = format!("person:{}", who.id);
        // The key first: a browser with no key could never be sealed.
        let pending = new_id();
        self.vault.put(&seal_key_name(&pending), &id_key).map_err(failed)?;
        let id = self.new_browser(&actor, &who.id, Role::Identity { site: host.clone() }, vec![]).await.map_err(failed)?;
        self.vault.put(&seal_key_name(&id), &id_key).map_err(failed)?;
        let _ = self.vault.remove(&seal_key_name(&pending));
        let changes = vec![
            Change::DestinationAdded { id: id.clone(), host: host.clone() },
            Change::DestinationAdded { id: id.clone(), host: format!("*.{host}") },
            Change::PageOpened { id: id.clone(), page: "site".into(), host: host.clone() },
        ];
        self.emit_all(changes.into_iter().map(|c| (actor.as_str(), Ev::Computer(c))).collect()).await.map_err(failed)?;
        Ok(json!({ "id": id, "site": host }))
    }

    pub fn identities_of(&self, who: &Person) -> Value {
        let mine = Owner::Person(who.id.clone());
        let shown = self.read(|s| {
            s.computers
                .all()
                .filter(|c| c.owner == mine)
                .filter_map(|c| match &c.role {
                    Role::Identity { site } => Some(json!({ "id": c.id, "site": site, "status": c.status })),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        json!({ "identities": shown })
    }

    /// The person sees "no such identity" for one that is not theirs.
    fn own_identity(&self, person: &str, id: &str) -> Result<Computer, Refused> {
        self.read(|s| s.computers.get(id).cloned())
            .filter(|c| matches!(c.role, Role::Identity { .. }) && c.owner == Owner::Person(person.to_string()))
            .ok_or(Refused::NotFound("identity"))
    }

    pub async fn delete_identity(&self, who: &Person, id: &str) -> Result<Value, Refused> {
        let socket = self.computer_socket()?;
        self.own_identity(&who.id, id)?;
        self.record_computer(who, Change::Killed { id: id.into(), mode: KillMode::Destroy, delete_volume: true }).await?;
        client::ask(&socket, json!({ "op": "destroy", "id": id, "delete_volume": true })).await.map_err(failed)?;
        let _ = self.vault.remove(&seal_key_name(id));
        let _ = self.vault.remove(&password_name(id));
        let _ = self.vault.remove(&username_name(id));
        Ok(json!({ "deleted": id }))
    }

    /// The VM, running: opens the profile and boots it if it is not.
    async fn running_browser(&self, c: &Computer) -> Result<()> {
        let socket = self.computer_socket().map_err(|e| anyhow!("{e:?}"))?;
        let _one_boot = BOOT.lock().await;
        super::idle::touch(&c.id);
        if client::ask(&socket, json!({ "op": "status", "id": c.id })).await?["state"] == "running" {
            return Ok(());
        }
        if matches!(c.role, Role::Identity { .. }) {
            client::ask(&socket, json!({ "op": "unseal_volume", "id": c.id, "key_b64": self.profile_key(&c.id)? })).await?;
        }
        client::ask(&socket, json!({ "op": "start_browser", "id": c.id })).await.context("start the browser VM")?;
        wait_for_guest(&socket, &c.id).await?;
        self.emit("kernel", Ev::Computer(Change::Started { id: c.id.clone() })).await?;
        Ok(())
    }

    /// Stops a browser VM. An identity's profile goes back under its key.
    async fn stop_browser(&self, socket: &Path, c: &Computer) -> Result<()> {
        let _ = client::guest(socket, &c.id, json!({ "op": "sync" })).await;
        client::ask(socket, json!({ "op": "stop", "id": c.id })).await?;
        self.emit("kernel", Ev::Computer(Change::BrowserStopped { id: c.id.clone() })).await?;
        self.seal_browser(socket, c).await
    }

    async fn seal_browser(&self, socket: &Path, c: &Computer) -> Result<()> {
        if matches!(c.role, Role::Identity { .. }) {
            client::ask(socket, json!({ "op": "seal_volume", "id": c.id, "key_b64": self.profile_key(&c.id)? })).await?;
        }
        Ok(())
    }

    /// Stops unused browser VMs, and seals any profile a stop left open: a
    /// kill through computerd happens without Magnus.
    pub(super) async fn tend_browsers(&self, socket: &Path) {
        let browsers: Vec<Computer> = self.read(|s| s.computers.all().filter(|c| c.role != Role::Computer).cloned().collect());
        for c in browsers {
            let tended = async {
                // Not while a tool is opening this profile to boot it.
                let _no_boot = BOOT.lock().await;
                let live = client::ask(socket, json!({ "op": "status", "id": c.id })).await?["state"] == "running";
                if live && super::idle::idle_browser(&c.id, &c.role, BROWSER_IDLE) {
                    self.stop_browser(socket, &c).await?;
                } else if !live {
                    if c.status == Status::Running {
                        self.emit("kernel", Ev::Computer(Change::BrowserStopped { id: c.id.clone() })).await?;
                    }
                    self.seal_browser(socket, &c).await?;
                }
                Ok::<(), anyhow::Error>(())
            };
            if let Err(e) = tended.await {
                tracing::warn!("browser VM {}: {e:#}", c.id);
            }
        }
    }

    /// The run's own browser, made the first time it reads the web.
    async fn throwaway_for(&self, run: &str, person: &str) -> Result<Computer> {
        let found = |k: &Kernel| k.read(|s| s.computers.all().find(|c| matches!(&c.role, Role::Throwaway { run: r } if r == run)).cloned());
        if let Some(c) = found(self) {
            return Ok(c);
        }
        {
            let _one_boot = BOOT.lock().await;
            if found(self).is_none() {
                self.new_browser("kernel", person, Role::Throwaway { run: run.to_string() }, vec![]).await?;
            }
        }
        found(self).context("the throwaway browser was not recorded")
    }

    /// A run's end takes its browsers with it: the host's and any throwaway.
    pub async fn close_run_browsers(&self, run: &str) {
        self.browser.close(run).await;
        let Some(c) = self.read(|s| s.computers.all().find(|c| matches!(&c.role, Role::Throwaway { run: r } if r == run)).cloned()) else { return };
        let Some(socket) = self.cfg.computer.as_ref().map(|s| s.socket.clone()) else { return };
        let gone = async {
            self.emit("kernel", Ev::Computer(Change::Killed { id: c.id.clone(), mode: KillMode::Destroy, delete_volume: true })).await?;
            client::ask(&socket, json!({ "op": "destroy", "id": c.id, "delete_volume": true })).await
        };
        if let Err(e) = gone.await {
            tracing::warn!("the throwaway browser of {run} was not removed: {e:#}");
        }
    }

    async fn broker(&self, id: &str, req: Value) -> Result<Value> {
        let socket = self.computer_socket().map_err(|e| anyhow!("{e:?}"))?;
        let reply = client::guest_for(&socket, id, req, BROKER_WAIT).await?;
        match reply["error"].as_str() {
            Some(error) => Err(anyhow!("{error}")),
            None => Ok(reply),
        }
    }

    /// A browser tool: in a VM here, or the host's jail with no computers.
    pub(crate) async fn browser_tool(&self, a: &crate::boundary::Action) -> Result<(String, bool, Option<Value>)> {
        if let Some(done) = self.browser_vm_tool(&a.run, &a.tool, &a.input).await? {
            return Ok(done);
        }
        let mut req = a.input.clone();
        req["op"] = json!(a.tool.trim_start_matches("browser_"));
        let v = self.browser.request(&a.run, req).await?;
        let text = v["text"].as_str().unwrap_or_default().to_string();
        let data = v["image_b64"].as_str().map(|i| json!({ "image_b64": i }));
        Ok((text, false, data))
    }

    /// `browser_*` in a throwaway VM, `browser_as_*` in the named identity's.
    /// `None` when this appliance has no computers: the host's jail is used.
    async fn browser_vm_tool(&self, run: &str, tool: &str, input: &Value) -> Result<Option<(String, bool, Option<Value>)>> {
        if self.cfg.computer.is_none() {
            return Ok(None);
        }
        let person = self.read(|s| s.runs.get(run).map(|r| r.person.clone())).context("no such run")?;
        if tool == "browser_sign_in" {
            return self.sign_in(run, &person, input).await.map(Some);
        }
        let (c, op, throwaway) = match tool.strip_prefix("browser_as_") {
            Some(op) => {
                let id = input["identity"].as_str().unwrap_or_default();
                (self.own_identity(&person, id).map_err(|e| anyhow!("{e:?}"))?, op, false)
            }
            None => (self.throwaway_for(run, &person).await?, tool.trim_start_matches("browser_"), true),
        };
        self.running_browser(&c).await?;
        let mut req = input.clone();
        if let Some(fields) = req.as_object_mut() {
            fields.remove("identity");
        }
        req["op"] = json!(op);
        let v = self.broker(&c.id, req).await?;
        let mut text = v["text"].as_str().unwrap_or_default().to_string();
        if throwaway && op == "navigate" {
            text = format!("{THROWAWAY_PAGE_NOTE}\n{text}");
        }
        let data = v["image_b64"].as_str().map(|i| json!({ "image_b64": i }));
        Ok(Some((text, false, data)))
    }

    /// The vault first, then the person through a secure form. A password
    /// manager connection would go between them; none exists yet.
    async fn sign_in(&self, run: &str, person: &str, input: &Value) -> Result<(String, bool, Option<Value>)> {
        let id = input["identity"].as_str().unwrap_or_default();
        let c = self.own_identity(person, id).map_err(|e| anyhow!("{e:?}"))?;
        let password_field = input["password_field"].as_str().context("password_field is required")?;
        self.running_browser(&c).await?;
        if let Some(password) = self.vault.get(&password_name(id))? {
            if let (Some(field), Some(user)) = (input["username_field"].as_str(), self.vault.get(&username_name(id))?) {
                self.broker(id, json!({ "op": "fill_secret", "target": field, "value": user.expose() })).await?;
            }
            self.broker(id, json!({ "op": "fill_secret", "target": password_field, "value": password.expose(), "submit": true })).await?;
            return Ok(("Signed in with the saved login.".to_string(), false, None));
        }
        let task = self.read(|s| s.runs.get(run).map(|r| r.task.clone())).unwrap_or_default();
        let item = crate::kernel::new_id("item");
        let summary = "Sign in through the secure form so the agent can continue.".to_string();
        let details = json!({ "identity": id, "site": site_of(&c), "password_field": password_field, "username_field": input["username_field"] });
        self.emit(
            "kernel",
            Ev::Item { id: item.clone(), item_kind: "secure_form".into(), task: Some(task), run: Some(run.to_string()), person: person.to_string(), summary, details },
        )
        .await?;
        Ok((
            format!(
                "No saved login. The person has been asked to sign in through a secure form (item {item}); the agent never sees the value. Check the page again after they have."
            ),
            false,
            None,
        ))
    }

    /// From a person's own form: the value goes to the page, and to the
    /// vault if they say so, and is never logged or returned.
    pub async fn fill_secure_form(&self, who: &Person, id: &str, form: &SecureForm) -> Result<Value, Refused> {
        let SecureForm { target, value, submit, save, username } = form;
        let (submit, save) = (*submit, *save);
        if value.len() > MAX_FORM_VALUE_BYTES {
            return Err(Refused::Invalid("that value is too long".into()));
        }
        let c = self.own_identity(&who.id, id)?;
        self.running_browser(&c).await.map_err(failed)?;
        // Written before the fill, so a fill always has its record.
        self.record_computer(who, Change::SecureFormFilled { id: id.into(), by: who.id.clone(), saved: save, at: Utc::now() }).await?;
        self.broker(id, json!({ "op": "fill_secret", "target": target, "value": value, "submit": submit }))
            .await
            .map_err(|_| Refused::Conflict("the page could not be filled".into()))?;
        if save {
            self.vault.put(&password_name(id), value).map_err(failed)?;
            if let Some(u) = username.as_deref() {
                self.vault.put(&username_name(id), u).map_err(failed)?;
            }
        }
        Ok(json!({ "filled": true }))
    }

    /// A credential for one host. The real value goes to the vault; the
    /// computer gets a token that means nothing anywhere else.
    pub async fn issue_surrogate(&self, who: &Person, id: &str, name: &str, value: &str, host: &str) -> Result<Value, Refused> {
        self.controlled_computer(who, id)?;
        let host = super::service::checked_host(host)?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(Refused::Invalid("a credential's name is letters, digits, - and _".into()));
        }
        use rand::RngCore;
        let mut raw = [0u8; SURROGATE_RANDOM_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let token = format!("{}{}", surrogate::PREFIX, hex::encode(raw));
        self.vault.put(&surrogate::vault_name(id, name), value).map_err(failed)?;
        self.record_computer(who, Change::SurrogateIssued { id: id.into(), hash: surrogate::hash(&token), name: name.to_string(), host }).await?;
        Ok(json!({ "surrogate": token }))
    }
}

fn site_of(c: &Computer) -> &str {
    match &c.role {
        Role::Identity { site } => site,
        _ => "",
    }
}
