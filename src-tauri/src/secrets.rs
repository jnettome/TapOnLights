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
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| format!("Could not hash the web password: {e}"))
}

pub fn verify_web_password(password: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(hash)
        .map(|parsed| {
            argon2::Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}
