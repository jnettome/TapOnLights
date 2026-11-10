#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bulbs;
mod core;
mod secrets;
mod server;
mod settings;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::bulbs::Patch;
use crate::core::{AppState, SceneInput, SettingsUpdate, SettingsView, Snapshot};

// ---- commands -------------------------------------------------------------

#[tauri::command]
fn snapshot(app: AppHandle) -> Snapshot {
    core::snapshot(&app)
}

#[tauri::command]
fn command(app: AppHandle, targets: Vec<String>, patch: Patch) {
    core::command(&app, &targets, patch);
}

#[tauri::command]
fn refresh(app: AppHandle) {
    core::refresh(&app);
}

#[tauri::command]
fn apply_scene(app: AppHandle, name: String, targets: Vec<String>) -> Result<(), String> {
    core::apply_scene(&app, &name, &targets)
}

#[tauri::command]
fn save_scene(app: AppHandle, name: String, scene: SceneInput) -> Result<(), String> {
    core::save_scene(&app, &name, scene)
}

#[tauri::command]
fn delete_scene(app: AppHandle, name: String) -> Result<(), String> {
    core::delete_scene(&app, &name)
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsView {
    core::settings_view(&app, false)
}

#[tauri::command]
async fn save_settings(app: AppHandle, update: SettingsUpdate) -> Result<SettingsView, String> {
    core::save_settings(&app, update, false).await
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    hide_popup(&app);
}

#[tauri::command]
fn open_web(app: AppHandle) {
    open_web_page(&app);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---- popup window ---------------------------------------------------------

fn now_ms(app: &AppHandle) -> u64 {
    core::state(app).started.elapsed().as_millis() as u64
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn hide_popup(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            core::state(app).last_hidden_ms.store(now_ms(app), Ordering::SeqCst);
        }
    }
}

/// Place the popup next to the tray icon (or bottom-right of the work area),
/// like the Windows 11 Quick Settings flyout.
fn position_popup(app: &AppHandle, window: &WebviewWindow, anchor: Option<Rect>) {
    let Ok(size) = window.outer_size() else { return };
    let monitor = anchor
        .as_ref()
        .and_then(|rect| {
            let p = rect.position.to_physical::<f64>(1.0);
            app.monitor_from_point(p.x, p.y).ok().flatten()
        })
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (12.0 * scale).round() as i32;
    let (w, h) = (size.width as i32, size.height as i32);
    let left = area.position.x + margin;
    let top = area.position.y + margin;
    let right = area.position.x + area.size.width as i32 - w - margin;
    let bottom = area.position.y + area.size.height as i32 - h - margin;

    let (mut x, mut y) = (right, bottom);
    if let Some(rect) = anchor {
        let p = rect.position.to_physical::<i32>(scale);
        let s = rect.size.to_physical::<i32>(scale);
        x = (p.x + s.width / 2 - w / 2).clamp(left, right.max(left));
        // Taskbar at the top of the screen: open downward.
        if p.y < area.position.y + area.size.height as i32 / 2 {
            y = top;
        }
    }
    let _ = window.set_position(PhysicalPosition::new(x, y.max(top)));
}

fn show_popup(app: &AppHandle, anchor: Option<Rect>) {
    let Some(window) = main_window(app) else { return };
    position_popup(app, &window, anchor);
    let _ = window.show();
    let _ = window.set_focus();
    core::refresh(app);
    let _ = app.emit("popup-shown", ());
    watch_for_click_away(app);
}

/// Hide like a flyout once the user switches to another app. Focus events are not
/// reliable for this (the embedded browser moves focus internally), so poll which
/// process owns the foreground window while the popup is open. The popup only
/// auto-hides after it has been in the foreground, so it stays put when Windows
/// refuses to bring it forward (e.g. when started from a background process).
fn watch_for_click_away(app: &AppHandle) {
    let generation = core::state(app).popup_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut was_foreground = false;
        loop {
            tokio::time::sleep(Duration::from_millis(120)).await;
            let state = core::state(&app);
            if state.popup_generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(window) = main_window(&app) else { return };
            if !window.is_visible().unwrap_or(false) {
                return;
            }
            match foreground_is_ours() {
                Some(true) => was_foreground = true,
                Some(false) if was_foreground => {
                    hide_popup(&app);
                    return;
                }
                _ => {}
            }
        }
    });
}

