# Tapo Desktop

A lightweight Windows tray controller for TP-Link Tapo smart bulbs that talks to the bulbs directly on the local network.

## Tauri app (current): `desktop/`

Rust core ([`tapo`](https://crates.io/crates/tapo) crate) with a Svelte + TypeScript UI in a Tauri 2 tray popup. The same UI can also be served as a web page from this PC.

- **Instant changes:** each bulb keeps one open session. Commands update the UI at once and go out in a single request; values queued during a slider drag are merged so only the latest is sent. Measured on the L530s: login ~80–110 ms, state read ~22 ms, brightness change ~100–135 ms.
- **Same settings and login as the Python app:** reads `%APPDATA%\Tapo Desktop\settings.json` and the password the Python app stored in Windows Credential Manager.
- **Optional web page** (Settings › Web page): off by default, this PC only unless "Allow other devices on my network" is on, optional HTTP password (stored as an Argon2 hash). Account, startup and web-access settings can only be changed in the desktop app. Requests whose Host isn't an IP address or `localhost` are refused (DNS-rebinding protection).
- **Size:** 3.6 MB installer / 13.5 MB exe, using the WebView2 built into Windows 11 (the PyInstaller bundle was 181 MB).

Build (Windows, needs Rust, Node and the MSVC build tools):

```powershell
cd desktop
npm install
npx tauri build          # exe: src-tauri\target\release\tapo-desktop.exe
                         # installer: src-tauri\target\release\bundle\nsis\
npx tauri dev            # run with hot reload
npm run check            # Svelte/TypeScript checks
cd src-tauri; cargo test
```

The tray icon's left click toggles the popup; right click has all on/off, the web page, settings and quit. `spikes/tapo-probe` is the latency/compatibility probe used to choose the `tapo` crate.

## Python app (original)

The sections below cover the original PySide6 + `python-kasa` app, kept for reference.

## Compatibility status

The current `python-kasa` supported-device documentation lists the Tapo L530E, including tested hardware/firmware combinations. This project requires python-kasa 0.12 or newer; its async API provides UDP discovery and direct-IP connection, and devices on its newer protocol may require the Tapo account email and password. Device capabilities must be read from the connected device rather than assumed.

Phase 1 code in this repository provides a discovery/probe utility. The development environment has Python 3.14.4 and python-kasa 0.12.0 installed. Before enabling Tapo's Third-Party Compatibility setting, both local L530 bulbs responded with `SMART.TAPOBULB` and TPAP encryption (level 2), which python-kasa 0.12.0 rejects before authentication. After enabling the setting and supplying Tapo credentials, both bulbs authenticated and completed `device.update()`. The probe formatter and async cleanup were corrected for python-kasa 0.12.0's typed `DeviceInfo` and async disconnect API. The L530 at `192.168.1.86` now reports hardware 2.0, firmware 1.4.4, power on, brightness, HSV, color-temperature, and effect features.

## Phase 2: command-line control

With `KASA_USERNAME` set in PowerShell, the control CLI prompts for the password without echoing it:

```powershell
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 state
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 on
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 off
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 brightness 45
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 color 210 80 70
& '.venv314\Scripts\python.exe' -m scripts.control --ip 192.168.1.86 temperature 3000
```

The CLI checks advertised features and color-temperature limits before sending a command. It uses python-kasa's public `Light` module and serializes commands per bulb. Real authenticated state reads are confirmed; each write command still needs a real-device run before it can be claimed verified.

## Requirements

- Python 3.12 or newer
- A Tapo bulb and computer on the same local network
- Tapo account credentials if the device requires authentication

## Install

```powershell
py -3.12 -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install -e ".[dev]"
```

If the Python launcher does not recognize an installed version, call that installation directly, for example `& 'C:\Path\To\Python314\python.exe' -m venv .venv314`. Python 3.14.4 and python-kasa 0.12.0 were used for the device probe in this project.

## Phase 1: discover and probe

```powershell
python -m scripts.discover
python -m scripts.discover --ip 192.168.1.42
```

Provide credentials through `KASA_USERNAME` and `KASA_PASSWORD` environment variables, or pass `--username` (the password is then requested without echo). Avoid putting passwords in shell history. Discovery broadcasts on the LAN and reports detected name, model, online state and advertised feature IDs. Direct-IP mode uses python-kasa's device/protocol detection.

The output distinguishes authentication, unsupported-device, timeout and connectivity failures. A successful probe should report model `L530`/`L530E` and features before the UI is built. If discovery fails on a multi-interface computer, verify the active LAN and firewall settings; python-kasa's CLI also supports choosing a broadcast target.

## Development

```powershell
python -m pip install -e ".[dev]"
pytest
```

## Desktop tray app

Install the UI and packaging dependencies, then run the app:

```powershell
& '.venv314\Scripts\python.exe' -m pip install -e ".[desktop]"
& '.venv314\Scripts\python.exe' -m app.main
```

The first run opens Settings. Add each bulb by IP, assign an optional name and room, and provide the Tapo account email/password. Passwords are stored through the system keyring (Windows Credential Manager on Windows); the settings JSON contains no password. Closing hides the popup to the tray; use the tray menu's Quit action to exit.

Build a Windows executable after installing the desktop extra with `./build.ps1`.

The Qt popup was launched and visually inspected; a first-run settings layering issue found during inspection was fixed before packaging. The bundled executable launches and loads Qt (see the QtCore DLL note in the development notes); real-device write commands have not been sent. A successful authenticated state read from the L530 at `192.168.1.86` is verified. Never commit credentials or local settings.

For probe/API quirks, Windows setup notes, and build findings, see [docs/development-notes.md](docs/development-notes.md).
