"""Small settings dialog for bulb entries and secure Tapo credentials."""

from __future__ import annotations

import ipaddress

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QHBoxLayout,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
    QVBoxLayout,
    QWidget,
)


class SettingsDialog(QDialog):
    def __init__(self, settings: dict, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Tapo Desktop Settings")
        self.setMinimumWidth(390)
        self.settings = settings
        self._devices = [dict(entry) for entry in settings.get("devices", [])]

        layout = QVBoxLayout(self)
        self.device_list = QListWidget()
        self._refresh_devices()
        layout.addWidget(self.device_list)

        form = QFormLayout()
        self.ip = QLineEdit()
        self.ip.setPlaceholderText("192.168.1.42")
        self.name = QLineEdit()
        self.name.setPlaceholderText("Desk lamp")
        self.room = QLineEdit()
        self.room.setPlaceholderText("Office")
        form.addRow("IP address", self.ip)
        form.addRow("Friendly name", self.name)
        form.addRow("Room", self.room)
        layout.addLayout(form)

        buttons = QHBoxLayout()
        add_button = QPushButton("Add bulb")
        remove_button = QPushButton("Remove selected")
        add_button.clicked.connect(self._add_device)
        remove_button.clicked.connect(self._remove_device)
        buttons.addWidget(add_button)
        buttons.addWidget(remove_button)
        layout.addLayout(buttons)

        self.username = QLineEdit(settings.get("username", ""))
        self.username.setPlaceholderText("Tapo account email")
        self.password = QLineEdit()
        self.password.setEchoMode(QLineEdit.EchoMode.Password)
        self.password.setPlaceholderText("Leave blank to keep saved password")
        form.addRow("Tapo account", self.username)
        form.addRow("Password", self.password)

        self.theme = QComboBox()
        self.theme.addItems(["Light", "Dark"])
        self.theme.setCurrentText(settings.get("theme", "light").title())
        self.start_minimized = QCheckBox("Start minimized to tray")
        self.start_minimized.setChecked(bool(settings.get("start_minimized", False)))
        self.launch_startup = QCheckBox("Launch when I sign in to Windows")
        self.launch_startup.setChecked(bool(settings.get("launch_on_startup", False)))
        form.addRow("Appearance", self.theme)
        form.addRow("", self.start_minimized)
        form.addRow("", self.launch_startup)

        self.buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        self.buttons.accepted.connect(self._save)
        self.buttons.rejected.connect(self.reject)
        layout.addWidget(self.buttons)

    def _refresh_devices(self) -> None:
        self.device_list.clear()
        for entry in self._devices:
            label = entry.get("name") or entry["ip"]
            room = f" · {entry['room']}" if entry.get("room") else ""
            self.device_list.addItem(QListWidgetItem(f"{label}{room} — {entry['ip']}"))

    def _add_device(self) -> None:
        try:
            address = str(ipaddress.ip_address(self.ip.text().strip()))
        except ValueError:
            QMessageBox.warning(self, "Invalid IP address", "Enter a valid IPv4 or IPv6 address.")
            return
        if any(entry["ip"] == address for entry in self._devices):
            QMessageBox.information(self, "Already added", "That IP address is already in the device list.")
            return
        self._devices.append({"ip": address, "name": self.name.text().strip(), "room": self.room.text().strip()})
        self.ip.clear()
        self.name.clear()
        self.room.clear()
        self._refresh_devices()

    def _remove_device(self) -> None:
        index = self.device_list.currentRow()
        if index >= 0:
            self._devices.pop(index)
            self._refresh_devices()

    def _save(self) -> None:
        self.settings.update(
            devices=self._devices,
            username=self.username.text().strip(),
            theme=self.theme.currentText().lower(),
            start_minimized=self.start_minimized.isChecked(),
            launch_on_startup=self.launch_startup.isChecked(),
        )
        self.settings["pending_password"] = self.password.text()
        self.accept()
