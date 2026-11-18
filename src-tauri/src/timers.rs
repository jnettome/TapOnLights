//! Timers: countdowns and clock-time alarms that either ring (a reminder) or
//! change the lights when they end.
//!
//! Deadlines are wall-clock times (unix ms) checked twice a second, so they hold
//! across sleep, resume and clock changes, unlike a monotonic sleep. Timers are
//! saved to `timers.json` next to the settings, so alarms survive a restart.
//!
//! A timer that comes due more than [`LATE_LIMIT_MS`] late (the PC was asleep or
//! off) never touches the lights: a reminder rings as "missed", and a light change
//! is dropped, so lights don't switch hours after they were meant to.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, LocalResult, NaiveTime, TimeDelta, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt as _;
use tokio::time::{MissedTickBehavior, interval};

use crate::{alarm, core};

/// How late a timer may fire and still act.
pub const LATE_LIMIT_MS: u64 = 2 * 60_000;
/// Longest countdown, and the furthest an extension may push a timer.
pub const MAX_DURATION_MS: u64 = 24 * 3_600_000;
pub const MAX_TIMERS: usize = 20;
pub const LABEL_MAX_CHARS: usize = 40;
pub const DEFAULT_FADE_MINUTES: f64 = 15.0;
const FILE_NAME: &str = "timers.json";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerKind {
    /// Runs for a duration.
    Countdown,
    /// Ends at a clock time.
    Alarm,
}

fn default_fade() -> f64 {
    DEFAULT_FADE_MINUTES
}

/// What happens when a timer ends. `flash` and `remind` ring (notification,
/// popup, until dismissed); the others change the lights silently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TimerAction {
    /// Ring, and pulse the lights (the alert effect).
    Flash,
    /// Ring without touching the lights.
    Remind,
    Off,
    On,
    /// Fade down, then off (the sunset effect, starting when the timer ends).
    Sunset {
        #[serde(default = "default_fade")]
        minutes: f64,
    },
    /// Fade up (the sunrise effect, starting when the timer ends).
    Sunrise {
        #[serde(default = "default_fade")]
        minutes: f64,
    },
    Scene {
        name: String,
    },
}

impl TimerAction {
    pub fn rings(&self) -> bool {
        matches!(self, Self::Flash | Self::Remind)
    }

