"""Tapo Desktop application entry point."""

from __future__ import annotations

import sys

from PySide6.QtCore import QTimer
from PySide6.QtGui import QFont
from PySide6.QtWidgets import QApplication

from app.async_runner import AsyncRunner
from app.settings.storage import load_settings
from app.tray import TrayController
from app.ui.main_window import MainWindow


LIGHT_STYLE = """
QWidget { background: #f5f6f8; color: #20232a; font-family: 'Segoe UI'; font-size: 10pt; }
QLabel#eyebrow { color: #858b97; font-size: 8pt; font-weight: 700; letter-spacing: 1px; }
QLabel#title { font-size: 17pt; font-weight: 650; color: #16191f; }
QLabel#muted { color: #828895; font-size: 9pt; }
QLabel#deviceName { font-size: 11pt; font-weight: 600; }
QFrame#card { background: #ffffff; border: 1px solid #e8eaf0; border-radius: 15px; }
QComboBox { padding: 9px 12px; border: 1px solid #e4e6ec; border-radius: 10px; background: #ffffff; }
QPushButton { padding: 7px 11px; border: 1px solid #e5e7ec; border-radius: 9px; background: #ffffff; }
QPushButton:hover { background: #f1f3f6; }
QPushButton#iconButton { border: 0; font-size: 15pt; padding: 5px; min-width: 34px; max-width: 34px; }
QPushButton#powerButton { background: #e2e7ef; color: #596171; border: 0; font-size: 22pt; }
QPushButton#powerButton[on="true"] { background: #dcf5e9; color: #137c4b; }
QPushButton#sceneButton { padding: 7px 8px; font-size: 8pt; }
QSlider::groove:horizontal { height: 5px; background: #e6e9ef; border-radius: 2px; }
QSlider::sub-page:horizontal { background: #5d79d5; border-radius: 2px; }
QSlider::handle:horizontal { width: 15px; margin: -5px 0; border-radius: 7px; background: #ffffff; border: 2px solid #5d79d5; }
QLineEdit, QListWidget { background: #ffffff; border: 1px solid #e2e5eb; border-radius: 8px; padding: 7px; }
"""

DARK_STYLE = """
QWidget { background: #171a20; color: #edf0f5; font-family: 'Segoe UI'; font-size: 10pt; }
QLabel#eyebrow { color: #929aa8; font-size: 8pt; font-weight: 700; letter-spacing: 1px; }
QLabel#title { font-size: 17pt; font-weight: 650; color: #f5f6f8; }
QLabel#muted { color: #929aa8; font-size: 9pt; }
QLabel#deviceName { font-size: 11pt; font-weight: 600; }
QFrame#card { background: #22262e; border: 1px solid #303641; border-radius: 15px; }
QComboBox { padding: 9px 12px; border: 1px solid #363c48; border-radius: 10px; background: #22262e; }
QPushButton { padding: 7px 11px; border: 1px solid #363c48; border-radius: 9px; background: #292e37; }
QPushButton:hover { background: #343b47; }
QPushButton#iconButton { border: 0; font-size: 15pt; padding: 5px; min-width: 34px; max-width: 34px; }
QPushButton#powerButton { background: #343b47; color: #c3cad5; border: 0; font-size: 22pt; }
QPushButton#powerButton[on="true"] { background: #174b37; color: #7ce0ad; }
QPushButton#sceneButton { padding: 7px 8px; font-size: 8pt; }
QSlider::groove:horizontal { height: 5px; background: #414753; border-radius: 2px; }
QSlider::sub-page:horizontal { background: #8199f1; border-radius: 2px; }
QSlider::handle:horizontal { width: 15px; margin: -5px 0; border-radius: 7px; background: #ffffff; border: 2px solid #8199f1; }
QLineEdit, QListWidget { background: #22262e; border: 1px solid #363c48; border-radius: 8px; padding: 7px; }
"""


def apply_theme(app: QApplication, theme: str) -> None:
    app.setStyleSheet(DARK_STYLE if theme == "dark" else LIGHT_STYLE)


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName("Tapo Desktop")
    app.setQuitOnLastWindowClosed(False)
    app.setFont(QFont("Segoe UI", 10))
    settings = load_settings()
    apply_theme(app, settings.get("theme", "light"))
    runner = AsyncRunner(app)
    window = MainWindow(settings, runner)
    tray = TrayController(window, app)
    window.show_popup_callback = tray.show_popup

    minimized = "--minimized" in sys.argv or settings.get("start_minimized", False)
    if not minimized:
        if not settings.get("devices"):
            QTimer.singleShot(0, window.open_settings)
        else:
            tray.show_popup()

    app.aboutToQuit.connect(runner.close)
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
