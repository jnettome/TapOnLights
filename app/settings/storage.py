"""Persist non-secret preferences as JSON and passwords in the OS keyring."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any


SERVICE_NAME = "Tapo Desktop"


def settings_path() -> Path:
    root = Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming"))
    return root / "Tapo Desktop" / "settings.json"


def load_settings() -> dict[str, Any]:
    defaults: dict[str, Any] = {
        "devices": [],
        "username": "",
        "theme": "light",
        "start_minimized": False,
        "launch_on_startup": False,
        "custom_scenes": {},
    }
    path = settings_path()
    try:
        loaded = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return defaults
    if isinstance(loaded, dict):
        defaults.update(loaded)
    return defaults


def save_settings(settings: dict[str, Any]) -> None:
    path = settings_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(settings, indent=2), encoding="utf-8")


def store_password(username: str, password: str) -> None:
    if not username or not password:
        return
    import keyring

    keyring.set_password(SERVICE_NAME, username, password)


def get_password(username: str) -> str | None:
    if not username:
        return None
    try:
        import keyring

        return keyring.get_password(SERVICE_NAME, username)
    except Exception:
        return None
