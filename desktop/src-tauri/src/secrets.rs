//! Tapo account password in Windows Credential Manager, readable by both this app
//! and the original Python app (python-keyring's WinVaultKeyring layout).

const SERVICE: &str = "Tapo Desktop";

pub fn tapo_password(username: &str) -> Option<String> {
    if username.is_empty() {
        return None;
    }
    // python-keyring stores under target "<service>", or "<user>@<service>" when a
    // second account was saved under the same service.
    [SERVICE.to_string(), format!("{username}@{SERVICE}")]
        .iter()
        .find_map(|target| {
            keyring::Entry::new_with_target(target, SERVICE, username)
                .ok()?
                .get_password()
                .ok()
        })
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
        .map(|parsed| argon2::Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}