/// Whether the foreground window belongs to this process; `None` while no window is
/// in the foreground (e.g. mid-switch).
#[cfg(windows)]
fn foreground_is_ours() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain Win32 queries with a valid out-pointer.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        Some(pid == std::process::id())
    }
}

#[cfg(not(windows))]
fn foreground_is_ours() -> Option<bool> {
    None
}

fn toggle_popup(app: &AppHandle, anchor: Rect) {
    let Some(window) = main_window(app) else { return };
    if window.is_visible().unwrap_or(false) {
        hide_popup(app);
        return;
    }
    // Clicking the tray icon first blurs (and hides) the open popup; don't reopen it.
    let hidden_at = core::state(app).last_hidden_ms.load(Ordering::SeqCst);
    if hidden_at != 0 && now_ms(app).saturating_sub(hidden_at) < 400 {
        return;
    }
    show_popup(app, Some(anchor));
}

fn open_web_page(app: &AppHandle) {
    let url = core::state(app).web.status().local_url;
    if let Some(url) = url {
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
}

// ---- tray -----------------------------------------------------------------

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let on = MenuItem::with_id(app, "on", "Turn all on", true, None::<&str>)?;
    let off = MenuItem::with_id(app, "off", "Turn all off", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open controls", true, None::<&str>)?;
    let web = MenuItem::with_id(app, "web", "Open web page", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&on, &off, &sep1, &open, &web, &settings, &sep2, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Tapo Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "on" => core::command(app, &[], Patch { on: Some(true), ..Default::default() }),
            "off" => core::command(app, &[], Patch { on: Some(false), ..Default::default() }),
            "open" => show_popup(app, None),
            "web" => open_web_page(app),
            "settings" => {
                show_popup(app, None);
                let _ = app.emit("navigate", "settings");
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                toggle_popup(tray.app_handle(), rect);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

// ---- startup --------------------------------------------------------------

fn windows_11() -> bool {
    #[cfg(windows)]
    {
        windows_version::OsVersion::current().build >= 22000
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn main() {
    let start_hidden_arg = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_popup(app, None)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(move |app| {
            let settings = settings::load();
            let bulbs = bulbs::Manager::new();
            bulbs.configure(&settings.devices, core::credentials_for(&settings));
            let backdrop = windows_11();
            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                bulbs: bulbs.clone(),
                web: server::WebServer::new(),
                last_hidden_ms: AtomicU64::new(0),
                popup_generation: AtomicU64::new(0),
                started: Instant::now(),
                backdrop,
            });

            build_tray(app)?;

            let handle = app.handle().clone();
            if let Some(window) = main_window(&handle) {
                if !backdrop {
                    let _ = window.set_effects(None);
                }
                let h = handle.clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        hide_popup(&h);
                    }
                    _ => {}
                });
            }
            core::apply_theme(&handle, &settings.theme);

            // Push state changes to the popup, coalescing bursts.
            let mut changes = bulbs.subscribe();
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                use tokio::sync::broadcast::error::{RecvError, TryRecvError};
                loop {
                    if let Err(RecvError::Closed) = changes.recv().await {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(12)).await;
                    while matches!(changes.try_recv(), Ok(_) | Err(TryRecvError::Lagged(_))) {}
                    let _ = h.emit("snapshot", core::snapshot(&h));
                }
            });

            let h = handle.clone();
            let web = settings.web.clone();
            tauri::async_runtime::spawn(async move {
                let state = core::state(&h);
                state.web.apply(h.clone(), web, state.bulbs.first_ip()).await;
            });

            if !(start_hidden_arg || settings.start_minimized) {
                show_popup(&handle, None);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            command,
            refresh,
            apply_scene,
            save_scene,
            delete_scene,
            get_settings,
            save_settings,
            hide_window,
            open_web,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tapo Desktop");
}
