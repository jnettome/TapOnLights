"""Compact tray popup for controlling one bulb or a selected group."""

from __future__ import annotations

import asyncio

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QApplication,
    QComboBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QSlider,
    QVBoxLayout,
    QWidget,
    QInputDialog,
    QGridLayout,
)
from kasa.exceptions import AuthenticationError, KasaException

from app.async_runner import AsyncRunner
from app.devices.kasa_client import BulbState, CapabilityError, KasaClient
from app.settings.storage import get_password, save_settings, store_password
from app.settings.startup import set_launch_on_startup
from app.ui.color_wheel import ColorWheel
from app.ui.settings_dialog import SettingsDialog


SCENES = {
    "Focus": {"temperature": 4200, "brightness": 95},
    "Relax": {"temperature": 2800, "brightness": 55},
    "Night": {"temperature": 2500, "brightness": 12},
    "Sunset": {"hsv": (20, 85, 68), "brightness": 48},
    "Party": {"hsv": (285, 100, 100), "brightness": 80},
    "Reading": {"temperature": 3000, "brightness": 88},
}


def _card() -> QFrame:
    frame = QFrame()
    frame.setObjectName("card")
    return frame


class MainWindow(QMainWindow):
    def __init__(self, settings: dict, runner: AsyncRunner, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.settings = settings
        self.runner = runner
        self.client = KasaClient()
        self.states: dict[str, BulbState] = {}
        self._quitting = False
        self._pending_task = False
        self._queued_task = None
        self.show_popup_callback = None
        self.setWindowFlags(Qt.WindowType.Tool | Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint)
        self.setWindowTitle("Tapo Desktop")
        self.setFixedWidth(390)
        self._build_ui()

        self.refresh_timer = QTimer(self)
        self.refresh_timer.setInterval(45_000)
        self.refresh_timer.timeout.connect(self.refresh)
        self.refresh_timer.start()
        if self.devices:
            QTimer.singleShot(150, self.refresh)

    @property
    def devices(self) -> list[dict]:
        return self.settings.get("devices", [])

    def _build_ui(self) -> None:
        root = QWidget()
        layout = QVBoxLayout(root)
        layout.setContentsMargins(20, 18, 20, 18)
        layout.setSpacing(12)

        header = QHBoxLayout()
        brand = QLabel("TAPO")
        brand.setObjectName("eyebrow")
        title = QLabel("Light control")
        title.setObjectName("title")
        heading = QVBoxLayout()
        heading.addWidget(brand)
        heading.addWidget(title)
        header.addLayout(heading)
        header.addStretch(1)
        self.theme_button = QPushButton("◐")
        self.theme_button.setObjectName("iconButton")
        self.theme_button.setToolTip("Switch appearance")
        self.theme_button.clicked.connect(self.toggle_theme)
        self.settings_button = QPushButton("⚙")
        self.settings_button.setObjectName("iconButton")
        self.settings_button.setToolTip("Settings")
        self.settings_button.clicked.connect(self.open_settings)
        self.hide_button = QPushButton("×")
        self.hide_button.setObjectName("iconButton")
        self.hide_button.setToolTip("Hide to tray")
        self.hide_button.clicked.connect(self.hide)
        header.addWidget(self.theme_button)
        header.addWidget(self.settings_button)
        header.addWidget(self.hide_button)
        layout.addLayout(header)

        self.target = QComboBox()
        self.target.currentIndexChanged.connect(self.refresh)
        layout.addWidget(self.target)

        power_card = _card()
        power_layout = QHBoxLayout(power_card)
        power_layout.setContentsMargins(16, 14, 16, 14)
        detail = QVBoxLayout()
        self.connection_label = QLabel("Not connected")
        self.connection_label.setObjectName("muted")
        self.device_label = QLabel("Add a bulb in Settings to begin")
        self.device_label.setObjectName("deviceName")
        detail.addWidget(self.connection_label)
        detail.addWidget(self.device_label)
        power_layout.addLayout(detail, 1)
        self.power_button = QPushButton("⏻")
        self.power_button.setObjectName("powerButton")
        self.power_button.setFixedSize(54, 54)
        self.power_button.clicked.connect(self.toggle_power)
        power_layout.addWidget(self.power_button)
        layout.addWidget(power_card)

        self.brightness_value = QLabel("—")
        brightness_card = self._slider_card("BRIGHTNESS", 1, 100, self.brightness_value)
        self.brightness_slider = brightness_card.findChild(QSlider)
        self.brightness_slider.setValue(70)
        self.brightness_slider.valueChanged.connect(self._brightness_changed)
        self.brightness_timer = QTimer(self)
        self.brightness_timer.setSingleShot(True)
        self.brightness_timer.setInterval(220)
        self.brightness_timer.timeout.connect(self.apply_brightness)
        layout.addWidget(brightness_card)

        color_card = _card()
        color_layout = QHBoxLayout(color_card)
        color_layout.setContentsMargins(14, 12, 14, 12)
        wheel_column = QVBoxLayout()
        self.color_wheel = ColorWheel()
        self.color_wheel.colorSelected.connect(self._color_changed)
        wheel_column.addWidget(self.color_wheel, alignment=Qt.AlignmentFlag.AlignCenter)
        color_layout.addLayout(wheel_column)
        color_detail = QVBoxLayout()
        color_detail.addWidget(QLabel("COLOR"), alignment=Qt.AlignmentFlag.AlignLeft)
        self.color_preview = QFrame()
        self.color_preview.setObjectName("colorPreview")
        self.color_preview.setFixedHeight(42)
        color_detail.addWidget(self.color_preview)
        self.color_label = QLabel("Hue 35° · Sat 70%")
        self.color_label.setObjectName("muted")
        color_detail.addWidget(self.color_label)
        color_detail.addStretch(1)
        self.apply_color_button = QPushButton("Apply color")
        self.apply_color_button.clicked.connect(self.apply_color)
        color_detail.addWidget(self.apply_color_button)
        color_layout.addLayout(color_detail, 1)
        layout.addWidget(color_card)

        self.temperature_value = QLabel("—")
        temp_card = self._slider_card("WHITE TEMPERATURE", 2500, 6500, self.temperature_value)
        self.temperature_slider = temp_card.findChild(QSlider)
        self.temperature_slider.setValue(3500)
        self.temperature_slider.valueChanged.connect(self._temperature_changed)
        self.temperature_timer = QTimer(self)
        self.temperature_timer.setSingleShot(True)
        self.temperature_timer.setInterval(220)
        self.temperature_timer.timeout.connect(self.apply_temperature)
        preset_row = QHBoxLayout()
        warm = QPushButton("Warm white")
        cool = QPushButton("Cool white")
        warm.clicked.connect(lambda: self.temperature_slider.setValue(2700))
        cool.clicked.connect(lambda: self.temperature_slider.setValue(6000))
        preset_row.addWidget(warm)
        preset_row.addWidget(cool)
        temp_layout = temp_card.layout()
        temp_layout.addLayout(preset_row)
        layout.addWidget(temp_card)

        scene_card = _card()
        scene_layout = QVBoxLayout(scene_card)
        scene_layout.setContentsMargins(14, 12, 14, 12)
        scene_heading = QHBoxLayout()
        label = QLabel("SCENES")
        label.setObjectName("eyebrow")
        scene_heading.addWidget(label)
        scene_heading.addStretch(1)
        save_scene = QPushButton("＋ Save")
        save_scene.clicked.connect(self.save_scene)
        scene_heading.addWidget(save_scene)
        scene_layout.addLayout(scene_heading)
        self.scene_buttons = QGridLayout()
        scene_layout.addLayout(self.scene_buttons)
        self._populate_scenes()
        layout.addWidget(scene_card)

        self.status = QLabel("Ready")
        self.status.setObjectName("muted")
        layout.addWidget(self.status)
        self.setCentralWidget(root)
        self._populate_targets()

    def _slider_card(self, heading: str, minimum: int, maximum: int, value_label: QLabel) -> QFrame:
        card = _card()
        layout = QVBoxLayout(card)
        layout.setContentsMargins(14, 12, 14, 12)
        top = QHBoxLayout()
        name = QLabel(heading)
        name.setObjectName("eyebrow")
        top.addWidget(name)
        top.addStretch(1)
        top.addWidget(value_label)
        layout.addLayout(top)
        slider = QSlider(Qt.Orientation.Horizontal)
        slider.setRange(minimum, maximum)
        layout.addWidget(slider)
        return card

    def _populate_targets(self) -> None:
        self.target.blockSignals(True)
        self.target.clear()
        self.target.addItem("All bulbs", "all")
        rooms = sorted({entry.get("room", "").strip() for entry in self.devices if entry.get("room", "").strip()})
        for room in rooms:
            self.target.addItem(f"Room · {room}", f"room:{room}")
        for index, entry in enumerate(self.devices):
            label = entry.get("name") or entry["ip"]
            self.target.addItem(label, f"device:{index}")
        self.target.blockSignals(False)

    def selected_devices(self) -> list[dict]:
        selection = self.target.currentData()
        if selection == "all" or not selection:
            return list(self.devices)
        if selection.startswith("room:"):
            room = selection.partition(":")[2]
            return [entry for entry in self.devices if entry.get("room") == room]
        return [self.devices[int(selection.partition(":")[2])]]

    def _credentials(self) -> tuple[str | None, str | None]:
        username = self.settings.get("username") or None
        return username, get_password(username) if username else None

    def _submit(self, coroutine, done) -> None:
        if self._pending_task:
            if self._queued_task:
                self._queued_task[0].close()
            self._queued_task = (coroutine, done)
            return
        self._pending_task = True
        self.runner.submit(coroutine, lambda value, error: self._finish(done, value, error))

    def _finish(self, done, value, error) -> None:
        self._pending_task = False
        if error:
            if isinstance(error, AuthenticationError):
                self.status.setText("Sign in required · check account and compatibility setting")
            else:
                self.status.setText(str(error))
        else:
            done(value)
        if self._queued_task:
            coroutine, callback = self._queued_task
            self._queued_task = None
            QTimer.singleShot(0, lambda: self._submit(coroutine, callback))

    async def _states_for(self, entries: list[dict]) -> list[tuple[dict, BulbState | Exception]]:
        username, password = self._credentials()
        results = []
        for entry in entries:
            try:
                results.append((entry, await self.client.state(entry["ip"], username=username, password=password)))
            except Exception as exc:
                results.append((entry, exc))
        return results

    def refresh(self, *_args) -> None:
        entries = self.selected_devices()
        if not entries:
            self.connection_label.setText("No bulbs configured")
            self.device_label.setText("Add a bulb in Settings to begin")
            return

        def done(results: list[tuple[dict, BulbState | Exception]]) -> None:
            online_states = [result for _, result in results if isinstance(result, BulbState)]
            auth_failures = [entry for entry, result in results if isinstance(result, AuthenticationError)]
            for entry, result in results:
                if isinstance(result, BulbState):
                    self.states[result.host] = result
            offline = len(results) - len(online_states)
            state_summary = f"Connected · {len(online_states)} of {len(results)} bulbs"
            if offline:
                state_summary += f" · {offline} offline"
            self.connection_label.setText(state_summary)
            if len(online_states) == 1 and len(results) == 1:
                state = online_states[0]
                self.device_label.setText(state.name)
                if state.brightness is not None:
                    self.brightness_slider.blockSignals(True)
                    self.brightness_slider.setValue(max(1, state.brightness))
                    self.brightness_slider.blockSignals(False)
                    self.brightness_value.setText(f"{state.brightness}%")
                self.brightness_slider.setEnabled(state.brightness is not None)
                if state.color_temperature is not None:
                    limits = state.color_temperature_range or (2500, 6500)
                    self.temperature_slider.setRange(*limits)
                    self.temperature_slider.blockSignals(True)
                    self.temperature_slider.setValue(state.color_temperature)
                    self.temperature_slider.blockSignals(False)
                    self.temperature_value.setText(f"{state.color_temperature} K")
                self.temperature_slider.setEnabled(state.color_temperature_range is not None)
                if state.hue is not None and state.saturation is not None:
                    self.color_wheel.set_hsv(state.hue, state.saturation)
                    self._color_changed(state.hue, state.saturation)
                self.color_wheel.setEnabled(state.hue is not None)
                self.apply_color_button.setEnabled(state.hue is not None)
                self.power_button.setProperty("on", state.is_on)
            else:
                self.device_label.setText("Selected bulbs" if len(results) != len(self.devices) else "All selected bulbs")
                self.power_button.setProperty("on", bool(online_states) and all(state.is_on for state in online_states))
                common_features = set.intersection(*(set(state.features) for state in online_states)) if online_states else set()
                self.brightness_slider.setEnabled("brightness" in common_features)
                self.temperature_slider.setEnabled("color_temperature" in common_features)
                self.color_wheel.setEnabled("hsv" in common_features)
                self.apply_color_button.setEnabled("hsv" in common_features)
            self.power_button.style().unpolish(self.power_button)
            self.power_button.style().polish(self.power_button)
            if auth_failures:
                self.status.setText("Authentication failed · check account credentials and Third-Party Compatibility")
            else:
                self.status.setText("Updated just now" if not offline else "Some bulbs are offline; online bulbs remain available")

        self._submit(self._states_for(entries), done)

    async def _apply(self, entries: list[dict], operation) -> list[tuple[str, BulbState | Exception]]:
        username, password = self._credentials()
        results: list[tuple[str, BulbState | Exception]] = []
        for entry in entries:
            try:
                result = await operation(self.client, entry["ip"], username, password)
                results.append((entry["ip"], result))
            except Exception as exc:
                results.append((entry["ip"], exc))
        return results

    def _apply_done(self, results) -> None:
        errors = [f"{host}: {result}" for host, result in results if isinstance(result, Exception)]
        self.status.setText(errors[0] if errors else "Command sent · updated state")
        self.refresh()

    def toggle_power(self) -> None:
        entries = self.selected_devices()
        if not entries:
            self.open_settings()
            return
        target_power = {
            entry["ip"]: not self.states[entry["ip"]].is_on
            for entry in entries
            if entry["ip"] in self.states
        }
        if not target_power:
            self.refresh()
            return

        async def operate(client, host, username, password):
            return await client.set_power(host, target_power.get(host, True), username=username, password=password)

        self._submit(self._apply(entries, operate), self._apply_done)

    def _brightness_changed(self, value: int) -> None:
        self.brightness_value.setText(f"{value}%")
        self.brightness_timer.start()

    def apply_brightness(self) -> None:
        entries, value = self.selected_devices(), self.brightness_slider.value()

        async def operate(client, host, username, password):
            return await client.set_brightness(host, value, username=username, password=password)

        if entries:
            self._submit(self._apply(entries, operate), self._apply_done)

    def _color_changed(self, hue: int, saturation: int) -> None:
        self.color_label.setText(f"Hue {hue}° · Sat {saturation}%")
        color = QColor.fromHsv(hue, int(saturation * 2.55), 255)
        self.color_preview.setStyleSheet(f"background: {color.name()}; border-radius: 8px;")

    def apply_color(self) -> None:
        entries = self.selected_devices()
        hue, saturation = self.color_wheel._hue, self.color_wheel._saturation
        value = max(1, self.brightness_slider.value())

        async def operate(client, host, username, password):
            return await client.set_hsv(host, hue, saturation, value, username=username, password=password)

        if entries:
            self._submit(self._apply(entries, operate), self._apply_done)

    def _temperature_changed(self, value: int) -> None:
        self.temperature_value.setText(f"{value} K")
        self.temperature_timer.start()

    def apply_temperature(self) -> None:
        entries, value = self.selected_devices(), self.temperature_slider.value()

        async def operate(client, host, username, password):
            return await client.set_color_temperature(host, value, username=username, password=password)

        if entries:
            self._submit(self._apply(entries, operate), self._apply_done)

    def _populate_scenes(self) -> None:
        while self.scene_buttons.count():
            item = self.scene_buttons.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
        entries = list(SCENES.items()) + list(self.settings.get("custom_scenes", {}).items())
        for index, (name, scene) in enumerate(entries):
            button = QPushButton(name)
            button.setObjectName("sceneButton")
            button.clicked.connect(lambda _checked=False, key=name, data=scene: self.apply_scene(key, data))
            self.scene_buttons.addWidget(button, index // 3, index % 3)

    def apply_scene(self, name: str, scene: dict) -> None:
        entries = self.selected_devices()

        async def operate(client, host, username, password):
            result = None
            if "temperature" in scene:
                result = await client.set_color_temperature(host, int(scene["temperature"]), username=username, password=password)
            elif "hsv" in scene:
                hue, saturation, value = scene["hsv"]
                result = await client.set_hsv(host, int(hue), int(saturation), int(value), username=username, password=password)
            if "brightness" in scene:
                result = await client.set_brightness(host, int(scene["brightness"]), username=username, password=password)
            return result

        self._submit(self._apply(entries, operate), lambda result: self._apply_done(result))

    def save_scene(self) -> None:
        name, accepted = QInputDialog.getText(self, "Save scene", "Scene name")
        if not accepted or not name.strip():
            return
        selected = self.selected_devices()
        state = self.states.get(selected[0]["ip"]) if selected else None
        scene: dict = {"brightness": self.brightness_slider.value()}
        if state and state.color_temperature is not None:
            scene["temperature"] = self.temperature_slider.value()
        else:
            scene["hsv"] = [self.color_wheel._hue, self.color_wheel._saturation, self.brightness_slider.value()]
        self.settings.setdefault("custom_scenes", {})[name.strip()] = scene
        save_settings(self.settings)
        self._populate_scenes()
        self.status.setText(f"Saved scene “{name.strip()}”")

    def toggle_theme(self) -> None:
        self.settings["theme"] = "dark" if self.settings.get("theme") == "light" else "light"
        save_settings(self.settings)
        self._apply_theme()

    def _apply_theme(self) -> None:
        app = QApplication.instance()
        if app:
            from app.main import apply_theme

            apply_theme(app, self.settings.get("theme", "light"))

    def open_settings(self) -> None:
        self.hide()
        dialog = SettingsDialog(self.settings)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            self._show_popup()
            return
        password = self.settings.pop("pending_password", "")
        if password:
            try:
                store_password(self.settings.get("username", ""), password)
            except Exception as exc:
                self._show_popup()
                QMessageBox.critical(self, "Secure storage unavailable", f"Could not save credentials to Windows Credential Manager.\n\n{exc}")
                return
        save_settings(self.settings)
        try:
            set_launch_on_startup(bool(self.settings.get("launch_on_startup", False)))
        except OSError as exc:
            self.status.setText(f"Could not update Windows startup setting: {exc}")
        self._populate_targets()
        self._populate_scenes()
        self._apply_theme()
        self._show_popup()
        self.refresh()

    def _show_popup(self) -> None:
        if self.show_popup_callback:
            self.show_popup_callback()
        else:
            self.show()
            self.raise_()
            self.activateWindow()

    def closeEvent(self, event) -> None:
        if self._quitting:
            event.accept()
        else:
            self.hide()
            event.ignore()

    def quit_application(self) -> None:
        self._quitting = True
        self.close()
