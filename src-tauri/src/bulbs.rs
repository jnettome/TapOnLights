//! Bulb connections. Each bulb has one worker task that owns a kept-open Tapo
//! session. Commands update the shared view immediately (optimistic UI) and are
//! queued to the worker, which merges everything queued while a request is in
//! flight and sends only the latest values in one request ("latest wins").

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tapo::responses::DeviceInfoColorLightResult;
use tapo::{ApiClient, ColorLightHandler, TapoResponseError};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Instant, sleep_until, timeout};

use crate::settings::DeviceEntry;

const POLL_ONLINE: Duration = Duration::from_secs(10);
const POLL_OFFLINE: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(8);
pub const TEMPERATURE_RANGE: (u16, u16) = (2500, 6500);

pub type Credentials = Arc<RwLock<Option<(String, String)>>>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    White,
    Color,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Connecting,
    Online,
    Offline,
    AuthError,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulbView {
    pub ip: String,
    pub name: String,
    pub room: String,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub status: Status,
    pub error: Option<String>,
    pub on: bool,
    pub brightness: u8,
    pub mode: Mode,
    pub hue: u16,
    pub saturation: u8,
    pub temperature: u16,
    pub signal: Option<u8>,
    /// The effect running on this bulb, if any (filled in by `core::snapshot`).
    pub effect: Option<crate::effects::EffectName>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HueSat {
    pub hue: u16,
    pub saturation: u8,
}

/// A partial change. Brightness, color or temperature imply "on".
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Patch {
    pub on: Option<bool>,
    pub brightness: Option<u8>,
    pub color: Option<HueSat>,
    pub temperature: Option<u16>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.on.is_none() && !self.has_light()
    }

    fn has_light(&self) -> bool {
        self.brightness.is_some() || self.color.is_some() || self.temperature.is_some()
    }

    pub fn sanitized(mut self) -> Self {
        self.brightness = self.brightness.map(|b| b.clamp(1, 100));
        self.color = self.color.map(|c| HueSat {
            hue: c.hue.min(360),
            saturation: c.saturation.clamp(1, 100),
        });
        self.temperature = self
            .temperature
            .map(|t| t.clamp(TEMPERATURE_RANGE.0, TEMPERATURE_RANGE.1));
        if self.color.is_some() {
            self.temperature = None;
        }
        if self.on == Some(false) {
            self.brightness = None;
            self.color = None;
            self.temperature = None;
        }
        self
    }

    fn merge(&mut self, newer: Patch) {
        if newer.on == Some(false) {
            *self = newer;
            return;
        }
        if newer.on.is_some() {
            self.on = newer.on;
        } else if newer.has_light() && self.on == Some(false) {
            self.on = None;
        }
        if newer.brightness.is_some() {
            self.brightness = newer.brightness;
        }
        if newer.color.is_some() {
            self.color = newer.color;
            self.temperature = None;
        }
        if newer.temperature.is_some() {
            self.temperature = newer.temperature;
            self.color = None;
        }
    }

    fn apply_to(&self, view: &mut BulbView) {
        if let Some(on) = self.on {
            view.on = on;
        } else if self.has_light() {
            view.on = true;
        }
        if let Some(b) = self.brightness {
            view.brightness = b;
        }
        if let Some(c) = self.color {
            view.mode = Mode::Color;
            view.hue = c.hue;
            view.saturation = c.saturation;
        }
        if let Some(t) = self.temperature {
            view.mode = Mode::White;
            view.temperature = t;
        }
    }

    async fn send(&self, handler: &ColorLightHandler) -> Result<(), tapo::Error> {
        if self.on == Some(false) {
            return handler.off().await;
        }
        if !self.has_light() {
            return if self.on == Some(true) {
                handler.on().await
            } else {
                Ok(())
            };
        }
        let mut params = handler.set().on();
        if let Some(b) = self.brightness {
            params = params.brightness(b);
        }
        if let Some(c) = self.color {
            params = params.hue_saturation(c.hue, c.saturation);
        } else if let Some(t) = self.temperature {
            params = params.color_temperature(t);
        }
        params.send(handler).await
    }
}

enum Msg {
    Patch(Patch),
    Refresh,
    Reconnect,
    Stop,
}

