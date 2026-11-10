//! Settings in `%APPDATA%\TapOnLights\settings.json`. Unknown keys are preserved.
//! On first run, settings from the app's former name ("Tapo Desktop") are copied over.

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
        Self {
            enabled: false,
            port: DEFAULT_WEB_PORT,
            lan: false,
            password_hash: None,
        }
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
    /// The user confirmed the one-time warning that effects are experimental.
    pub effects_acknowledged: bool,
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
            effects_acknowledged: false,
            extra: serde_json::Map::new(),
        }
    }
}

const APP_DIR: &str = "TapOnLights";
const LEGACY_APP_DIR: &str = "Tapo Desktop";

fn path_in(dir: &str) -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    root.join(dir).join("settings.json")
}

fn path() -> PathBuf {
    path_in(APP_DIR)
}

pub fn load() -> Settings {
    let path = path();
    let raw = std::fs::read_to_string(&path).ok().or_else(|| {
        // One-time migration from the former app name; the old file is left in place.
        let legacy = std::fs::read_to_string(path_in(LEGACY_APP_DIR)).ok()?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, &legacy);
        Some(legacy)
    });
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create settings folder: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("Could not write settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Could not save settings: {e}"))
}
