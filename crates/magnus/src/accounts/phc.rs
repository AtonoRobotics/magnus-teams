//! The two rules every new password meets, in one place: it is long
//! enough, and it is stored as an Argon2id hash with a fresh salt.

use anyhow::Result;
use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
use argon2::Argon2;

/// Refuse a password shorter than `min` characters (characters, not
/// bytes). Every path that sets a password judges it here, before it
/// touches anything, so a short one spends no invitation, recovery or
/// attempt.
pub(super) fn long_enough(password: &str, min: usize) -> Result<()> {
    anyhow::ensure!(password.chars().count() >= min, "a password needs at least {min} characters");
    Ok(())
}

/// The PHC string to store for `password`: Argon2id, default parameters,
/// a fresh salt each time.
pub(super) fn hash(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt).map_err(|e| anyhow::anyhow!("hash: {e}"))?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::{PasswordHash, PasswordVerifier};

    #[test]
    fn a_password_is_long_enough_from_the_minimum_counted_in_characters() {
        assert!(long_enough("123456789", 10).is_err(), "one short passed");
        assert!(long_enough("1234567890", 10).is_ok(), "exactly the minimum was refused");
        // Ten characters, twenty bytes: characters count.
        assert!(long_enough("éééééééééé", 10).is_ok(), "bytes were counted, not characters");
        assert!(long_enough("ééééééééé", 10).is_err(), "nine characters of two bytes passed");
        let said = long_enough("short", 10).unwrap_err().to_string();
        assert_eq!(said, "a password needs at least 10 characters", "the refusal's wording changed");
    }

    #[test]
    fn a_hash_is_argon2id_salted_and_verifies() {
        let one = hash("a-long-password").unwrap();
        let two = hash("a-long-password").unwrap();
        assert!(one.starts_with("$argon2id$"), "not Argon2id: {one}");
        assert_ne!(one, two, "two hashes of one password share a salt");
        let parsed = PasswordHash::new(&one).unwrap();
        assert!(Argon2::default().verify_password(b"a-long-password", &parsed).is_ok(), "the hash does not verify its password");
        assert!(Argon2::default().verify_password(b"another-password", &parsed).is_err(), "the hash verifies another password");
    }
}
