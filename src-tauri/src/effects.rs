//! Light effects computed on the PC and sent through the bulb workers.
//!
//! Tapo bulbs have no transition time in their local API: every command jumps
//! to the new value. Smoothness therefore comes from small, frequent steps:
//! up to 6 changes per second for breathe and 4 for the others (an L530
//! accepts ~8/s), only sent when a value actually changes, and brightness is
//! animated in perceived lightness so steps look even. Because workers merge
//! queued changes, a slow bulb skips frames instead of falling behind.
//!
//! A bulb belongs to at most one effect. Any user change to a bulb releases it
//! (the effect stops there); stopping a looping effect explicitly puts the
//! bulb back the way it was before the effect started.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::{Instant, MissedTickBehavior, interval, sleep};

use crate::bulbs::{BulbView, HueSat, Manager, Mode, Patch};

const BREATHE_HZ: f64 = 6.0;
const CANDLE_HZ: f64 = 4.0;
const COLOR_LOOP_HZ: f64 = 4.0;
const FADE_HZ: f64 = 2.0;
/// Default flame color for candle.
const FLAME: HueSat = HueSat {
    hue: 28,
    saturation: 95,
};
/// Default alert color (red).
const ALERT_RED: HueSat = HueSat {
    hue: 0,
    saturation: 100,
};
/// How far colorLoop drifts either side of a chosen color, in degrees.
const DRIFT_WIDTH: f64 = 45.0;
const ALERT_STEP: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectName {
    /// Brightness rises and falls slowly.
    Breathe,
    /// Warm, flickering flame.
    Candle,
    /// Hue drifts around the color wheel.
    ColorLoop,
    /// Fades up from a dim warm glow to bright neutral white.
    Sunrise,
    /// Fades down to a dim warm glow, then turns off.
    Sunset,
    /// A few colored pulses, then back to how the lights were.
    Alert,
}

impl EffectName {
    fn loops(self) -> bool {
        matches!(self, Self::Breathe | Self::Candle | Self::ColorLoop)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectOptions {
    /// Seconds per cycle: breathe (3–60, default 10), colorLoop (10–600, default 60).
    pub period: Option<f64>,
    /// Fade length for sunrise and sunset in minutes (0.5–120, default 15).
    pub minutes: Option<f64>,
    /// Peak brightness for breathe, base for candle, final for sunrise (1–100).
    pub brightness: Option<u8>,
    /// Color for breathe (default: the light's current look), candle (default
    /// flame orange), colorLoop (drift around it instead of the whole wheel)
    /// and alert (default red).
    pub color: Option<HueSat>,
    /// Alert pulses (1–10, default 3).
    pub times: Option<u8>,
}

#[derive(Debug, Clone)]
struct Claim {
    id: u64,
    name: EffectName,
    /// How to put the bulb back when a looping effect is stopped.
    restore: Option<Patch>,
}

pub struct Effects {
    claims: Mutex<HashMap<String, Claim>>,
    next_id: AtomicU64,
}

/// A patch that reproduces what a bulb showed.
pub fn restore_patch(v: &BulbView) -> Patch {
    if !v.on {
        return Patch {
            on: Some(false),
            ..Default::default()
        };
    }
    Patch {
        on: Some(true),
        brightness: Some(v.brightness),
        color: (v.mode == Mode::Color).then_some(HueSat {
            hue: v.hue,
            saturation: v.saturation,
        }),
        temperature: (v.mode == Mode::White).then_some(v.temperature),
    }
}

impl Effects {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            claims: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        })
    }

    /// Which effect each bulb (by IP) is running.
    pub fn running(&self) -> HashMap<String, EffectName> {
        self.claims
            .lock()
            .unwrap()
            .iter()
            .map(|(ip, c)| (ip.clone(), c.name))
            .collect()
    }

    fn claim(&self, name: EffectName, views: &[BulbView]) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let mut claims = self.claims.lock().unwrap();
        for v in views {
            let restore = name.loops().then(|| restore_patch(v));
            claims.insert(v.ip.clone(), Claim { id, name, restore });
        }
        id
    }

    fn owned(&self, id: u64) -> Vec<String> {
        self.claims
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| c.id == id)
            .map(|(ip, _)| ip.clone())
            .collect()
    }

    fn finish(&self, id: u64) {
        self.claims.lock().unwrap().retain(|_, c| c.id != id);
    }

    /// A user change takes over these bulbs: their effects stop where they are.
    /// Returns whether any effect was running on them.
    pub fn release(&self, ips: &[String]) -> bool {
        let mut claims = self.claims.lock().unwrap();
        let before = claims.len();
        for ip in ips {
            claims.remove(ip);
        }
        claims.len() != before
    }

    /// Stop effects on these bulbs. Returns patches that put bulbs running a
    /// looping effect back to how they were before it started.
    pub fn stop(&self, ips: &[String]) -> Vec<(String, Patch)> {
        let mut claims = self.claims.lock().unwrap();
        ips.iter()
            .filter_map(|ip| {
                claims
                    .remove(ip)
                    .and_then(|c| c.restore)
                    .map(|p| (ip.clone(), p))
            })
            .collect()
    }

    /// Run `name` on the bulbs in `views` (their current state is the starting point).
    pub fn start(
        self: &Arc<Self>,
        bulbs: Arc<Manager>,
        name: EffectName,
        views: Vec<BulbView>,
        options: EffectOptions,
    ) {
        let id = self.claim(name, &views);
        let effects = self.clone();
        tauri::async_runtime::spawn(async move {
            run(&effects, &bulbs, id, name, &views, &options).await;
            effects.finish(id);
            bulbs.notify();
        });
    }
}