struct Bulb {
    ip: String,
    view: Mutex<BulbView>,
    /// Incremented on every command; a state read that started before a command is discarded.
    generation: AtomicU64,
    tx: mpsc::UnboundedSender<Msg>,
}

pub struct Manager {
    bulbs: Mutex<Vec<Arc<Bulb>>>,
    credentials: Credentials,
    changed: broadcast::Sender<()>,
}

impl Manager {
    pub fn new() -> Arc<Self> {
        let (changed, _) = broadcast::channel(32);
        Arc::new(Self {
            bulbs: Mutex::new(Vec::new()),
            credentials: Arc::default(),
            changed,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changed.subscribe()
    }

    pub fn notify(&self) {
        let _ = self.changed.send(());
    }

    /// Sync workers with the configured devices. Changed credentials force a new login.
    pub fn configure(&self, devices: &[DeviceEntry], credentials: Option<(String, String)>) {
        let credentials_changed = {
            let mut current = self.credentials.write().unwrap();
            let changed = *current != credentials;
            *current = credentials;
            changed
        };
        let mut bulbs = self.bulbs.lock().unwrap();
        let mut next = Vec::with_capacity(devices.len());
        for entry in devices {
            if let Some(existing) = bulbs.iter().find(|b| b.ip == entry.ip) {
                {
                    let mut view = existing.view.lock().unwrap();
                    view.room = entry.room.clone();
                    if !entry.name.is_empty() {
                        view.name = entry.name.clone();
                    }
                }
                if credentials_changed {
                    let _ = existing.tx.send(Msg::Reconnect);
                }
                next.push(existing.clone());
            } else {
                next.push(self.spawn(entry));
            }
        }
        for old in bulbs.iter() {
            if !next.iter().any(|b| Arc::ptr_eq(b, old)) {
                let _ = old.tx.send(Msg::Stop);
            }
        }
        *bulbs = next;
        drop(bulbs);
        self.notify();
    }

    fn spawn(&self, entry: &DeviceEntry) -> Arc<Bulb> {
        let (tx, rx) = mpsc::unbounded_channel();
        let bulb = Arc::new(Bulb {
            ip: entry.ip.clone(),
            view: Mutex::new(BulbView {
                ip: entry.ip.clone(),
                name: if entry.name.is_empty() {
                    entry.ip.clone()
                } else {
                    entry.name.clone()
                },
                room: entry.room.clone(),
                model: None,
                firmware: None,
                status: Status::Connecting,
                error: None,
                on: false,
                brightness: 100,
                mode: Mode::White,
                hue: 0,
                saturation: 100,
                temperature: 3000,
                signal: None,
                effect: None,
            }),
            generation: AtomicU64::new(0),
            tx,
        });
        let worker = Worker {
            bulb: bulb.clone(),
            credentials: self.credentials.clone(),
            changed: self.changed.clone(),
            named_by_user: !entry.name.is_empty(),
            handler: None,
        };
        tauri::async_runtime::spawn(worker.run(rx));
        bulb
    }

    pub fn views(&self) -> Vec<BulbView> {
        self.bulbs
            .lock()
            .unwrap()
            .iter()
            .map(|b| b.view.lock().unwrap().clone())
            .collect()
    }

    /// Apply a patch to the given bulbs (all bulbs when `targets` is empty).
    pub fn command(&self, targets: &[String], patch: Patch) {
        let patch = patch.sanitized();
        for bulb in self.bulbs.lock().unwrap().iter() {
            if !targets.is_empty() && !targets.contains(&bulb.ip) {
                continue;
            }
            bulb.generation.fetch_add(1, Ordering::SeqCst);
            patch.apply_to(&mut bulb.view.lock().unwrap());
            let _ = bulb.tx.send(Msg::Patch(patch.clone()));
        }
        self.notify();
    }

    pub fn refresh(&self) {
        for bulb in self.bulbs.lock().unwrap().iter() {
            let _ = bulb.tx.send(Msg::Refresh);
        }
    }

    /// Best LAN address to show for the web UI: the local interface that routes to a bulb.
    pub fn first_ip(&self) -> Option<String> {
        self.bulbs.lock().unwrap().first().map(|b| b.ip.clone())
    }
}

struct Worker {
    bulb: Arc<Bulb>,
    credentials: Credentials,
    changed: broadcast::Sender<()>,
    named_by_user: bool,
    handler: Option<ColorLightHandler>,
}

enum Op {
    Send(Patch),
    Read,
}

enum Done {
    Sent,
    Info(Box<DeviceInfoColorLightResult>),
}

enum Failure {
    NoCredentials,
    Timeout,
    Tapo(tapo::Error),
}

impl Failure {
    fn status_and_message(&self, ip: &str) -> (Status, String) {
        match self {
            Failure::NoCredentials => (
                Status::AuthError,
                "Add your Tapo account in Settings".into(),
            ),
            Failure::Timeout => (Status::Offline, format!("No answer from {ip}")),
            Failure::Tapo(tapo::Error::Tapo(TapoResponseError::Unauthorized {
                kind,
                description,
            })) => {
                let message = if *kind == "FORBIDDEN" {
                    description.clone()
                } else {
                    "Sign-in rejected. Check the Tapo email and password in Settings.".into()
                };
                (Status::AuthError, message)
            }
            Failure::Tapo(tapo::Error::Http(_)) => (Status::Offline, format!("Can't reach {ip}")),
            Failure::Tapo(e) => (Status::Offline, e.to_string()),
        }
    }
}

impl Worker {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        let mut next_poll = Instant::now();
        loop {
            let mut pending: Option<Patch> = None;
            let mut refresh = false;
            let mut stop = false;
            let first = tokio::select! {
                msg = rx.recv() => Some(msg.unwrap_or(Msg::Stop)),
                _ = sleep_until(next_poll) => None,
            };
            // Take everything queued meanwhile, so only the latest values are sent.
            let mut queued: Vec<Msg> = first.into_iter().collect();
            if queued.is_empty() {
                refresh = true;
            }
            while let Ok(msg) = rx.try_recv() {
                queued.push(msg);
            }
            for msg in queued {
                match msg {
                    Msg::Patch(p) => match pending.as_mut() {
                        Some(existing) => existing.merge(p),
                        None => pending = Some(p),
                    },
                    Msg::Refresh => refresh = true,
                    Msg::Reconnect => {
                        self.handler = None;
                        refresh = true;
                    }
                    Msg::Stop => stop = true,
                }
            }
            if stop {
                break;
            }

            if let Some(patch) = pending {
                match self.with_session(Op::Send(patch)).await {
                    Ok(_) => {
                        self.set_online();
                        next_poll = Instant::now() + POLL_ONLINE;
                    }
                    Err(failure) => {
                        self.set_failure(&failure);
                        refresh = !matches!(failure, Failure::NoCredentials | Failure::Timeout);
                        next_poll = Instant::now() + POLL_OFFLINE;
                    }
                }
            }

            if refresh {
                let started = self.bulb.generation.load(Ordering::SeqCst);
                match self.with_session(Op::Read).await {
                    Ok(Done::Info(info)) => {
                        // A command issued while reading wins over the (older) device state.
                        if started == self.bulb.generation.load(Ordering::SeqCst) {
                            self.apply_info(&info);
                        } else {
                            self.set_online();
                        }
                        next_poll = Instant::now() + POLL_ONLINE;
                    }
                    Ok(Done::Sent) => unreachable!("a read returns device info"),
                    Err(failure) => {
                        self.set_failure(&failure);
                        next_poll = Instant::now() + POLL_OFFLINE;
                    }
                }
            }
        }
    }

    /// Run `op` on the open session, logging in first if needed. A failed request
    /// on an existing session (expired, connection dropped) gets one fresh login.
    async fn with_session(&mut self, op: Op) -> Result<Done, Failure> {
        for attempt in 0..2 {
            let fresh = self.handler.is_none();
            if fresh {
                let (user, password) = self
                    .credentials
                    .read()
                    .unwrap()
                    .clone()
                    .ok_or(Failure::NoCredentials)?;
                let login = ApiClient::new(user, password)
                    .with_timeout(REQUEST_TIMEOUT)
                    .l530(self.bulb.ip.clone());
                self.handler = Some(match timeout(LOGIN_TIMEOUT, login).await {
                    Ok(Ok(handler)) => handler,
                    Ok(Err(e)) => return Err(Failure::Tapo(e)),
                    Err(_) => return Err(Failure::Timeout),
                });
            }
            let handler = self.handler.as_ref().expect("handler set above");
            let request = async {
                match &op {
                    Op::Send(patch) => patch.send(handler).await.map(|_| Done::Sent),
                    Op::Read => handler
                        .get_device_info()
                        .await
                        .map(|info| Done::Info(Box::new(info))),
                }
            };
            match timeout(REQUEST_TIMEOUT, request).await {
                Ok(Ok(value)) => return Ok(value),
                Ok(Err(e)) => {
                    self.handler = None;
                    let retry = attempt == 0
                        && !fresh
                        && matches!(
                            e,
                            tapo::Error::Http(_)
                                | tapo::Error::Tapo(TapoResponseError::Unauthorized { .. })
                        );
                    if !retry {
                        return Err(Failure::Tapo(e));
                    }
                }
                Err(_) => {
                    self.handler = None;
                    return Err(Failure::Timeout);
                }
            }
        }
        unreachable!("second attempt always returns")
    }

    fn update(&self, f: impl FnOnce(&mut BulbView) -> bool) {
        let changed = f(&mut self.bulb.view.lock().unwrap());
        if changed {
            let _ = self.changed.send(());
        }
    }

    fn set_online(&self) {
        self.update(|v| {
            let changed = v.status != Status::Online || v.error.is_some();
            v.status = Status::Online;
            v.error = None;
            changed
        });
    }

    fn set_failure(&self, failure: &Failure) {
        let (status, message) = failure.status_and_message(&self.bulb.ip);
        self.update(|v| {
            let changed = v.status != status || v.error.as_deref() != Some(message.as_str());
            v.status = status;
            v.error = Some(message);
            changed
        });
    }

    fn apply_info(&self, info: &DeviceInfoColorLightResult) {
        let named_by_user = self.named_by_user;
        self.update(|v| {
            let before = format!("{v:?}");
            v.status = Status::Online;
            v.error = None;
            v.model = Some(info.model.clone());
            v.firmware = info.fw_ver.split_whitespace().next().map(str::to_string);
            v.signal = Some(info.signal_level);
            v.on = info.device_on;
            v.brightness = info.brightness.max(1);
            if info.color_temp > 0 {
                v.mode = Mode::White;
                v.temperature = info.color_temp;
            } else {
                v.mode = Mode::Color;
            }
            if let (Some(hue), Some(saturation)) = (info.hue, info.saturation) {
                v.hue = hue;
                v.saturation = saturation.clamp(1, 100) as u8;
            }
            if !named_by_user && !info.nickname.is_empty() {
                v.name = info.nickname.clone();
            }
            format!("{v:?}") != before
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{HueSat, Patch};

    #[test]
    fn newest_values_win_and_color_replaces_temperature() {
        let mut p = Patch {
            brightness: Some(10),
            temperature: Some(3000),
            ..Default::default()
        };
        p.merge(Patch {
            brightness: Some(20),
            color: Some(HueSat {
                hue: 120,
                saturation: 80,
            }),
            ..Default::default()
        });
        assert_eq!(p.brightness, Some(20));
        assert_eq!(
            p.color,
            Some(HueSat {
                hue: 120,
                saturation: 80
            })
        );
        assert_eq!(p.temperature, None);
        p.merge(Patch {
            temperature: Some(2700),
            ..Default::default()
        });
        assert_eq!((p.color, p.temperature), (None, Some(2700)));
    }

    #[test]
    fn off_clears_pending_light_changes_and_light_after_off_turns_on() {
        let mut p = Patch {
            brightness: Some(40),
            ..Default::default()
        };
        p.merge(Patch {
            on: Some(false),
            ..Default::default()
        });
        assert_eq!((p.on, p.brightness), (Some(false), None));
        p.merge(Patch {
            brightness: Some(60),
            ..Default::default()
        });
        assert_eq!((p.on, p.brightness), (None, Some(60)));
    }

    #[test]
    fn sanitize_clamps_and_rejects_unknown_fields() {
        let p = Patch {
            brightness: Some(0),
            temperature: Some(9000),
            ..Default::default()
        }
        .sanitized();
        assert_eq!((p.brightness, p.temperature), (Some(1), Some(6500)));
        assert!(serde_json::from_str::<Patch>(r#"{"power": "on"}"#).is_err());
        assert!(serde_json::from_str::<Patch>("{}").unwrap().is_empty());
    }
}
