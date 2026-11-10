//! Operations shared by the desktop UI (Tauri commands) and the web server.

use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _, Theme};

use crate::bulbs::{BulbView, HueSat, Manager, Patch, Status, TEMPERATURE_RANGE};
use crate::effects::{EffectName, EffectOptions, Effects};
use crate::secrets;
use crate::server::{WebServer, WebStatus};
use crate::settings::{self, DeviceEntry, Scene, Settings};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub bulbs: Arc<Manager>,
    pub effects: Arc<Effects>,
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
    let white = |temperature, brightness| Scene {
        temperature: Some(temperature),
        hsv: None,
        brightness: Some(brightness),
    };
    let color = |h, s, brightness| Scene {
        temperature: None,
        hsv: Some([h, s, brightness as u16]),
        brightness: Some(brightness),
    };
    vec![
        ("Focus", white(4200, 95)),
        ("Reading", white(3000, 88)),
        ("Relax", white(2800, 55)),
        ("Night", white(2500, 12)),
        ("Dusk", color(20, 85, 48)),
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
    let color = scene.hsv.map(|[hue, saturation, _]| HueSat {
        hue,
        saturation: saturation.clamp(1, 100) as u8,
    });
    Patch {
        on: Some(true),
        brightness: scene
            .brightness
            .or(scene.hsv.map(|hsv| hsv[2].clamp(1, 100) as u8)),
        temperature: if color.is_none() {
            scene.temperature
        } else {
            None
        },
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
    /// Effects are experimental; until this is true they can't be started.
    pub effects_acknowledged: bool,
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    let state = state(app);
    let (scenes, theme, has_account, effects_acknowledged) = {
        let settings = state.settings.lock().unwrap();
        let mut scenes: Vec<SceneView> = builtin_scenes()
            .iter()
            .map(|(name, scene)| scene_view(name, scene, true))
            .collect();
        scenes.extend(
            settings
                .custom_scenes
                .iter()
                .map(|(name, scene)| scene_view(name, scene, false)),
        );
        (
            scenes,
            settings.theme.clone(),
            !settings.username.trim().is_empty(),
            settings.effects_acknowledged,
        )
    };
    let running = state.effects.running();
    let mut bulbs = state.bulbs.views();
    for bulb in &mut bulbs {
        bulb.effect = running.get(&bulb.ip).copied();
    }
    Snapshot {
        bulbs,
        scenes,
        theme,
        web: state.web.status(),
        has_account,
        temperature_range: TEMPERATURE_RANGE,
        backdrop: state.backdrop,
        effects_acknowledged,
    }
}

/// Turn targets given as bulb IPs, bulb names or room names (case-insensitive, in
/// that order of precedence) into bulb IPs. An empty list means every bulb.
pub fn resolve_targets(app: &AppHandle, targets: &[String]) -> Result<Vec<String>, String> {
    resolve_in(&state(app).bulbs.views(), targets)
}

fn resolve_in(bulbs: &[BulbView], targets: &[String]) -> Result<Vec<String>, String> {
    let mut ips: Vec<String> = Vec::new();
    for target in targets {
        let t = target.trim();
        let matched: Vec<&BulbView> = match bulbs
            .iter()
            .find(|b| b.ip == t || b.name.eq_ignore_ascii_case(t))
        {
            Some(bulb) => vec![bulb],
            None => bulbs
                .iter()
                .filter(|b| !b.room.is_empty() && b.room.eq_ignore_ascii_case(t))
                .collect(),
        };
        if matched.is_empty() {
            let known: Vec<String> = bulbs
                .iter()
                .map(|b| format!("{} ({}, room “{}”)", b.name, b.ip, b.room))
                .collect();
            return Err(format!(
                "No bulb or room matches “{t}”. Bulbs: {}",
                known.join("; ")
            ));
        }
        for bulb in matched {
            if !ips.contains(&bulb.ip) {
                ips.push(bulb.ip.clone());
            }
        }
    }
    Ok(ips)
}

/// IPs for a resolved target list (empty means every bulb).
fn target_ips(app: &AppHandle, targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        state(app).bulbs.views().into_iter().map(|b| b.ip).collect()
    } else {
        targets.to_vec()
    }
}

/// A user change: it takes over the bulbs from any running effect.
pub fn command(app: &AppHandle, targets: &[String], patch: Patch) {
    let state = state(app);
    state.effects.release(&target_ips(app, targets));
    state.bulbs.command(targets, patch);
}

pub const EFFECTS_NOT_ACKNOWLEDGED: &str = "Effects are experimental and haven't been confirmed on this PC yet. \
Tell the user: Tapo bulbs can't fade on their own, so effects send a stream of small changes (up to 6 per \
second per bulb); long-term wear from hours of use is unknown; alert flashes the lights. If they agree, \
POST /api/effects/acknowledge once, then start the effect again.";

pub fn effects_acknowledged(app: &AppHandle) -> bool {
    state(app).settings.lock().unwrap().effects_acknowledged
}

/// Record that the user accepted the experimental-effects warning (asked once per PC).
pub fn acknowledge_effects(app: &AppHandle) -> Result<(), String> {
    update_settings(app, |s| s.effects_acknowledged = true)
}