// ---- frame math (pure, unit-tested) ------------------------------------------

/// Perceived lightness is roughly brightness^(1/2.2). Animating in that space
/// makes steps look even instead of jumping at the dim end.
const GAMMA: f64 = 2.2;

fn to_perceived(brightness: f64) -> f64 {
    (brightness / 100.0).clamp(0.0, 1.0).powf(1.0 / GAMMA)
}

fn from_perceived(lightness: f64) -> u8 {
    (100.0 * lightness.clamp(0.0, 1.0).powf(GAMMA))
        .round()
        .clamp(1.0, 100.0) as u8
}

fn round_kelvin(k: f64) -> u16 {
    ((k / 10.0).round() * 10.0) as u16
}

/// Brightness for breathing: starts at `max`, dips to `min` half a period later.
fn breathe_level(t: f64, period: f64, min: f64, max: f64) -> u8 {
    let x = 0.5 + 0.5 * (TAU * t / period).cos();
    let (lo, hi) = (to_perceived(min), to_perceived(max));
    from_perceived(lo + (hi - lo) * x)
}

/// Sunrise at progress `p` (0–1): brightness 1 → `end`, 2500 K → 4000 K.
fn sunrise_at(p: f64, end: u8) -> (u8, u16) {
    let p = p.clamp(0.0, 1.0);
    (
        from_perceived(to_perceived(f64::from(end)) * p),
        round_kelvin(2500.0 + 1500.0 * p),
    )
}

/// Sunset at progress `p` (0–1): from (`start_b`, `start_k`) down to 1% at 2500 K.
fn sunset_at(p: f64, start_b: u8, start_k: u16) -> (u8, u16) {
    let p = p.clamp(0.0, 1.0);
    let k = f64::from(start_k);
    (
        from_perceived(to_perceived(f64::from(start_b)) * (1.0 - p)),
        round_kelvin(k + (2500.0 - k) * p),
    )
}

/// Hue going around the whole wheel once per `period`.
fn color_loop_hue(t: f64, period: f64, start: f64) -> u16 {
    ((start + 360.0 * t / period).rem_euclid(360.0)).round() as u16 % 360
}

/// Hue swinging ±`width` degrees around `center` once per `period`.
fn color_drift_hue(t: f64, period: f64, center: f64, width: f64) -> u16 {
    ((center + width * (TAU * t / period).sin()).rem_euclid(360.0)).round() as u16 % 360
}

/// Small xorshift generator; candle flicker doesn't need more.
struct Rng(u64);

