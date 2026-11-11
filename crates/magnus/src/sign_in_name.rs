//! The name a person signs in with: their username, or the email address
//! on their account. Invitations and recovery run on the address, and the
//! invitation's last page tells people to sign in with it, so the address
//! has to work wherever the username does.

use crate::accounts::{Accounts, LocalPerson};

/// The account `typed` names, lowercased: a username as itself, an email
/// address as the person holding it (the first by id, as recovery picks),
/// or, when nobody holds it, the address itself, which then fails as any
/// unknown name does. Usernames never hold '@', so the two never meet.
pub async fn resolve(accounts: &Accounts, typed: &str) -> String {
    let typed = typed.trim().to_lowercase();
    if !typed.contains('@') {
        return typed;
    }
    // Someone invited seconds ago resolves, as a username does (#466).
    if let Err(e) = accounts.refresh_people().await {
        tracing::warn!("sign-in by email: the directory did not refresh: {e:#}");
    }
    holder(&accounts.local_people(), &typed).unwrap_or(typed)
}

/// The first person by id whose address is `email`, ignoring case.
fn holder(people: &[(String, LocalPerson)], email: &str) -> Option<String> {
    people.iter().filter(|(_, p)| p.email.as_deref().is_some_and(|e| e.trim().eq_ignore_ascii_case(email))).map(|(id, _)| id.clone()).min()
}

#[cfg(test)]
mod tests {
    use super::holder;
    use crate::accounts::LocalPerson;

    fn person(id: &str, email: Option<&str>) -> (String, LocalPerson) {
        (id.into(), LocalPerson { name: id.into(), email: email.map(Into::into) })
    }

    /// An address finds its holder whatever its case, two holders resolve
    /// to the first by id as recovery does, and an unknown address finds
    /// nobody.
    #[test]
    fn an_address_names_the_person_holding_it() {
        let people =
            [person("zed", Some("shared@example.com")), person("hartenfelsj", Some("HartenfelsJ@Yahoo.com")), person("amy", Some("shared@example.com")), person("bo", None)];
        assert_eq!(holder(&people, "hartenfelsj@yahoo.com").as_deref(), Some("hartenfelsj"));
        assert_eq!(holder(&people, "shared@example.com").as_deref(), Some("amy"));
        assert_eq!(holder(&people, "nobody@example.com"), None);
    }
}
