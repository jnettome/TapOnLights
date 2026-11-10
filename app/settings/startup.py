"""Optional per-user Windows login startup registration."""

from __future__ import annotations

import sys
from pathlib import Path


def set_launch_on_startup(enabled: bool) -> None:
    if sys.platform != "win32":
        return
    import winreg

    key_path = r"Software\Microsoft\Windows\CurrentVersion\Run"
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0, winreg.KEY_SET_VALUE) as key:
        if enabled:
            if getattr(sys, "frozen", False):
                command = f'"{sys.executable}" --minimized'
            else:
                project_root = Path(__file__).resolve().parents[2]
                pythonw = Path(sys.executable).with_name("pythonw.exe")
                command = (
                    f'cmd.exe /c "set PYTHONPATH={project_root}&& '
                    f'cd /d {project_root}&& {pythonw} -m app.main --minimized"'
                )
            winreg.SetValueEx(key, "Tapo Desktop", 0, winreg.REG_SZ, command)
        else:
            try:
                winreg.DeleteValue(key, "Tapo Desktop")
            except FileNotFoundError:
                pass
