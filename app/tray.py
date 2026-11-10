"""Windows system tray icon, popup activation, and context menu."""

from __future__ import annotations

from PySide6.QtCore import QObject, Qt
from PySide6.QtGui import QAction, QColor, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QApplication, QMenu, QSystemTrayIcon

from app.ui.main_window import MainWindow


def _bulb_icon():
    pixmap = QPixmap(64, 64)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    painter.setPen(Qt.PenStyle.NoPen)
    painter.setBrush(QColor("#F4B740"))
    painter.drawEllipse(14, 7, 36, 38)
    painter.setPen(QPen(QColor("#D38A14"), 3, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
    painter.drawLine(24, 43, 40, 43)
    painter.drawLine(25, 49, 39, 49)
    painter.drawLine(28, 55, 36, 55)
    painter.setPen(QPen(QColor("#F4B740"), 3, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
    painter.drawLine(32, 2, 32, 0)
    painter.drawLine(8, 19, 4, 17)
    painter.drawLine(56, 19, 60, 17)
    painter.end()
    from PySide6.QtGui import QIcon

    return QIcon(pixmap)


class TrayController(QObject):
    def __init__(self, window: MainWindow, app: QApplication) -> None:
        super().__init__(app)
        self.window = window
        self.app = app
        self.icon = QSystemTrayIcon(_bulb_icon(), self)
        self.icon.setToolTip("Tapo Desktop")
        self.menu = self._menu()
        self.icon.setContextMenu(self.menu)
        self.icon.activated.connect(self._activated)
        self.icon.show()

    def _menu(self) -> QMenu:
        menu = QMenu()
        turn_on = menu.addAction("Turn on")
        turn_on.triggered.connect(lambda: self._set_power(True))
        turn_off = menu.addAction("Turn off")
        turn_off.triggered.connect(lambda: self._set_power(False))
        menu.addSeparator()
        refresh = menu.addAction("Refresh")
        refresh.triggered.connect(self.window.refresh)
        settings = menu.addAction("Settings")
        settings.triggered.connect(self.window.open_settings)
        menu.addSeparator()
        quit_action = menu.addAction("Quit")
        quit_action.triggered.connect(self._quit)
        return menu

    def _set_power(self, on: bool) -> None:
        entries = self.window.selected_devices()
        if not entries:
            self.window.open_settings()
            return

        async def run_all():
            username, password = self.window._credentials()
            results = []
            for entry in entries:
                try:
                    state = await self.window.client.set_power(
                        entry["ip"], on, username=username, password=password
                    )
                    results.append((entry["ip"], state))
                except Exception as exc:
                    results.append((entry["ip"], exc))
            return results

        self.window._submit(run_all(), self.window._apply_done)

    def _activated(self, reason: QSystemTrayIcon.ActivationReason) -> None:
        if reason in (QSystemTrayIcon.ActivationReason.Trigger, QSystemTrayIcon.ActivationReason.DoubleClick):
            if self.window.isVisible():
                self.window.hide()
            else:
                self.show_popup()

    def show_popup(self) -> None:
        self.window.adjustSize()
        geometry = self.icon.geometry()
        screen = QApplication.screenAt(geometry.center()) if not geometry.isNull() else QApplication.primaryScreen()
        if screen:
            area = screen.availableGeometry()
            if geometry.isNull():
                x = area.right() - self.window.width() - 12
                y = area.bottom() - self.window.height() - 12
            else:
                x = max(area.left(), min(area.right() - self.window.width(), geometry.center().x() - self.window.width() // 2))
                y = max(area.top(), min(area.bottom() - self.window.height(), geometry.top() - self.window.height() - 8))
            self.window.move(x, y)
        self.window.show()
        self.window.raise_()
        self.window.activateWindow()

    def _quit(self) -> None:
        self.icon.hide()
        self.window.quit_application()
        self.app.quit()
