# Web API

When the web page is on, TapOnLights serves a small JSON API next to it. The web page uses this API, and you can use it from scripts, home automation, a Stream Deck or an AI assistant. Anything the app's own controls can do, the API can do.

- [Turning it on](#turning-it-on)
- [Requests](#requests)
- [Endpoints](#endpoints)
- [Choosing bulbs: targets](#choosing-bulbs-targets)
- [Changing lights: the patch](#changing-lights-the-patch)
- [Effects (experimental)](#effects-experimental)
- [Alerts and notifications](#alerts-and-notifications)
- [The snapshot](#the-snapshot)
- [Live updates](#live-updates)
- [Errors](#errors)
- [Examples](#examples): curl, PowerShell, Python, Home Assistant
- [Control it with an AI assistant](#control-it-with-an-ai-assistant)

## Turning it on

In the app, open Settings › **Web page**:

- **Control lights from a browser:** turns the server on, at `http://localhost:4530` (the port can be changed).
- **Allow other devices on my network:** also serves it on the PC's network address (Settings shows the URL). Windows Firewall asks the first time; allow private networks.
- **Password (optional):** clients must use HTTP Basic auth, with any username and this password.

The server only answers requests addressed to an IP address or `localhost`; any other `Host` gets 403, which blocks DNS-rebinding attacks. It is plain HTTP: don't expose it to the internet, and use network access only on networks you trust.

## Requests

- **Base URL:** `http://localhost:4530` on the same PC, or `http://<pc-ip>:4530` from other devices.
- **Request bodies** are JSON and need `Content-Type: application/json`.
- **Successful changes** return `204 No Content`, and the new state is in the snapshot right away. The bulb follows in about 0.1–0.2 s.
- **Password:** `curl -u "any:yourpassword" …`, or an `Authorization: Basic …` header.

## Endpoints

| Method | Path | Body | Response |
|---|---|---|---|
| `GET` | `/api/snapshot` | — | `200`: [snapshot](#the-snapshot) |
| `GET` | `/api/events` | — | Server-sent events: a `snapshot` event on connect and on every change |
| `POST` | `/api/command` | `{"targets": [...], "patch": {...}}` | `204` |
| `POST` | `/api/scenes/apply` | `{"name": "Relax", "targets": [...]}` | `204` |
| `POST` | `/api/scenes` | `{"name": "Movie", "scene": {"brightness": 20, "color": null, "temperature": 2700}}` | `204`; saves or replaces a custom scene |
| `DELETE` | `/api/scenes/{name}` | — | `204`; deletes a custom scene (built-in scenes can't be deleted) |
| `POST` | `/api/effects/start` | `{"name": "candle", "targets": [...], "options": {...}}` | `204`; see [Effects](#effects-experimental). `409` until effects have been confirmed once. |
| `POST` | `/api/effects/acknowledge` | — | `204`; records that the user accepted the experimental-effects warning |
| `POST` | `/api/effects/stop` | `{"targets": [...]}` | `204`; looping effects put the lights back to how they were |
| `POST` | `/api/refresh` | — | `204`; re-reads every bulb now instead of waiting for the next 10 s poll |
| `GET` | `/api/settings` | — | `200`: bulbs (`ip`, `name`, `room`), theme and other preferences |
| `PUT` | `/api/settings` | `{"devices": [{"ip", "name", "room"}, ...], "theme": "system"}` | `200`; **replaces the whole bulb list**. `theme` is `system`, `light` or `dark`. |
| `GET` | `/llms.txt` | — | A plain-text guide for AI assistants, with this server's address filled in |

For a saved scene, set either `color` (`{"hue", "saturation"}`) or `temperature`, and the other to `null`.

The Tapo account, startup options and web-access settings can't be read or changed through the API. Change them in the app.

## Choosing bulbs: targets

`targets` is a list. Each entry can be:

1. a bulb's IP address, like `"192.168.1.42"`;
2. a bulb's name, like `"Desk"` (case-insensitive);
3. a room name, like `"Office"`, meaning every bulb in that room.

Entries are matched in that order, so a bulb named the same as a room wins. An empty list, or leaving `targets` out, means **all bulbs**. An entry that matches nothing returns `400`, and the message lists the known bulbs.

## Changing lights: the patch

| Field | Type | Range | Notes |
|---|---|---|---|
| `on` | boolean | | `false` turns the bulb off and ignores the other fields |
| `brightness` | integer | 1–100 | Percent. Turns the bulb on. |
| `temperature` | integer | 2500–6500 | Kelvin. White mode. Turns the bulb on. |
| `color` | object | `hue` 0–360, `saturation` 1–100 | Color mode. Turns the bulb on. |

- At least one field is required.
- Out-of-range numbers are clamped, and unknown fields are rejected.
- `color` and `temperature` are alternatives; if you send both, `color` wins.

Rapid requests (for example, a slider drag or a fade) are merged per bulb: while one request is on its way to the bulb, newer changes are combined, and only the latest values are sent. For visible steps in a fade, space requests at least 150 ms apart.

Useful values:

- **White (kelvin):** candlelight 2500, warm 2700, soft 3000, neutral 4000, cool 5000, daylight 6500.
- **Hue:** red 0, orange 25, amber 40, yellow 55, lime 90, green 120, teal 170, cyan 185, blue 220, indigo 250, purple 275, magenta 300, pink 330.
- **Saturation:** 100 for vivid colors, 40–70 for pastels.

## Effects (experimental)

> **Experimental.** Tapo bulbs like the L530 can't fade on their own, so effects work by sending a stream of small changes, up to 6 per second per bulb, over your Wi-Fi.
>
> - **Tested:** short runs on an L530; the bulbs handled about 8 changes per second without errors.
> - **Not tested:** hours of continuous use. Whether that wears the bulb (for example, if it saves its state on every change) isn't known.
> - **Flashing:** `alert` flashes the lights. Avoid it around anyone sensitive to flashing light.

**Confirming once.** The first time anyone starts an effect, the app shows this warning and asks for confirmation. The answer is saved in the settings, so it's asked once per PC. Until then, `POST /api/effects/start` returns `409` with an explanation. Scripts and AI assistants should show the user the warning, and then, if the user agrees, call `POST /api/effects/acknowledge` once. The snapshot's `effectsAcknowledged` field tells you whether that has happened.

Effects run inside the app. It computes each step and sends it to the bulbs, so nothing needs to stay connected to the API.

| `name` | What it does | `options` (all optional) |
|---|---|---|
| `breathe` | Brightness rises and falls slowly | `color` (default: the light's current look), `period` seconds per breath (3–60, default 10), `brightness` peak |
| `candle` | Soft, flickering flame | `color` (default flame orange), `brightness` base level (default 60) |
| `colorLoop` | Drifts around the whole color wheel, or ±45° around `color` if given; multiple bulbs are spread a little apart | `color`, `period` seconds per loop (10–600, default 60) |
| `sunrise` | Fades up from 1% at 2500 K to full brightness at 4000 K, then stays there | `minutes` (0.5–120, default 15), `brightness` final level (default 100) |
| `sunset` | Fades down to 1% at 2500 K, then turns the lights off | `minutes` (0.5–120, default 15) |
| `alert` | Pulses a color a few times, then puts the lights back to how they were | `color` (default red), `times` (1–10, default 3) |

`color` is `{"hue": 0–360, "saturation": 1–100}`, the same as in a patch.

```sh
curl -X POST http://localhost:4530/api/effects/start -H "Content-Type: application/json" \
     -d '{"name": "sunrise", "targets": ["Bedroom"], "options": {"minutes": 20}}'

curl -X POST http://localhost:4530/api/effects/start -H "Content-Type: application/json" \
     -d '{"name": "alert", "options": {"color": {"hue": 120, "saturation": 100}, "times": 2}}'  # green double pulse, all bulbs

curl -X POST http://localhost:4530/api/effects/start -H "Content-Type: application/json" \
     -d '{"name": "breathe", "targets": ["Desk"], "options": {"color": {"hue": 220, "saturation": 90}}}'  # breathe in blue
```

How effects behave:

- **One effect per bulb.** Starting one replaces whatever ran there. If the old one was a looping effect, the bulb first goes back to its earlier look, which becomes the starting point.
- **A manual change stops the effect on that bulb.** Any change from the app, the web page, a scene or `/api/command` stops it right there and keeps the new setting.
- **Stopping** with `/api/effects/stop`, or the Stop button, puts bulbs running `breathe`, `candle` or `colorLoop` back to how they were before. `sunrise` and `sunset` stay where they are.
- **Only online bulbs take part.** If none of the targets is online, the request returns `400`.
- **What's running** is each bulb's `effect` field in the snapshot (`null` when none).
- **Speed:** in the app, the Speed choice (Slow, Normal, Fast) sets `period` for `breathe` (16, 10 or 6 s) and `colorLoop` (120, 60 or 30 s). Slower means smaller steps, so smoother motion.
- **Smoothness:** Tapo bulbs have no transition time in their local API, so every change is a jump. Effects use small, frequent steps instead: up to 6 changes per second per bulb for `breathe` and 4 for the others (an L530 accepts about 8), and nothing is sent when a value doesn't change. Brightness moves in steps that look even to the eye. At the dim end, single 1% steps can still be visible, because 1% is the smallest step the bulbs accept.

## Alerts and notifications

`alert` is the most useful effect for scripts: it gets your attention, then puts every light back exactly as it was, including turning it off again if it was off.

**One-time setup:** turn on the web page, and confirm effects once, either by starting any effect in the app or with `POST /api/effects/acknowledge`.

### Suggested colors

| Meaning | Color | `color` option |
|---|---|---|
| Success, done | green | `{"hue": 120, "saturation": 100}` |
| Failure, error | red | `{"hue": 0, "saturation": 100}` (the default) |
| Warning, attention | amber | `{"hue": 40, "saturation": 100}` |
| Information, someone's here | blue | `{"hue": 220, "saturation": 100}` |

Two or three pulses (`times`) are enough; an alert takes about 0.8 s per pulse.

### From a terminal

```sh
# bash: flash green if the last command succeeded, red if it failed
flash() {
  local hue=0; [ "${1:-$?}" -eq 0 ] && hue=120
  curl -s -X POST http://localhost:4530/api/effects/start -H "Content-Type: application/json" \
       -d "{\"name\": \"alert\", \"options\": {\"color\": {\"hue\": $hue, \"saturation\": 100}, \"times\": 2}}"
}
npm test; flash $?
```

```powershell
# PowerShell: add to your $PROFILE
function Flash-Lights([int]$Hue = 0, [int]$Times = 2, [string[]]$Targets = @()) {
  $body = @{ name = "alert"; targets = $Targets; options = @{ color = @{ hue = $Hue; saturation = 100 }; times = $Times } }
  Invoke-RestMethod -Method Post -Uri http://localhost:4530/api/effects/start `
    -ContentType "application/json" -Body ($body | ConvertTo-Json -Depth 4) | Out-Null
}
dotnet build; if ($?) { Flash-Lights -Hue 120 } else { Flash-Lights }   # green or red
```

### From Home Assistant (for example, a doorbell)

```yaml
rest_command:
  lights_alert_blue:
    url: "http://192.168.1.20:4530/api/effects/start"
    method: post
    content_type: "application/json"
    payload: '{"name": "alert", "targets": ["Office"], "options": {"color": {"hue": 220, "saturation": 100}, "times": 3}}'

automation:
  - alias: "Doorbell flashes the office lights"
    trigger:
      - platform: state
        entity_id: binary_sensor.doorbell
        to: "on"
    action:
      - service: rest_command.lights_alert_blue
```

### Prompts for an AI assistant

An assistant that runs commands on your computer, such as Claude Code, can use alerts to tell you when it's done. Paste the prompt from Settings › Web page › **Copy prompt for an AI assistant** first (or the [template](#control-it-with-an-ai-assistant)), then ask, for example:

```text
When you finish this task, run the tests and flash my office lights: green twice if
everything passes, red three times if anything fails.
```

```text
Watch the deploy (gh run watch) and when it ends, flash all lights blue if it succeeded
or red if it failed.
```

```text
Create a Windows scheduled task that flashes the office lights amber at 18:00 on weekdays,
using the TapOnLights API, so I remember to stop working.
```

```text
Every 25 minutes until I tell you to stop, flash the desk lamp green once (Pomodoro breaks).
```

If effects haven't been confirmed yet, the assistant gets a `409` that explains the warning. The guide at `/llms.txt` tells it to show you the warning and ask before it confirms.

## The snapshot

```json
{
  "bulbs": [
    {
      "ip": "192.168.1.42",
      "name": "Desk",
      "room": "Office",
      "model": "L530",
      "firmware": "1.4.4",
      "status": "online",
      "error": null,
      "on": true,
      "brightness": 80,
      "mode": "white",
      "hue": 145,
      "saturation": 75,
      "temperature": 4000,
      "signal": 2,
      "effect": null
    }
  ],
  "scenes": [
    { "name": "Focus", "builtin": true, "brightness": 95, "temperature": 4200, "color": null },
    { "name": "Dusk", "builtin": true, "brightness": 48, "temperature": null, "color": { "hue": 20, "saturation": 85 } }
  ],
  "theme": "system",
  "web": { "enabled": true, "running": true, "lan": false, "hasPassword": false,
           "localUrl": "http://localhost:4530", "lanUrl": null, "error": null },
  "hasAccount": true,
  "temperatureRange": [2500, 6500],
  "backdrop": true,
  "effectsAcknowledged": false
}
```

- **`status`:** `online`, `connecting`, `offline` (unreachable) or `authError` (sign-in rejected). `error` explains any status other than `online`. Bulbs that aren't online don't respond until the app reconnects, which it retries every 15 s.
- **`mode`:** whether the bulb shows white (`temperature` applies) or color (`hue` and `saturation` apply). The inactive values are the last ones used.
- **`signal`:** Wi-Fi signal level reported by the bulb.
- **`effect`:** the [effect](#effects-experimental) running on the bulb (`breathe`, `candle`, `colorLoop`, `sunrise`, `sunset` or `alert`), or `null`.
- **`effectsAcknowledged`:** whether the one-time [experimental-effects](#effects-experimental) warning has been accepted on this PC.
- **`backdrop`:** whether the desktop popup uses the Windows 11 background; scripts can ignore it.

## Live updates

`GET /api/events` is a [server-sent events](https://developer.mozilla.org/docs/Web/API/Server-sent_events) stream. You get a `snapshot` event with the full state when you connect, and again after every change, from any source: the app, the web page, the API, or a poll that picked up changes from the Tapo phone app. Bursts are combined into one event.

```sh
curl -N http://localhost:4530/api/events
```

```
event: snapshot
data: {"bulbs":[...],"scenes":[...],...}
```

## Errors

| Status | When |
|---|---|
| `400` | Plain-text reason: a target that matches no bulb or room, an unknown scene, an empty patch, an invalid IP address in settings, … |
| `401` | A password is set and was missing or wrong |
| `403` | The URL's host isn't an IP address or `localhost` |
| `404` | Unknown `/api/...` path |
| `409` | Starting an effect before effects have been confirmed once (see [Effects](#effects-experimental)) |
| `415` | Missing `Content-Type: application/json` |
| `422` | JSON with the wrong shape or an unknown field; the message names it |

## Examples

### curl

```sh
BASE=http://localhost:4530

# All lights off
curl -X POST $BASE/api/command -H "Content-Type: application/json" \
     -d '{"patch": {"on": false}}'

# Office: warm and dim
curl -X POST $BASE/api/command -H "Content-Type: application/json" \
     -d '{"targets": ["Office"], "patch": {"brightness": 30, "temperature": 2700}}'

# One bulb purple
curl -X POST $BASE/api/command -H "Content-Type: application/json" \
     -d '{"targets": ["Desk"], "patch": {"color": {"hue": 275, "saturation": 90}}}'

# Apply a scene
curl -X POST $BASE/api/scenes/apply -H "Content-Type: application/json" \
     -d '{"name": "Relax"}'

# Who's on?
curl -s $BASE/api/snapshot | jq '.bulbs[] | {name, on, brightness}'
```

### PowerShell

In Windows PowerShell, `curl` is an alias for `Invoke-WebRequest`. Use `curl.exe`, or:

```powershell
$base = "http://localhost:4530"
Invoke-RestMethod -Method Post -Uri "$base/api/command" -ContentType "application/json" `
  -Body (@{ targets = @("Office"); patch = @{ brightness = 60; temperature = 4000 } } | ConvertTo-Json -Depth 3)

(Invoke-RestMethod "$base/api/snapshot").bulbs | Format-Table name, room, on, brightness, mode
```

With a password: add `-Headers @{ Authorization = "Basic " + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("me:yourpassword")) }`.

### Python (standard library only)

```python
import json, time, urllib.request

BASE = "http://localhost:4530"

def command(patch, targets=()):
    req = urllib.request.Request(
        f"{BASE}/api/command", method="POST",
        data=json.dumps({"targets": list(targets), "patch": patch}).encode(),
        headers={"Content-Type": "application/json"})
    urllib.request.urlopen(req)

# Fade the office down over about 3 seconds
for level in range(100, 9, -6):
    command({"brightness": level}, ["Office"])
    time.sleep(0.2)
```

### Home Assistant

```yaml
# configuration.yaml
rest_command:
  office_relax:
    url: "http://192.168.1.20:4530/api/scenes/apply"
    method: post
    content_type: "application/json"
    payload: '{"name": "Relax", "targets": ["Office"]}'
    # username: any
    # password: !secret taponlights_password
```

## Control it with an AI assistant

An AI assistant that can send HTTP requests on your network can control the lights in plain language: "dim the office to 30% and make it warm", "turn everything off at 11pm", "slowly fade the desk lamp to blue".

**Which assistants can do it:** ones that run on your computer or network and can make HTTP requests, such as [Claude Code](https://claude.com/claude-code) or a desktop assistant with a shell or fetch tool. Chat assistants in a browser run in the cloud and can't reach your home network. Don't open the port to the internet to make that work.

1. Turn on the web page (and *Allow other devices on my network* if the assistant runs on another machine).
2. In Settings › Web page, click **Copy prompt for an AI assistant**. It copies a prompt with your address filled in. Or paste this template and fill it in yourself:

   ```text
   You can control my smart lights through TapOnLights, an app on my home network.
   Its API is at http://<PC-ADDRESS>:4530. First read http://<PC-ADDRESS>:4530/llms.txt: it explains everything the API can do and how to use it.
   Then do what I ask by sending HTTP requests (for example with curl).
   No password is needed.            ← or: It uses HTTP Basic auth: any username, password: <PASSWORD>

   My request: make the office warm and cozy, around 40%.
   ```

3. The assistant reads `/llms.txt`, a guide the app serves with the endpoints, the patch format, color and white values, timing and error handling. It then checks `/api/snapshot` and sends commands.

Things to know:

- **The password goes into the chat.** If you include it, the assistant and its provider see it. Use a password you don't use anywhere else.
- **Settings are off-limits by default.** The guide tells assistants not to change settings or delete scenes unless you ask. The API can't touch your Tapo account or the web-access settings at all.
- **Automations** ("every evening at sunset…") need something that keeps running, such as Windows Task Scheduler, Home Assistant or cron. Ask the assistant to write that script for you.