impl Rng {
    fn seeded(salt: u64) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Self((nanos ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1)
    }

    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---- runners -------------------------------------------------------------------

fn send(bulbs: &Manager, ip: &str, patch: Patch) {
    bulbs.command(&[ip.to_string()], patch);
}

/// Call `frame(t, index, start_view)` for every bulb still owned by effect `id`,
/// `hz` times per second, until no bulb is owned or `length` has passed (the
/// final frame is then sent with `t = length`). `frame` returns `None` to send nothing.
async fn animate(
    effects: &Effects,
    bulbs: &Manager,
    id: u64,
    start: &[BulbView],
    hz: f64,
    length: Option<f64>,
    mut frame: impl FnMut(f64, usize, &BulbView) -> Option<Patch>,
) {
    let mut ticker = interval(Duration::from_secs_f64(1.0 / hz));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let began = Instant::now();
    loop {
        ticker.tick().await;
        let owned = effects.owned(id);
        if owned.is_empty() {
            return;
        }
        let mut t = began.elapsed().as_secs_f64();
        let done = length.is_some_and(|l| t >= l);
        if let Some(l) = length {
            t = t.min(l);
        }
        for (i, v) in start.iter().enumerate() {
            if owned.contains(&v.ip)
                && let Some(patch) = frame(t, i, v)
            {
                send(bulbs, &v.ip, patch);
            }
        }
        if done {
            return;
        }
    }
}

async fn run(
    effects: &Effects,
    bulbs: &Manager,
    id: u64,
    name: EffectName,
    start: &[BulbView],
    o: &EffectOptions,
) {
    let n = start.len().max(1);
    let mut last: Vec<Option<(u8, u16)>> = vec![None; start.len()];
    match name {
        EffectName::Breathe => {
            let period = o.period.unwrap_or(10.0).clamp(3.0, 60.0);
            animate(effects, bulbs, id, start, BREATHE_HZ, None, |t, i, v| {
                let max = f64::from(o.brightness.unwrap_or(v.brightness.max(30)).clamp(1, 100));
                let b = breathe_level(t, period, (max * 0.3).max(3.0), max);
                if last[i] == Some((b, 0)) {
                    return None;
                }
                // The chosen color goes out with the first step; later steps merge onto it.
                let color = if last[i].is_none() { o.color } else { None };
                last[i] = Some((b, 0));
                Some(Patch {
                    brightness: Some(b),
                    color,
                    ..Default::default()
                })
            })
            .await;
        }
        EffectName::Candle => {
            let base = f64::from(o.brightness.unwrap_or(60).clamp(5, 100));
            let flame = o.color.unwrap_or(FLAME);
            let mut flames: Vec<(Rng, f64, f64)> = (0..start.len())
                .map(|i| (Rng::seeded(i as u64 + 1), 1.0, 1.0))
                .collect();
            animate(effects, bulbs, id, start, CANDLE_HZ, None, |_, i, _| {
                let (rng, level, target) = &mut flames[i];
                let roll = rng.next();
                if roll < 0.05 {
                    *target = 0.4 + 0.15 * rng.next(); // a gust
                } else if roll < 0.45 {
                    *target = 0.6 + 0.4 * rng.next();
                }
                *level += (*target - *level) * 0.4;
                let b = (base * *level).round().clamp(1.0, 100.0) as u8;
                let hue = (f64::from(flame.hue) + rng.next() * 6.0 - 3.0).rem_euclid(360.0) as u16;
                let saturation =
                    (f64::from(flame.saturation) - rng.next() * 6.0).clamp(1.0, 100.0) as u8;
                if last[i] == Some((b, hue)) {
                    return None;
                }
                last[i] = Some((b, hue));
                Some(Patch {
                    brightness: Some(b),
                    color: Some(HueSat { hue, saturation }),
                    ..Default::default()
                })
            })
            .await;
        }
        EffectName::ColorLoop => {
            let period = o.period.unwrap_or(60.0).clamp(10.0, 600.0);
            animate(effects, bulbs, id, start, COLOR_LOOP_HZ, None, |t, i, v| {
                // Spread bulbs apart a little so they don't all show the same color.
                let spread = i as f64 / n as f64;
                let (hue, saturation) = match o.color {
                    Some(c) => (
                        color_drift_hue(
                            t + period * spread / 4.0,
                            period,
                            f64::from(c.hue),
                            DRIFT_WIDTH,
                        ),
                        c.saturation,
                    ),
                    None => {
                        let base = if v.mode == Mode::Color {
                            f64::from(v.hue)
                        } else {
                            0.0
                        };
                        (color_loop_hue(t, period, base + 90.0 * spread), 100)
                    }
                };
                if last[i] == Some((saturation, hue)) {
                    return None;
                }
                last[i] = Some((saturation, hue));
                Some(Patch {
                    color: Some(HueSat { hue, saturation }),
                    ..Default::default()
                })
            })
            .await;
        }
        EffectName::Sunrise => {
            let length = o.minutes.unwrap_or(15.0).clamp(0.5, 120.0) * 60.0;
            let end = o.brightness.unwrap_or(100).clamp(1, 100);
            animate(
                effects,
                bulbs,
                id,
                start,
                FADE_HZ,
                Some(length),
                |t, i, _| {
                    let (b, k) = sunrise_at(t / length, end);
                    (last[i] != Some((b, k))).then(|| {
                        last[i] = Some((b, k));
                        Patch {
                            brightness: Some(b),
                            temperature: Some(k),
                            ..Default::default()
                        }
                    })
                },
            )
            .await;
        }
        EffectName::Sunset => {
            let length = o.minutes.unwrap_or(15.0).clamp(0.5, 120.0) * 60.0;
            animate(
                effects,
                bulbs,
                id,
                start,
                FADE_HZ,
                Some(length),
                |t, i, v| {
                    let start_k = if v.mode == Mode::White {
                        v.temperature
                    } else {
                        2700
                    };
                    let (b, k) = sunset_at(t / length, v.brightness, start_k);
                    (last[i] != Some((b, k))).then(|| {
                        last[i] = Some((b, k));
                        Patch {
                            brightness: Some(b),
                            temperature: Some(k),
                            ..Default::default()
                        }
                    })
                },
            )
            .await;
            let owned = effects.owned(id);
            if !owned.is_empty() {
                bulbs.command(
                    &owned,
                    Patch {
                        on: Some(false),
                        ..Default::default()
                    },
                );
            }
        }
        EffectName::Alert => {
            let color = o.color.unwrap_or(ALERT_RED);
            let bright = Patch {
                on: Some(true),
                brightness: Some(100),
                color: Some(color),
                temperature: None,
            };
            let dim = Patch {
                brightness: Some(15),
                ..Default::default()
            };
            for _ in 0..o.times.unwrap_or(3).clamp(1, 10) {
                for step in [&bright, &dim] {
                    let owned = effects.owned(id);
                    if owned.is_empty() {
                        return;
                    }
                    bulbs.command(&owned, step.clone());
                    sleep(ALERT_STEP).await;
                }
            }
            let owned = effects.owned(id);
            for v in start.iter().filter(|v| owned.contains(&v.ip)) {
                send(bulbs, &v.ip, restore_patch(v));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulbs::Status;

    fn view(ip: &str, on: bool, mode: Mode) -> BulbView {
        BulbView {
            ip: ip.into(),
            name: ip.into(),
            room: String::new(),
            model: None,
            firmware: None,
            status: Status::Online,
            error: None,
            on,
            brightness: 70,
            mode,
            hue: 200,
            saturation: 80,
            temperature: 3500,
            signal: None,
            effect: None,
        }
    }

    #[test]
    fn breathe_starts_at_peak_and_dips_at_half_period() {
        assert_eq!(breathe_level(0.0, 8.0, 10.0, 80.0), 80);
        assert_eq!(breathe_level(4.0, 8.0, 10.0, 80.0), 10);
        assert_eq!(breathe_level(8.0, 8.0, 10.0, 80.0), 80);
    }

    #[test]
    fn fades_hit_their_endpoints_and_round_kelvin() {
        assert_eq!(sunrise_at(0.0, 100), (1, 2500));
        assert_eq!(sunrise_at(1.0, 80), (80, 4000));
        assert_eq!(sunset_at(0.0, 60, 3000), (60, 3000));
        assert_eq!(sunset_at(1.0, 60, 3000), (1, 2500));
        assert_eq!(sunrise_at(0.37, 100).1 % 10, 0);
        // Brightness steps look even: half way is far below 50% raw brightness.
        assert!(sunrise_at(0.5, 100).0 < 30);
    }

    #[test]
    fn breathe_is_even_in_perceived_lightness() {
        // A quarter period in, perceived lightness is half way, so raw brightness
        // sits below the arithmetic midpoint.
        let quarter = breathe_level(2.5, 10.0, 20.0, 80.0);
        assert!(quarter > 20 && quarter < 50, "{quarter}");
    }

    #[test]
    fn drift_stays_within_its_width() {
        for step in 0..120 {
            let hue = color_drift_hue(f64::from(step) * 0.5, 60.0, 10.0, 45.0);
            assert!(hue <= 55 || hue >= 325, "{hue}");
        }
    }

    #[test]
    fn color_loop_wraps() {
        assert_eq!(color_loop_hue(0.0, 60.0, 350.0), 350);
        assert_eq!(color_loop_hue(30.0, 60.0, 350.0), 170);
        assert!(color_loop_hue(59.99, 60.0, 0.0) < 360);
    }

    #[test]
    fn restore_reproduces_mode_and_power() {
        let off = restore_patch(&view("a", false, Mode::White));
        assert_eq!((off.on, off.brightness), (Some(false), None));
        let color = restore_patch(&view("a", true, Mode::Color));
        assert_eq!(
            color.color,
            Some(HueSat {
                hue: 200,
                saturation: 80
            })
        );
        assert_eq!(color.temperature, None);
        let white = restore_patch(&view("a", true, Mode::White));
        assert_eq!((white.temperature, white.color), (Some(3500), None));
    }

    #[test]
    fn claims_release_and_stop() {
        let fx = Effects::new();
        let id = fx.claim(
            EffectName::Candle,
            &[view("a", true, Mode::White), view("b", false, Mode::White)],
        );
        assert_eq!(fx.owned(id).len(), 2);
        // A user change on "a" releases it without restoring.
        assert!(fx.release(&["a".into()]));
        assert_eq!(fx.owned(id), vec!["b".to_string()]);
        // Stopping "b" restores it (it was off).
        let restores = fx.stop(&["b".into()]);
        assert_eq!(restores.len(), 1);
        assert_eq!(restores[0].1.on, Some(false));
        assert!(fx.owned(id).is_empty());
        // Non-looping effects have nothing to restore on stop.
        fx.claim(EffectName::Sunrise, &[view("c", true, Mode::White)]);
        assert!(fx.stop(&["c".into()]).is_empty());
        assert!(fx.running().is_empty());
    }
}
