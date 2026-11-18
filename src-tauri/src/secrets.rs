//! Tapo account password in Windows Credential Manager (generic credential,
//! target "TapOnLights"), plus hashing for the optional web-page password.

const SERVICE: &str = "TapOnLights";
/// The app's former name. Its credential layout matches python-keyring's
/// WinVaultKeyring: target "<service>", or "<user>@<service>" for a second account.
const LEGACY_SERVICE: &str = "Tapo Desktop";

fn read(target: &str, service: &str, username: &str) -> Option<String> {
    keyring::Entry::new_with_target(target, service, username)
        .ok()?
        .get_password()
        .ok()
}

pub fn tapo_password(username: &str) -> Option<String> {
    if username.is_empty() {
        return None;
    }
    if let Some(password) = read(SERVICE, SERVICE, username) {
        return Some(password);
    }
    // One-time migration from the former name; the old entry is left in place.
    let legacy = [
        LEGACY_SERVICE.to_string(),
        format!("{username}@{LEGACY_SERVICE}"),
    ]
    .iter()
    .find_map(|target| read(target, LEGACY_SERVICE, username))?;
    let _ = set_tapo_password(username, &legacy);
    Some(legacy)
}

pub fn set_tapo_password(username: &str, password: &str) -> Result<(), String> {
    keyring::Entry::new_with_target(SERVICE, SERVICE, username)
        .and_then(|entry| entry.set_password(password))
        .map_err(|e| format!("Could not save the password to Windows Credential Manager: {e}"))
}

pub fn hash_web_password(password: &str) -> Result<String, String> {
    use argon2::{PasswordHash, PasswordHasher};
    // A random salt comes from the OS (argon2's default `getrandom` feature).
    PasswordHasher::<PasswordHash>::hash_password(&argon2::Argon2::default(), password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| format!("Could not hash the web password: {e}"))
}

/// Check a password against a stored PHC string. The algorithm, version and cost
/// come from the string, so hashes saved by earlier versions keep working.
pub fn verify_web_password(password: &str, hash: &str) -> bool {
    use argon2::PasswordVerifier;
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), hash)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_passwords_round_trip() {
        let hash = hash_web_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$"), "{hash}");
        assert!(verify_web_password("correct horse", &hash));
        assert!(!verify_web_password("wrong", &hash));
        assert_ne!(
            hash,
            hash_web_password("correct horse").unwrap(),
            "fresh salt each time"
        );
    }

    #[test]
    fn verifies_hashes_with_other_parameters() {
        // Reference Argon2id vector (64 MiB, t=2): stored hashes keep their own cost settings.
        let hash = "$argon2id$v=19$m=65536,t=2,p=1$c29tZXNhbHQ$CTFhFdXPJO1aFaMaO6Mm5c8y7cJHAph8ArZWb2GRPPc";
        assert!(verify_web_password("password", hash));
        assert!(!verify_web_password("Password", hash));
        assert!(!verify_web_password("password", "not a hash"));
    }
}
