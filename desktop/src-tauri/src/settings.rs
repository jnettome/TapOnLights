//! Settings file shared with the original Python app:
//! `%APPDATA%\Tapo Desktop\settings.json`. Unknown keys are preserved.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_WEB_PORT: u16 = 4530;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceEntry {
    pub ip: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub room: String,
}

/// Same shape the Python app stored: `temperature` (white) or `hsv` (color), plus brightness.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsv: Option<[u16; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WebSettings {
    pub enabled: bool,
    pub port: u16,
    /// Listen on all interfaces (other devices on the network) instead of 127.0.0.1 only.
    pub lan: bool,
    /// Argon2 PHC string. `None` means no password.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_WEB_PORT, lan: false, password_hash: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub devices: Vec<DeviceEntry>,
    pub username: String,
    /// "system", "light" or "dark".
    pub theme: String,
    pub start_minimized: bool,
    pub launch_on_startup: bool,
    pub custom_scenes: BTreeMap<String, Scene>,
    pub web: WebSettings,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            username: String::new(),
            theme: "system".into(),
            start_minimized: false,
            launch_on_startup: false,
            custom_scenes: BTreeMap::new(),
            web: WebSettings::default(),
            extra: serde_json::Map::new(),
        }
    }
}

fn path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir());
    root.join("Tapo Desktop").join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create settings folder: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("Could not write settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Could not save settings: {e}"))
}
