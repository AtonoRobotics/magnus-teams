//! A person's role on the server (spec rev 141, People; spec, Roles): owner,
//! admin, member or guest. A guest reads and chats only. The role is kept as
//! the person's roles always were, by `RolesSet`, beside any other role they
//! hold (developer, say); an owner or admin is also an admin in the server's
//! own directory, so every check that reads either agrees.

use crate::accounts::Role as Directory;
use crate::config::Person;
use crate::events::Ev;
use crate::kernel::Kernel;
use axum::http::StatusCode;

/// The four roles, most trusted first.
pub const ROLES: [&str; 4] = ["owner", "admin", "member", "guest"];

/// What a guest's refused boundary action names as its rule.
pub const GUEST_RULE: &str = "kernel:guest_reads_and_chats_only";

pub type Refusal = (StatusCode, &'static str, String);

/// Whether `p` is a guest: the guest role and no role above it.
pub fn is_guest(p: &Person) -> bool {
    p.has_role("guest") && !p.is_owner() && !p.has_role("admin")
}

/// The one role of the four a person holds, the highest when several.
pub fn role_of(p: &Person, directory: Directory) -> &'static str {
    if p.is_owner() {
        "owner"
    } else if p.has_role("admin") || directory == Directory::Admin {
        "admin"
    } else if p.has_role("guest") {
        "guest"
    } else {
        "member"
    }
}

/// `roles` with its one role of the four replaced by `role`, the rest kept.
pub fn with_role(roles: &[String], role: &str) -> Vec<String> {
    let mut out: Vec<String> = roles.iter().filter(|r| !ROLES.contains(&r.as_str())).cloned().collect();
    out.push(role.to_string());
    out
}

impl Kernel {
    /// Everyone the server keeps: the config's people and its own.
    pub fn everyone(&self) -> Vec<String> {
        let mut ids: std::collections::BTreeSet<String> = self.cfg.people.iter().map(|p| p.id.clone()).collect();
        ids.extend(self.accounts.local_people().into_iter().map(|(id, _)| id));
        ids.into_iter().collect()
    }

    /// The owners who are turned on, by id.
    pub async fn active_owners(&self) -> Vec<String> {
        let mut out = vec![];
        for id in self.everyone() {
            if self.person(&id).is_some_and(|p| p.is_owner()) && self.accounts.status(&id).await != crate::accounts::Status::Disabled {
                out.push(id);
            }
        }
        out
    }

    /// Whether `id` is the last owner who is turned on: they cannot be
    /// removed, turned off or given another role.
    pub async fn last_owner(&self, id: &str) -> bool {
        let owners = self.active_owners().await;
        owners.len() == 1 && owners[0] == id
    }

    /// Give `id` one of the four roles, as `actor`. Held under the decision
    /// lock, so two owners demoting each other at once leave one.
    pub async fn set_server_role(&self, actor: &str, id: &str, role: &str) -> Result<(), Refusal> {
        if !ROLES.contains(&role) {
            return Err((StatusCode::BAD_REQUEST, "invalid", "The role is owner, admin, member or guest.".into()));
        }
        let Some(p) = self.person(id) else { return Err((StatusCode::NOT_FOUND, "not_found", "No such person.".into())) };
        if self.read(|s| s.provider_rights.get(id).is_some_and(|(_, r)| r.is_some())) {
            return Err((StatusCode::CONFLICT, "provider", "The identity provider names this person's roles: change them there.".into()));
        }
        let _decide = self.decide.lock().await;
        if p.is_owner() && role != "owner" && self.last_owner(id).await {
            return Err((StatusCode::CONFLICT, "last_owner", "This is the last owner: make someone else an owner first.".into()));
        }
        let admin = matches!(role, "owner" | "admin");
        let now = self.accounts.role(id).await;
        if admin != (now == Directory::Admin) {
            if let Err(e) = self.accounts.set_role(id, if admin { Directory::Admin } else { Directory::Member }).await {
                let m = format!("{e:#}");
                if m.contains("last admin") {
                    return Err((StatusCode::CONFLICT, "last_admin", "This is the last admin: make someone else an admin first.".into()));
                }
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "failed", m));
            }
        }
        let roles = with_role(&p.roles, role);
        self.emit(actor, Ev::RolesSet { person: id.to_string(), roles }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, "failed", format!("{e:#}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(roles: &[&str]) -> Person {
        serde_json::from_value(serde_json::json!({ "id": "p", "roles": roles })).unwrap()
    }

    #[test]
    fn a_person_holds_one_of_the_four_and_keeps_the_rest() {
        assert_eq!(role_of(&person(&["owner", "guest"]), Directory::Member), "owner");
        assert_eq!(role_of(&person(&[]), Directory::Admin), "admin", "the directory's admin was not an admin");
        assert_eq!(role_of(&person(&["guest"]), Directory::Member), "guest");
        assert_eq!(role_of(&person(&["developer"]), Directory::Member), "member");
        assert_eq!(with_role(&["developer".into(), "admin".into()], "guest"), vec!["developer".to_string(), "guest".to_string()]);
        assert!(is_guest(&person(&["guest"])));
        assert!(!is_guest(&person(&["guest", "admin"])), "an admin was held to a guest's limits");
    }
}