/// Start an effect on the given bulbs (IPs; empty means all). Bulbs running a
/// looping effect are first put back to how they were, so that becomes the base.
pub fn start_effect(
    app: &AppHandle,
    name: EffectName,
    targets: &[String],
    options: EffectOptions,
) -> Result<(), String> {
    if !effects_acknowledged(app) {
        return Err(EFFECTS_NOT_ACKNOWLEDGED.into());
    }
    let state = state(app);
    let ips = target_ips(app, targets);
    for (ip, patch) in state.effects.stop(&ips) {
        state.bulbs.command(&[ip], patch);
    }
    let views: Vec<BulbView> = state
        .bulbs
        .views()
        .into_iter()
        .filter(|b| ips.contains(&b.ip) && b.status == Status::Online)
        .collect();
    if views.is_empty() {
        return Err("None of those bulbs is online".into());
    }
    state
        .effects
        .start(state.bulbs.clone(), name, views, options);
    state.bulbs.notify();
    Ok(())
}

/// Stop effects on the given bulbs (empty means all); looping effects put the
/// bulbs back to how they were.
pub fn stop_effects(app: &AppHandle, targets: &[String]) {
    let state = state(app);
    for (ip, patch) in state.effects.stop(&target_ips(app, targets)) {
        state.bulbs.command(&[ip], patch);
    }
    state.bulbs.notify();
}

pub fn refresh(app: &AppHandle) {
    state(app).bulbs.refresh();
}

pub fn apply_scene(app: &AppHandle, name: &str, targets: &[String]) -> Result<(), String> {
    let custom = state(app)
        .settings
        .lock()
        .unwrap()
        .custom_scenes
        .get(name)
        .cloned();
    let scene = custom
        .or_else(|| {
            builtin_scenes()
                .into_iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| s)
        })
        .ok_or_else(|| format!("No scene named “{name}”"))?;
    command(app, targets, scene_patch(&scene));
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    if builtin_scenes()
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        return Err(format!("“{name}” is a built-in scene"));
    }
    let brightness = input.brightness.clamp(1, 100);
    let scene = match input.color {
        Some(c) => Scene {
            temperature: None,
            hsv: Some([
                c.hue.min(360),
                c.saturation.clamp(1, 100) as u16,
                brightness as u16,
            ]),
            brightness: Some(brightness),
        },
        None => Scene {
            temperature: Some(input.temperature.unwrap_or(3000)),
            hsv: None,
            brightness: Some(brightness),
        },
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
        username: if desktop {
            s.username.clone()
        } else {
            String::new()
        },
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
        let ip: IpAddr =
            d.ip.trim()
                .parse()
                .map_err(|_| format!("“{}” is not a valid IP address", d.ip.trim()))?;
        let ip = ip.to_string();
        if out.iter().any(|e| e.ip == ip) {
            return Err(format!("{ip} is listed twice"));
        }
        out.push(DeviceEntry {
            ip,
            name: d.name.trim().to_string(),
            room: d.room.trim().to_string(),
        });
    }
    Ok(out)
}

pub async fn save_settings(
    app: &AppHandle,
    update: SettingsUpdate,
    for_web: bool,
) -> Result<SettingsView, String> {
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
                if v {
                    autostart.enable()
                } else {
                    autostart.disable()
                }
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
                Some(password) => {
                    next.web.password_hash = Some(secrets::hash_web_password(password)?)
                }
                None => {}
            }
        }
    }

    settings::save(&next)?;
    *state.settings.lock().unwrap() = next.clone();
    state.bulbs.configure(&next.devices, credentials_for(&next));
    apply_theme(app, &next.theme);
    if next.web != previous_web {
        state
            .web
            .apply(app.clone(), next.web.clone(), state.bulbs.first_ip())
            .await;
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

#[cfg(test)]
mod tests {
    use super::resolve_in;
    use crate::bulbs::{BulbView, Mode, Status};

    fn bulb(ip: &str, name: &str, room: &str) -> BulbView {
        BulbView {
            ip: ip.into(),
            name: name.into(),
            room: room.into(),
            model: None,
            firmware: None,
            status: Status::Online,
            error: None,
            on: true,
            brightness: 50,
            mode: Mode::White,
            hue: 0,
            saturation: 100,
            temperature: 3000,
            signal: None,
            effect: None,
        }
    }

    fn targets(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_ips_names_and_rooms() {
        let bulbs = [
            bulb("10.0.0.1", "Desk", "Office"),
            bulb("10.0.0.2", "Shelf", "Office"),
            bulb("10.0.0.3", "Lamp", "Bedroom"),
        ];
        assert_eq!(resolve_in(&bulbs, &[]).unwrap(), Vec::<String>::new());
        assert_eq!(
            resolve_in(&bulbs, &targets(&["10.0.0.3"])).unwrap(),
            ["10.0.0.3"]
        );
        assert_eq!(
            resolve_in(&bulbs, &targets(&["desk"])).unwrap(),
            ["10.0.0.1"]
        );
        assert_eq!(
            resolve_in(&bulbs, &targets(&["OFFICE", "Desk"])).unwrap(),
            ["10.0.0.1", "10.0.0.2"]
        );
        let err = resolve_in(&bulbs, &targets(&["Kitchen"])).unwrap_err();
        assert!(err.contains("Kitchen") && err.contains("Desk (10.0.0.1"));
    }

    #[test]
    fn bulb_name_wins_over_room_name() {
        let bulbs = [
            bulb("10.0.0.1", "Office", "Office"),
            bulb("10.0.0.2", "Shelf", "Office"),
        ];
        assert_eq!(
            resolve_in(&bulbs, &targets(&["office"])).unwrap(),
            ["10.0.0.1"]
        );
    }
}