    /// Effects are experimental and need the user's one-time OK.
    pub fn uses_effects(&self) -> bool {
        matches!(
            self,
            Self::Flash | Self::Sunset { .. } | Self::Sunrise { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub id: u64,
    #[serde(default)]
    pub label: String,
    pub kind: TimerKind,
    pub started_ms: u64,
    pub ends_ms: u64,
    pub action: TimerAction,
    /// Bulb IPs, names or room names, resolved when the timer ends; empty means every bulb.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Set while a reminder is ringing (until dismissed or snoozed).
    #[serde(default)]
    pub ringing_since_ms: Option<u64>,
    /// It came due while the PC was asleep or off.
    #[serde(default)]
    pub missed: bool,
}

/// A timer as the UI and API see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerView {
    #[serde(flatten)]
    pub timer: Timer,
    pub rings: bool,
}

impl From<Timer> for TimerView {
    fn from(timer: Timer) -> Self {
        Self {
            rings: timer.action.rings(),
            timer,
        }
    }
}

/// What the user asked for; see [`deadline`] for the rules.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimerInput {
    pub minutes: Option<f64>,
    pub seconds: Option<f64>,
    /// Local clock time, "HH:MM" (24-hour): the next time it comes round.
    pub at: Option<String>,
    #[serde(default)]
    pub label: String,
    /// Default: remind.
    pub action: Option<TimerAction>,
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Parse "7:30" or "07:30" (24-hour clock).
pub fn parse_at(text: &str) -> Option<NaiveTime> {
    let (h, m) = text.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

/// The next time the clock shows `time` after `now` (tomorrow if it's already
/// past). A time skipped by a DST change moves on by an hour.
pub fn next_occurrence<Tz: TimeZone>(now: &DateTime<Tz>, time: NaiveTime) -> DateTime<Tz> {
    let tz = now.timezone();
    let mut naive = now.date_naive().and_time(time);
    if naive <= now.naive_local() {
        naive += TimeDelta::days(1);
    }
    loop {
        match tz.from_local_datetime(&naive) {
            LocalResult::Single(t) => return t,
            LocalResult::Ambiguous(earliest, _) => return earliest,
            LocalResult::None => naive += TimeDelta::hours(1),
        }
    }
}

/// When a timer ends: `at` (a clock time) or `minutes` + `seconds` (a countdown, 1 s to 24 h).
pub fn deadline<Tz: TimeZone>(
    input: &TimerInput,
    now: &DateTime<Tz>,
) -> Result<(TimerKind, u64), String> {
    let now_ms = now.timestamp_millis().max(0) as u64;
    if let Some(at) = &input.at {
        if input.minutes.is_some() || input.seconds.is_some() {
            return Err("Give either “at” or minutes/seconds, not both".into());
        }
        let time = parse_at(at)
            .ok_or_else(|| format!("“{at}” isn't a time; use HH:MM on the 24-hour clock"))?;
        let ends = next_occurrence(now, time).timestamp_millis().max(0) as u64;
        return Ok((TimerKind::Alarm, ends));
    }
    if input.minutes.is_none() && input.seconds.is_none() {
        return Err("Say when: “minutes”, “seconds” or “at” (HH:MM)".into());
    }
    let total = input.minutes.unwrap_or(0.0) * 60.0 + input.seconds.unwrap_or(0.0);
    if !total.is_finite() || total < 1.0 || total * 1000.0 > MAX_DURATION_MS as f64 {
        return Err("Timers run from 1 second to 24 hours".into());
    }
    Ok((
        TimerKind::Countdown,
        now_ms + (total * 1000.0).round() as u64,
    ))
}

/// Trim, drop control characters, and check the length.
pub fn clean_label(label: &str) -> Result<String, String> {
    let label: String = label.trim().chars().filter(|c| !c.is_control()).collect();
    if label.chars().count() > LABEL_MAX_CHARS {
        return Err(format!(
            "Reminders can be up to {LABEL_MAX_CHARS} characters"
        ));
    }
    Ok(label)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// On time: run the action.
    Fire,
    /// Too late to act (the PC was asleep or off).
    Missed,
}

pub fn outcome(ends_ms: u64, now_ms: u64) -> Outcome {
    if now_ms.saturating_sub(ends_ms) > LATE_LIMIT_MS {
        Outcome::Missed
    } else {
        Outcome::Fire
    }
}

/// Local clock time of a unix-ms instant, "14:32".
pub fn clock_time(ms: u64) -> String {
    Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// The label, or a name made from the timer itself ("10-min timer", "Alarm 07:30").
pub fn title(t: &Timer) -> String {
    if !t.label.is_empty() {
        return t.label.clone();
    }
    match t.kind {
        TimerKind::Countdown => {
            let secs = t.ends_ms.saturating_sub(t.started_ms).div_ceil(1000);
            if secs < 60 {
                format!("{secs}-sec timer")
            } else {
                format!("{}-min timer", secs.div_ceil(60))
            }
        }
        TimerKind::Alarm => format!("Alarm {}", clock_time(t.ends_ms)),
    }
}

/// What a light-changing timer does, for messages.
fn action_text(action: &TimerAction) -> String {
    match action {
        TimerAction::Flash | TimerAction::Remind => "Reminder".into(),
        TimerAction::Off => "Lights off".into(),
        TimerAction::On => "Lights on".into(),
        TimerAction::Sunset { .. } => "Sunset".into(),
        TimerAction::Sunrise { .. } => "Sunrise".into(),
        TimerAction::Scene { name } => format!("Scene “{name}”"),
    }
}

/// Notification text for a reminder that has come due.
fn ring_text(t: &Timer, outcome: Outcome) -> String {
    let at = clock_time(t.ends_ms);
    match (outcome, t.kind) {
        (Outcome::Missed, _) => {
            format!("Missed: it was due at {at} while the PC was asleep or off")
        }
        (Outcome::Fire, TimerKind::Alarm) => format!("It's {at}"),
        (Outcome::Fire, TimerKind::Countdown) if t.label.is_empty() => "Time's up".into(),
        (Outcome::Fire, TimerKind::Countdown) => {
            let mut plain = t.clone();
            plain.label.clear();
            format!("{} is done", title(&plain))
        }
    }
}

/// Windows sounds for notifications (without one the toast is silent). On-time
/// rings use the looping alarm toast instead (see `alarm`); this chime is for
/// missed reminders and for when that toast can't be shown.
const RING_SOUND: &str = "Reminder";
const INFO_SOUND: &str = "Default";

fn notify(app: &AppHandle, title: &str, body: &str, sound: &str) {
    let shown = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .sound(sound)
        .show();
    if let Err(e) = shown {
        eprintln!("Could not show a notification: {e}");
    }
}

/// Check the timers twice a second and act on those that come due. `ring` is
/// called (once per check) when a reminder starts ringing, to bring up the popup,
/// and when the alarm notification is clicked.
pub fn spawn_scheduler(app: AppHandle, ring: fn(&AppHandle)) {
    tauri::async_runtime::spawn(async move {
        let mut tick = interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let due = core::state(&app).timers.take_due(now_ms());
            if due.is_empty() {
                continue;
            }
            let mut rang = false;
            for (timer, outcome) in &due {
                let result = core::run_timer_action(&app, timer, *outcome);
                if timer.action.rings() {
                    // A failed flash (bulbs offline) still rings.
                    rang = true;
                    let (name, text) = (title(timer), ring_text(timer, *outcome));
                    // On time: an alarm that loops until answered. Missed: one chime.
                    let alarmed =
                        *outcome == Outcome::Fire && alarm::show(&app, timer, &name, &text, ring);
                    if !alarmed {
                        notify(&app, &name, &text, RING_SOUND);
                    }
                } else if *outcome == Outcome::Missed {
                    notify(
                        &app,
                        "Timer skipped",
                        &format!(
                            "{} was due at {} while the PC was asleep or off",
                            action_text(&timer.action),
                            clock_time(timer.ends_ms)
                        ),
                        INFO_SOUND,
                    );
                } else if let Err(e) = result {
                    notify(
                        &app,
                        "Timer didn't run",
                        &format!("{}: {e}", action_text(&timer.action)),
                        INFO_SOUND,
                    );
                }
            }
            core::state(&app).bulbs.notify();
            if rang {
                ring(&app);
            }
        }
    });
}

/// Tray tooltip: the next timer, or a ringing one.
pub fn tooltip(timers: &[Timer]) -> String {
    const APP: &str = "TapOnLights";
    if let Some(t) = timers.iter().find(|t| t.ringing_since_ms.is_some()) {
        return format!("{APP} · {} is ringing", title(t));
    }
    match timers.iter().min_by_key(|t| t.ends_ms) {
        Some(t) => {
            let more = timers.len() - 1;
            let extra = if more > 0 {
                format!(" (+{more})")
            } else {
                String::new()
            };
            format!("{APP} · {} at {}{extra}", title(t), clock_time(t.ends_ms))
        }
        None => APP.into(),
    }
}

pub struct Timers {
    list: Mutex<Vec<Timer>>,
    next_id: AtomicU64,
    /// Where to save; `None` keeps them in memory only (tests).
    path: Option<PathBuf>,
}

impl Timers {
    pub fn new(list: Vec<Timer>, path: Option<PathBuf>) -> Self {
        let next = list.iter().map(|t| t.id).max().unwrap_or(0);
        Self {
            list: Mutex::new(list),
            next_id: AtomicU64::new(next),
            path,
        }
    }

    /// Timers saved by the last run (an unreadable file starts empty).
    pub fn load() -> Self {
        let path = crate::settings::app_dir().join(FILE_NAME);
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<Timer>>(&raw).ok())
            .unwrap_or_default();
        Self::new(list, Some(path))
    }

    fn save(&self, list: &[Timer]) {
        let Some(path) = &self.path else { return };
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let json = serde_json::to_string_pretty(list).map_err(std::io::Error::other)?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = result {
            eprintln!("Could not save timers: {e}");
        }
    }

    /// Ringing timers first, then by end time.
    pub fn list(&self) -> Vec<Timer> {
        let mut list = self.list.lock().unwrap().clone();
        list.sort_by_key(|t| (t.ringing_since_ms.is_none(), t.ends_ms, t.id));
        list
    }

    pub fn add(
        &self,
        kind: TimerKind,
        ends_ms: u64,
        now_ms: u64,
        label: String,
        action: TimerAction,
        targets: Vec<String>,
    ) -> Result<Timer, String> {
        let mut list = self.list.lock().unwrap();
        if list.len() >= MAX_TIMERS {
            return Err(format!(
                "Up to {MAX_TIMERS} timers at once; cancel one first"
            ));
        }
        let timer = Timer {
            id: self.next_id.fetch_add(1, Ordering::SeqCst) + 1,
            label,
            kind,
            started_ms: now_ms,
            ends_ms,
            action,
            targets,
            ringing_since_ms: None,
            missed: false,
        };
        list.push(timer.clone());
        self.save(&list);
        Ok(timer)
    }

    /// Cancel a timer, or dismiss it if it's ringing. Returns whether it existed.
    pub fn cancel(&self, id: u64) -> bool {
        let mut list = self.list.lock().unwrap();
        let before = list.len();
        list.retain(|t| t.id != id);
        let removed = list.len() != before;
        if removed {
            self.save(&list);
        }
        removed
    }

    /// Add time to a timer. A ringing timer is snoozed: it starts again, `ms` from now.
    pub fn extend(&self, id: u64, ms: u64, now_ms: u64) -> Option<Timer> {
        let mut list = self.list.lock().unwrap();
        let timer = list.iter_mut().find(|t| t.id == id)?;
        if timer.ringing_since_ms.is_some() {
            timer.ringing_since_ms = None;
            timer.missed = false;
            timer.kind = TimerKind::Countdown;
            timer.started_ms = now_ms;
            timer.ends_ms = now_ms + ms;
        } else {
            timer.ends_ms = (timer.ends_ms + ms).min(now_ms + MAX_DURATION_MS);
        }
        let timer = timer.clone();
        self.save(&list);
        Some(timer)
    }

    /// Timers that have come due: reminders start ringing and stay listed,
    /// light changes are removed. Each comes back with what to do about it.
    pub fn take_due(&self, now_ms: u64) -> Vec<(Timer, Outcome)> {
        let mut list = self.list.lock().unwrap();
        let mut due = Vec::new();
        list.retain_mut(|t| {
            if t.ringing_since_ms.is_some() || t.ends_ms > now_ms {
                return true;
            }
            let result = outcome(t.ends_ms, now_ms);
            let keep = t.action.rings();
            if keep {
                t.ringing_since_ms = Some(now_ms);
                t.missed = result == Outcome::Missed;
            }
            due.push((t.clone(), result));
            keep
        });
        if !due.is_empty() {
            self.save(&list);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
        FixedOffset::west_opt(3 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 17, h, m, 0)
            .unwrap()
    }

    fn input() -> TimerInput {
        TimerInput::default()
    }

    #[test]
    fn parses_clock_times() {
        assert_eq!(parse_at("7:30"), NaiveTime::from_hms_opt(7, 30, 0));
        assert_eq!(parse_at(" 07:05 "), NaiveTime::from_hms_opt(7, 5, 0));
        assert_eq!(parse_at("23:59"), NaiveTime::from_hms_opt(23, 59, 0));
        for bad in [
            "24:00", "7:3", "7", "7:60", ":30", "123:00", "ab:cd", "7:30pm",
        ] {
            assert_eq!(parse_at(bad), None, "{bad}");
        }
    }

    #[test]
    fn alarms_take_the_next_occurrence() {
        let now = at(10, 0);
        let later = next_occurrence(&now, parse_at("10:30").unwrap());
        assert_eq!(later, at(10, 30));
        // Already past (or exactly now): tomorrow.
        for past in ["09:00", "10:00"] {
            let t = next_occurrence(&now, parse_at(past).unwrap());
            assert_eq!(
                t.date_naive(),
                now.date_naive() + TimeDelta::days(1),
                "{past}"
            );
        }
    }

    #[test]
    fn deadlines_follow_the_rules() {
        let now = at(10, 0);
        let now_ms = now.timestamp_millis() as u64;

        let countdown = TimerInput {
            minutes: Some(1.5),
            seconds: Some(10.0),
            ..input()
        };
        assert_eq!(
            deadline(&countdown, &now),
            Ok((TimerKind::Countdown, now_ms + 100_000))
        );

        let alarm = TimerInput {
            at: Some("10:05".into()),
            ..input()
        };
        assert_eq!(
            deadline(&alarm, &now),
            Ok((TimerKind::Alarm, now_ms + 5 * 60_000))
        );

        let bad = [
            input(),
            TimerInput {
                seconds: Some(0.5),
                ..input()
            },
            TimerInput {
                minutes: Some(24.0 * 60.0 + 1.0),
                ..input()
            },
            TimerInput {
                minutes: Some(f64::NAN),
                ..input()
            },
            TimerInput {
                minutes: Some(5.0),
                at: Some("10:05".into()),
                ..input()
            },
            TimerInput {
                at: Some("noon".into()),
                ..input()
            },
        ];
        for b in bad {
            assert!(deadline(&b, &now).is_err(), "{b:?}");
        }
    }

    #[test]
    fn labels_are_trimmed_and_limited() {
        assert_eq!(clean_label("  Pasta\n "), Ok("Pasta".into()));
        assert_eq!(clean_label("a\u{7}b"), Ok("ab".into()));
        assert!(clean_label(&"x".repeat(LABEL_MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn late_timers_are_missed() {
        assert_eq!(outcome(1_000, 1_000), Outcome::Fire);
        assert_eq!(outcome(1_000, 1_000 + LATE_LIMIT_MS), Outcome::Fire);
        assert_eq!(outcome(1_000, 1_001 + LATE_LIMIT_MS), Outcome::Missed);
    }

    fn add(timers: &Timers, ends: u64, action: TimerAction) -> Timer {
        timers
            .add(TimerKind::Countdown, ends, 0, String::new(), action, vec![])
            .unwrap()
    }

    #[test]
    fn due_reminders_ring_and_light_changes_go() {
        let timers = Timers::new(vec![], None);
        let remind = add(&timers, 1_000, TimerAction::Remind);
        let off = add(&timers, 1_000, TimerAction::Off);
        let later = add(&timers, 9_000, TimerAction::Flash);

        assert!(timers.take_due(999).is_empty());
        let due = timers.take_due(1_000);
        let ids: Vec<u64> = due.iter().map(|(t, _)| t.id).collect();
        assert_eq!(ids, vec![remind.id, off.id]);
        assert!(due.iter().all(|(_, o)| *o == Outcome::Fire));

        let list = timers.list();
        assert_eq!(
            list.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![remind.id, later.id],
            "ringing first, light change removed"
        );
        assert_eq!(list[0].ringing_since_ms, Some(1_000));
        // A ringing timer isn't due again.
        assert!(timers.take_due(5_000).is_empty());

        // Far too late: missed.
        let due = timers.take_due(9_000 + LATE_LIMIT_MS + 1);
        assert_eq!(due[0].1, Outcome::Missed);
        assert!(timers.list().iter().any(|t| t.id == later.id && t.missed));
    }

    #[test]
    fn extend_adds_time_and_snoozes_ringing_timers() {
        let timers = Timers::new(vec![], None);
        let t = add(&timers, 60_000, TimerAction::Remind);
        assert_eq!(
            timers.extend(t.id, 60_000, 10_000).unwrap().ends_ms,
            120_000
        );
        // Capped at 24 h from now.
        assert_eq!(
            timers
                .extend(t.id, MAX_DURATION_MS, 10_000)
                .unwrap()
                .ends_ms,
            10_000 + MAX_DURATION_MS
        );

        let t = add(&timers, 1_000, TimerAction::Remind);
        timers.take_due(1_000);
        let snoozed = timers.extend(t.id, 300_000, 2_000).unwrap();
        assert_eq!(snoozed.ringing_since_ms, None);
        assert_eq!((snoozed.started_ms, snoozed.ends_ms), (2_000, 302_000));

        assert!(timers.extend(999, 1, 0).is_none());
        assert!(timers.cancel(t.id));
        assert!(!timers.cancel(t.id));
    }

    #[test]
    fn timer_limit() {
        let timers = Timers::new(vec![], None);
        for _ in 0..MAX_TIMERS {
            add(&timers, 1, TimerAction::Remind);
        }
        assert!(
            timers
                .add(
                    TimerKind::Countdown,
                    1,
                    0,
                    String::new(),
                    TimerAction::Remind,
                    vec![]
                )
                .is_err()
        );
    }

    #[test]
    fn actions_round_trip_as_json() {
        let a: TimerAction = serde_json::from_str(r#"{"type":"sunset"}"#).unwrap();
        assert_eq!(
            a,
            TimerAction::Sunset {
                minutes: DEFAULT_FADE_MINUTES
            }
        );
        let a: TimerAction = serde_json::from_str(r#"{"type":"scene","name":"Relax"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"type":"scene","name":"Relax"}"#
        );
        assert!(TimerAction::Flash.rings() && !TimerAction::Off.rings());
    }

    #[test]
    fn titles_name_untitled_timers() {
        let mut t = Timer {
            id: 1,
            label: String::new(),
            kind: TimerKind::Countdown,
            started_ms: 0,
            ends_ms: 10 * 60_000,
            action: TimerAction::Remind,
            targets: vec![],
            ringing_since_ms: None,
            missed: false,
        };
        assert_eq!(title(&t), "10-min timer");
        t.ends_ms = 30_000;
        assert_eq!(title(&t), "30-sec timer");
        t.label = "Pasta".into();
        assert_eq!(title(&t), "Pasta");
    }
}
