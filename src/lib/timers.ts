// Display helpers for timers. Times are unix ms on the PC's clock; pass a `now`
// corrected with the snapshot's nowMs so phones with a skewed clock agree.
import type { Bulb, Timer, TimerAction } from "./types";

/** "7:42" or "1:02:03". Rounds up, so a timer shows 0:01 until it rings. */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Local clock time, "14:32". */
export function clockTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** The label, or a name made from the timer (same rules as the app's notifications). */
export function timerTitle(t: Timer): string {
  if (t.label) return t.label;
  if (t.kind === "alarm") return `Alarm ${clockTime(t.endsMs)}`;
  const secs = Math.ceil((t.endsMs - t.startedMs) / 1000);
  return secs < 60 ? `${secs}-sec timer` : `${Math.ceil(secs / 60)}-min timer`;
}

export function actionLabel(a: TimerAction): string {
  switch (a.type) {
    case "flash":
      return "Flash";
    case "remind":
      return "Remind";
    case "off":
      return "Lights off";
    case "on":
      return "Lights on";
    case "sunset":
      return `Sunset, ${a.minutes} min`;
    case "sunrise":
      return `Sunrise, ${a.minutes} min`;
    case "scene":
      return a.name;
  }
}

/** Readable targets: bulb names, room names, or "All lights". */
export function targetsLabel(targets: string[], bulbs: Bulb[]): string {
  if (targets.length === 0) return "All lights";
  return targets
    .map((t) => bulbs.find((b) => b.ip === t || b.name.toLowerCase() === t.toLowerCase())?.name ?? t)
    .join(", ");
}

/** Countdown progress, 0–1. */
export function progress(t: Timer, now: number): number {
  const span = t.endsMs - t.startedMs;
  return span > 0 ? Math.min(1, Math.max(0, (now - t.startedMs) / span)) : 1;
}
