# Development notes

Design decisions behind TapOnLights, what has been verified on real hardware, and known gaps. Build instructions are in the [README](../README.md#building-from-source).

## Talking to the bulbs

- **Library:** the Rust [`tapo`](https://crates.io/crates/tapo) crate (0.12). It handles discovery of the bulb's protocol, the KLAP and TPAP handshakes, and encryption. The app uses its `ColorLightHandler` (`ApiClient::l530`) for every bulb, which is why only color bulbs are supported for now.
- **Why it's fast:** an earlier Python prototype (with `python-kasa`, still in the git history) took seconds per change. Each change did UDP discovery, a new sign-in, a full state read, the command, another full read and a disconnect, then refreshed every bulb one after another, with only one job allowed at a time across the app. The design below removes all of that.
- **One worker per bulb** (`src-tauri/src/bulbs.rs`) owns a signed-in session and receives messages: patch, refresh, reconnect, stop. When it wakes, it takes everything queued, merges the patches (newest wins; color and temperature cancel each other; `off` clears the rest) and sends them as one `set_device_info` request.
- **Instant UI:** `Manager::command` applies the patch to the shared state and notifies listeners before the worker sends anything. A state read that started before a command is discarded (a generation counter), so the UI doesn't jump back to old values.
- **Sessions:** the crate doesn't renew sessions on its own. If a request on an existing session fails with an HTTP or `Unauthorized` error, the worker signs in again once and retries. Timeouts: 4 s per request, 8 s per sign-in.
- **Polling:** every 10 s while a bulb is online, 15 s while it's offline. This also picks up changes made in the Tapo app.
- **Color mode:** the bulb reports `color_temp > 0` in white mode and `0` in color mode, and keeps its last hue and saturation either way.

## Effects

- **Where they run:** `src-tauri/src/effects.rs` computes effect frames in the app and sends them through `Manager::command`, the same path as user changes. The workers' latest-value merging means a slow bulb skips frames instead of lagging.
- **Smooth motion:** the Tapo local API has no transition time (the set request only carries power, brightness, hue/saturation and color temperature), so each change is a jump.
  - The first version sent 3 changes/s, and steps of about 13% were plainly visible.
  - Effects now send small steps: breathe up to 6/s, candle and color loop 4/s, sunrise and sunset at most 2/s, alert every 400 ms.
  - Values are only sent when they change, against a measured capacity of about 8/s.
  - Brightness is animated in perceived lightness (brightness^(1/2.2)), so steps look even, and kelvin moves in 10 K steps.
  - Measured on an L530 with the default 10 s breathe: about 2.5% per step (largest 4%), down from about 13% in the first version. Slow speed (16 s) roughly halves that again.
  - The remaining limit is 1% brightness resolution, which can show at the dim end.
- **Ownership:** a bulb belongs to at most one effect (`Effects` keeps a claim per IP).
  - `core::command`, used for every user change, scenes and the tray, releases claims first, so the effect stops and the new setting stays.
  - Stopping explicitly applies the restore patch saved when a looping effect started.
  - Starting a new effect stops the old one with a restore first, so the restored look becomes the base.
- **Experimental, confirmed once:** `settings.effects_acknowledged` gates `core::start_effect`. The UI shows a warning dialog on the first start, and the API returns 409 with the warning text until `POST /api/effects/acknowledge` is called. `/llms.txt` tells assistants to relay the warning and ask before acknowledging. The unknowns are long-run wear (whether the bulb writes its state to storage on every change) and stability over hours; only short runs were tested.
- **Not on the bulb:** the L530 has built-in effects of its own (`dynamic_light_effect_*` in its device info), but the `tapo` crate can only start effects on light strips. That's why these run on the PC.

## Desktop shell

- **Tray popup:** a frameless, always-on-top Tauri window with Mica (Windows 11), placed next to the tray icon using the click's position and the monitor's work area.
- **Hiding on click-away:** Tauri reported `Focused(true)` and then `Focused(false)` about 2 ms after `show()`, and again after hiding (probably WebView2 moving focus internally), so focus events can't be used. While the popup is open, the app checks every 120 ms which process owns the foreground window, and hides the popup once another app is in front, but only if the popup was in front first. If Windows refuses to bring it forward (for example, when started from a background process), the popup stays open until dismissed. A tray click within 400 ms of an auto-hide doesn't reopen it.
- **Theme:** `window.set_theme` follows the setting, so Mica and the WebView's `prefers-color-scheme` agree. Without Mica (the web page, Windows 10) the UI paints an opaque background.

## Web server

- `src-tauri/src/server.rs`, axum. It's off by default and binds to `127.0.0.1`, or `0.0.0.0` when network access is enabled. Turning it on or off, or changing it, stops the old server, including its event streams, and starts a new one.
- **Same UI files:** pages are served from the assets embedded in the app (`AssetResolver`), so the browser gets exactly what the popup shows. `src/lib/api.ts` picks Tauri IPC or HTTP and server-sent events at runtime.
- **Password:** HTTP Basic, checked against an Argon2 hash. A successful header is cached as a SHA-256 digest, so the slow Argon2 check runs once per client, and failed attempts wait 500 ms.
- **DNS rebinding:** requests whose `Host` is neither an IP address nor `localhost` get a 403. Commands require `Content-Type: application/json`, which cross-site pages can't send without a CORS preflight, and the server doesn't answer preflights.
- **Desktop-only settings:** from the web, settings updates may change bulbs, scenes and the theme only. Account, startup and web-access fields are ignored.
- **Strict API for scripts and AI assistants:** request bodies reject unknown fields (422), an empty patch or a target that matches no bulb returns 400 (the message lists the known bulbs), and unknown `/api/*` paths return 404 instead of the web page. Targets can be IPs, bulb names or rooms (`core::resolve_targets`).
- **AI guide:** `/llms.txt` serves `src-tauri/assets/llms.txt` with `{{BASE_URL}}` replaced by `http://<Host header>`. The Host header is already checked, so it's always an IP address or localhost. Keep that file in sync with the API; `docs/API.md` is the human-facing version.

## Settings and credentials

- `%APPDATA%\TapOnLights\settings.json` keeps unknown keys, so a newer version's settings survive opening an older one.
- The Tapo password is in Credential Manager under target `TapOnLights` (UTF-16, via `keyring` 3 with `Entry::new_with_target`).
- **Migration from the earlier name "Tapo Desktop":** on first run, settings and the credential (including the python-keyring layouts `Tapo Desktop` and `<user>@Tapo Desktop`) are copied over. The originals are left in place.

## Verified on hardware

On two L530 bulbs (hardware 2.0, firmware 1.4.4, Third-Party Compatibility on):

- With the probe: sign-in 80–110 ms, state read ~22 ms, brightness change 100–135 ms, values restored after the test.
- In the app's release build:
  - both bulbs online with the correct state;
  - the web server served the page with a Content Security Policy and returned 403 for a foreign `Host`;
  - a burst of 10 brightness commands was accepted in 6 ms, and a read-back from the bulb showed the new value;
  - the popup appeared next to the tray and stayed in front.

- **Sustained rate** (`tapo-probe --stress`, ±1% brightness, one request in flight per bulb, the app running at the same time):
  - **Back alone, 15 s:** 8.1 changes/s; latency p50 128 ms, p90 140, p99 205, max 231.
  - **Both bulbs at once, 20 s:** 8.1–8.2 changes/s each, so bulbs don't slow each other; p99 158–176 ms, max 224; no request over 500 ms, no timeouts.
- **Effects on an L530:**
  - breathe (4 s period) went 89 → 11 → 95% and Stop restored the exact earlier white;
  - alert pulsed, then restored;
  - a manual change released candle and kept the new setting;
  - a 30 s sunrise went from 1% at 2550 K to 100% at 4000 K, then cleared itself;
  - an unknown effect name returns 422 listing the valid ones.
- **One session per bulb (likely):** each stress run hit 1–2 `SESSION_TIMEOUT` errors, recovered by signing in again. The likely cause is the running app and the probe invalidating each other's sessions, which would mean a new sign-in ends the previous one. Expect brief hiccups if another client (Home Assistant, a second copy of the app) talks to the same bulbs.

## Not yet verified

- The color wheel and swatches on a bulb
- Opening the web page from another device on the network, and the password prompt in a browser
- Launch at sign-in, and the NSIS installer
- Windows 10, other bulb models, and Third-Party Compatibility turned off (TPAP)
