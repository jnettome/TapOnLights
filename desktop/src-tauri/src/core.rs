//! Operations shared by the desktop UI (Tauri commands) and the web server.

use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _, Theme};

use crate::bulbs::{BulbView, HueSat, Manager, Patch, TEMPERATURE_RANGE};
use crate::secrets;
use crate::server::{WebServer, WebStatus};
use crate::settings::{self, DeviceEntry, Scene, Settings};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub bulbs: Arc<Manager>,
    pub web: WebServer,
    /// When the popup was last hidden (ms since start), to ignore the tray click that hid it.
    pub last_hidden_ms: AtomicU64,
    /// Incremented on every show; stops the previous show's auto-hide watcher.
    pub popup_generation: AtomicU64,
    pub started: std::time::Instant,
    /// Windows 11 backdrop (Mica) available; otherwise the UI paints an opaque background.
    pub backdrop: bool,
}

pub fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn credentials_for(settings: &Settings) -> Option<(String, String)> {
    let user = settings.username.trim();
    secrets::tapo_password(user).map(|password| (user.to_string(), password))
}

fn builtin_scenes() -> Vec<(&'static str, Scene)> {
    let white = |temperature, brightness| Scene { temperature: Some(temperature), hsv: None, brightness: Some(brightness) };
    let color = |h, s, brightness| Scene { temperature: None, hsv: Some([h, s, brightness as u16]), brightness: Some(brightness) };
    vec![
        ("Focus", white(4200, 95)),
        ("Reading", white(3000, 88)),
        ("Relax", white(2800, 55)),
        ("Night", white(2500, 12)),
        ("Sunset", color(20, 85, 48)),
        ("Party", color(285, 100, 80)),
    ]
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneView {
    pub name: String,
    pub builtin: bool,
    pub brightness: Option<u8>,
    pub temperature: Option<u16>,
    pub color: Option<HueSat>,
}

fn scene_patch(scene: &Scene) -> Patch {
    let color = scene.hsv.map(|[hue, saturation, _]| HueSat { hue, saturation: saturation.clamp(1, 100) as u8 });
    Patch {
        on: Some(true),
        brightness: scene.brightness.or(scene.hsv.map(|hsv| hsv[2].clamp(1, 100) as u8)),
        temperature: if color.is_none() { scene.temperature } else { None },
        color,
    }
}

fn scene_view(name: &str, scene: &Scene, builtin: bool) -> SceneView {
    let patch = scene_patch(scene);
    SceneView {
        name: name.to_string(),
        builtin,
        brightness: patch.brightness,
        temperature: patch.temperature,
        color: patch.color,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub bulbs: Vec<BulbView>,
    pub scenes: Vec<SceneView>,
    pub theme: String,
    pub web: WebStatus,
    pub has_account: bool,
    pub temperature_range: (u16, u16),
    pub backdrop: bool,
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    let state = state(app);
    let (scenes, theme, has_account) = {
        let settings = state.settings.lock().unwrap();
        let mut scenes: Vec<SceneView> =
            builtin_scenes().iter().map(|(name, scene)| scene_view(name, scene, true)).collect();
        scenes.extend(settings.custom_scenes.iter().map(|(name, scene)| scene_view(name, scene, false)));
        (scenes, settings.theme.clone(), !settings.username.trim().is_empty())
    };
    Snapshot {
        bulbs: state.bulbs.views(),
        scenes,
        theme,
        web: state.web.status(),
        has_account,
        temperature_range: TEMPERATURE_RANGE,
        backdrop: state.backdrop,
    }
}

pub fn command(app: &AppHandle, targets: &[String], patch: Patch) {
    state(app).bulbs.command(targets, patch);
}

pub fn refresh(app: &AppHandle) {
    state(app).bulbs.refresh();
}

pub fn apply_scene(app: &AppHandle, name: &str, targets: &[String]) -> Result<(), String> {
    let custom = state(app).settings.lock().unwrap().custom_scenes.get(name).cloned();
    let scene = custom
        .or_else(|| builtin_scenes().into_iter().find(|(n, _)| *n == name).map(|(_, s)| s))
        .ok_or_else(|| format!("No scene named “{name}”"))?;
    command(app, targets, scene_patch(&scene));
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInput {
    pub brightness: u8,
    pub color: Option<HueSat>,
    pub temperature: Option<u16>,
}

pub fn save_scene(app: &AppHandle, name: &str, input: SceneInput) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 32 {
        return Err("Scene names need 1–32 characters".into());
    }
    if builtin_scenes().iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Err(format!("“{name}” is a built-in scene"));
    }
    let brightness = input.brightness.clamp(1, 100);
    let scene = match input.color {
        Some(c) => Scene { temperature: None, hsv: Some([c.hue.min(360), c.saturation.clamp(1, 100) as u16, brightness as u16]), brightness: Some(brightness) },
        None => Scene { temperature: Some(input.temperature.unwrap_or(3000)), hsv: None, brightness: Some(brightness) },
    };
    update_settings(app, |s| {
        s.custom_scenes.insert(name.to_string(), scene);
    })
}

pub fn delete_scene(app: &AppHandle, name: &str) -> Result<(), String> {
    update_settings(app, |s| {
        s.custom_scenes.remove(name);
    })
}

fn update_settings(app: &AppHandle, f: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let state = state(app);
    {
        let mut settings = state.settings.lock().unwrap();
        let mut next = settings.clone();
        f(&mut next);
        settings::save(&next)?;
        *settings = next;
    }
    state.bulbs.notify();
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSettingsView {
    pub enabled: bool,
    pub port: u16,
    pub lan: bool,
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub devices: Vec<DeviceEntry>,
    pub theme: String,
    /// False for the web UI: account, startup and web-access settings are desktop-only.
    pub desktop: bool,
    pub username: String,
    pub has_tapo_password: bool,
    pub start_minimized: bool,
    pub launch_on_startup: bool,
    pub web: WebSettingsView,
}

pub fn settings_view(app: &AppHandle, for_web: bool) -> SettingsView {
    let s = state(app).settings.lock().unwrap().clone();
    let desktop = !for_web;
    SettingsView {
        has_tapo_password: desktop && secrets::tapo_password(s.username.trim()).is_some(),
        username: if desktop { s.username.clone() } else { String::new() },
        devices: s.devices,
        theme: s.theme,
        desktop,
        start_minimized: s.start_minimized,
        launch_on_startup: s.launch_on_startup,
        web: WebSettingsView {
            enabled: s.web.enabled,
            port: s.web.port,
            lan: s.web.lan,
            has_password: s.web.password_hash.is_some(),
        },
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebUpdate {
    pub enabled: bool,
    pub port: u16,
    pub lan: bool,
    /// `None` keeps the current password, `Some("")` removes it, anything else sets it.
    pub password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub devices: Vec<DeviceEntry>,
    pub theme: String,
    // Desktop-only; ignored when the update comes from the web UI.
    pub username: Option<String>,
    pub tapo_password: Option<String>,
    pub start_minimized: Option<bool>,
    pub launch_on_startup: Option<bool>,
    pub web: Option<WebUpdate>,
}

fn clean_devices(devices: Vec<DeviceEntry>) -> Result<Vec<DeviceEntry>, String> {
    let mut out: Vec<DeviceEntry> = Vec::new();
    for d in devices {
        let ip: IpAddr = d.ip.trim().parse().map_err(|_| format!("“{}” is not a valid IP address", d.ip.trim()))?;
        let ip = ip.to_string();
        if out.iter().any(|e| e.ip == ip) {
            return Err(format!("{ip} is listed twice"));
        }
        out.push(DeviceEntry { ip, name: d.name.trim().to_string(), room: d.room.trim().to_string() });
    }
    Ok(out)
}

pub async fn save_settings(app: &AppHandle, update: SettingsUpdate, for_web: bool) -> Result<SettingsView, String> {
    let state = state(app);
    let mut next = state.settings.lock().unwrap().clone();
    let previous_web = next.web.clone();

    next.devices = clean_devices(update.devices)?;
    next.theme = match update.theme.as_str() {
        "light" | "dark" => update.theme,
        _ => "system".into(),
    };

    if !for_web {
        if let Some(username) = update.username {
            next.username = username.trim().to_string();
        }
        if let Some(password) = update.tapo_password.filter(|p| !p.is_empty()) {
            if next.username.is_empty() {
                return Err("Enter the Tapo account email before the password".into());
            }
            secrets::set_tapo_password(&next.username, &password)?;
        }
        if let Some(v) = update.start_minimized {
            next.start_minimized = v;
        }
        if let Some(v) = update.launch_on_startup {
            if v != next.launch_on_startup {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                if v { autostart.enable() } else { autostart.disable() }
                    .map_err(|e| format!("Could not change the Windows sign-in setting: {e}"))?;
            }
            next.launch_on_startup = v;
        }
        if let Some(web) = update.web {
            if web.port < 1024 {
                return Err("Use a port between 1024 and 65535".into());
            }
            next.web.enabled = web.enabled;
            next.web.port = web.port;
            next.web.lan = web.lan;
            match web.password.as_deref() {
                Some("") => next.web.password_hash = None,
                Some(password) => next.web.password_hash = Some(secrets::hash_web_password(password)?),
                None => {}
            }
        }
    }

    settings::save(&next)?;
    *state.settings.lock().unwrap() = next.clone();
    state.bulbs.configure(&next.devices, credentials_for(&next));
    apply_theme(app, &next.theme);
    if next.web != previous_web {
        state.web.apply(app.clone(), next.web.clone(), state.bulbs.first_ip()).await;
    }
    state.bulbs.notify();
    Ok(settings_view(app, for_web))
}

pub fn apply_theme(app: &AppHandle, theme: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_theme(match theme {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        });
    }
}
