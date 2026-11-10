# Development notes

This document records findings from the first real-device probe and the initial Windows build. Do not add account passwords or other credentials here. Observed LAN addresses are lab details and may change.

## Verified hardware and connection path

- Two Tapo bulbs were visible in the EdgeRouter DHCP leases as `L530` at `192.168.1.85` and `192.168.1.86`.
- A broadcast discovery scan returned no supported devices in this Windows environment, but direct-IP probing reached both bulbs. If LAN discovery misses a bulb, try direct-IP mode before concluding that it is offline.
- Before Tapo **Third-Party Compatibility** was enabled, both bulbs advertised `SMART.TAPOBULB` with `TPAP` encryption, level 2. python-kasa 0.12.0 rejected this transport before authentication. Its upstream TPAP implementation was still an open pull request at the time of the probe.
- After that setting was enabled, python-kasa proceeded to authentication. With the Tapo account credentials supplied locally, both bulbs completed `device.update()` successfully.
- The bulb at `192.168.1.86` reported `TapoBack — L530`, power on, hardware `2.0`, firmware `1.4.4`. Advertised features included brightness, HSV, color temperature, light effects, presets, device time, signal level, and firmware information. This confirms real authenticated state reads for this hardware/firmware pair.
- TP-Link warns that Third-Party Compatibility can reduce device security and does not guarantee third-party integrations. The user enabled it for this setup.
- No real-device write command (power, brightness, color, temperature, or scene) has been sent yet. Keep that distinction in release notes until those operations are tried.

## python-kasa 0.12.0 API details

- The installed release uses `AuthenticationError` and `UnsupportedDeviceError` from `kasa.exceptions`; older examples may use exception class names that do not import in 0.12.0.
- `device.device_info` is a typed `kasa.device.DeviceInfo` object. Use fields such as `hardware_version`, `firmware_version`, and `device_type`; it is not a dictionary and has no `.get()` method.
- `Device.disconnect()` is asynchronous in this release and must be awaited, including in `finally` blocks.
- Bulb controls are exposed through `device.modules[Module.Light]`. Check `light.has_feature("brightness")`, `light.has_feature("hsv")`, and `light.get_feature("color_temp")` before showing or calling the corresponding controls. Read color-temperature limits from the feature's `minimum_value` and `maximum_value`.
- The relevant public methods are `device.turn_on()`, `device.turn_off()`, `light.set_brightness(percent)`, `light.set_hsv(hue, saturation, value)`, and `light.set_color_temp(kelvin)`. Keep device I/O in one asyncio loop and outside Qt's UI thread.
- The probe and controller serialize calls per IP and close each device session after use. A debounce should coalesce slider changes so a slow bulb does not accumulate stale commands.

## Windows setup findings

- Python 3.14.4 was installed at `C:\Users\jnettome\AppData\Local\Programs\Python\Python314\python.exe`, but `py -0p` did not list it and `python` resolved to the Microsoft Store alias. When the launcher cannot find the install, call the full interpreter path directly.
- `venv` creation initially failed because `ensurepip` could not write to the sandbox-provided temporary directory. Setting PowerShell `TEMP` and `TMP` to a writable project-local `.tmp` directory allowed it to complete. `.tmp/` and `.venv*/` are ignored by Git.
- Keep the Tapo password out of command arguments and shell history. The CLI accepts `KASA_USERNAME` and prompts for the password with input hidden. The desktop stores the password through `keyring`; the selected backend here was `keyring.backends.Windows.WinVaultKeyring` (Windows Credential Manager).
- `QSystemTrayIcon` uses a lightbulb icon drawn in code. Closing the popup hides it; the tray menu provides power, refresh, settings, and quit actions. The first-run settings dialog initially appeared behind the always-on-top popup. Startup was changed to open settings before showing the popup, and settings are opened as a standalone dialog before returning to the tray popup.

## Build and verification status

- The project environment used Python 3.14.4, python-kasa 0.12.0, PySide6 6.12.0, keyring 25.7.0, and PyInstaller 6.22.3.
- `python -m compileall -q app scripts tests` and imports of the Qt app/controller modules completed successfully. The pytest suite was created but not run.
- The Qt source popup was launched for a visual check. The popup layout rendered; the later first-run layering fix was included before the final package build but was not rechecked in that final build.
- `build.ps1 -Clean` completed and created a one-folder package at `dist\Tapo Desktop\Tapo Desktop.exe`. The folder is about 181 MB; the executable itself is about 6.7 MB.
- The packaged app failed at startup with “DLL load failed while importing QtCore: The specified procedure could not be found” (Windows error 127), although imports worked from the virtual environment. **Root cause:** the build ran from a shell whose PATH included a tool runtime's native folders (poppler and libheif under `C:\Program Files\WindowsApps\OpenAI.CodexPrimaryRuntime…`). PyInstaller resolved `Qt6Core.dll`'s `icuuc.dll` dependency from there and bundled an upstream ICU 78 build (`icuuc.dll` + `icudt78.dll`) into `_internal`. That ICU exports versioned symbols (`ucnv_open_78`), while Qt6Core imports the unversioned names (`ucnv_open`) that only Windows' own `System32\icuuc.dll` provides. `ucrtbase.dll` and `api-ms-win-*` forwarders were also pulled in from the libheif folder.
- `build.ps1` now runs PyInstaller with a minimal PATH (venv Scripts plus Windows system folders) and fails the build if `icu*.dll` or `ucrtbase.dll` appear in `_internal`. Verified after the fix: the launched exe loads `Qt6Core.dll`/`Qt6Widgets.dll` from the bundle and `icuuc.dll`/`icu.dll` from `C:\Windows\System32`.
- Earlier attempts (reinstalling the PySide6/Shiboken wheels, and the separate `dist-verified` build) did not fix this. Their “still running after launch” check was misleading: the windowed error dialog keeps the process alive. To verify a build, check the loaded modules (for example, `(Get-Process <name>).Modules`) or the window title, not just whether the process is running.
- An initial PyInstaller configuration using `--collect-all kasa` and `--collect-all keyring` unnecessarily pulled in CLI modules and alternate keyring backends. The build script now relies on normal static imports and explicitly includes only `keyring.backends.Windows`. PySide6 remains the main size contributor; if bundle size becomes a priority, evaluate a PySide6 Essentials-only dependency and exclude unused Qt Addons with a runtime smoke check.

## Current project entry points

- `python -m scripts.discover` — UDP broadcast discovery.
- `python -m scripts.discover --ip <address>` — direct-IP probe and capability report.
- `python -m scripts.control --ip <address> state|on|off|brightness|color|temperature` — CLI controller.
- `python -m app.main` — Qt tray application.
- `build.ps1` — one-folder Windows executable build.
