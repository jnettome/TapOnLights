"""Small clickable HSV color wheel."""

from __future__ import annotations

import math

from PySide6.QtCore import QPointF, Qt, Signal
from PySide6.QtGui import QColor, QImage, QPainter, QPen
from PySide6.QtWidgets import QWidget


class ColorWheel(QWidget):
    colorSelected = Signal(int, int)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setMinimumSize(174, 174)
        self.setMaximumSize(220, 220)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self._hue = 35
        self._saturation = 70
        self._image: QImage | None = None

    def set_hsv(self, hue: int, saturation: int) -> None:
        self._hue = hue % 360
        self._saturation = max(0, min(100, saturation))
        self.update()

    def _wheel_image(self, size: int) -> QImage:
        if self._image is not None and self._image.width() == size:
            return self._image
        image = QImage(size, size, QImage.Format.Format_ARGB32)
        center = (size - 1) / 2
        radius = center - 1
        for y in range(size):
            for x in range(size):
                dx, dy = x - center, center - y
                distance = math.hypot(dx, dy) / radius
                if distance > 1:
                    image.setPixelColor(x, y, QColor(0, 0, 0, 0))
                    continue
                hue = int((math.degrees(math.atan2(dy, dx)) + 360) % 360)
                saturation = int(min(1, distance) * 255)
                image.setPixelColor(x, y, QColor.fromHsv(hue, saturation, 255))
        self._image = image
        return image

    def paintEvent(self, _event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        side = min(self.width(), self.height())
        rect_x = (self.width() - side) / 2
        rect_y = (self.height() - side) / 2
        rect = self.rect().adjusted(int(rect_x), int(rect_y), -int(rect_x), -int(rect_y))
        image = self._wheel_image(side)
        painter.drawImage(rect, image)
        radius = side * self._saturation / 100 / 2
        angle = math.radians(self._hue)
        cx, cy = rect.center().x(), rect.center().y()
        marker = QPointF(cx + math.cos(angle) * radius, cy - math.sin(angle) * radius)
        painter.setPen(QPen(QColor("#FFFFFF"), 3))
        painter.setBrush(Qt.BrushStyle.NoBrush)
        painter.drawEllipse(marker, 7, 7)
        painter.setPen(QPen(QColor("#252934"), 1))
        painter.drawEllipse(marker, 7, 7)

    def mousePressEvent(self, event) -> None:
        self._select(event.position())

    def mouseMoveEvent(self, event) -> None:
        if event.buttons() & Qt.MouseButton.LeftButton:
            self._select(event.position())

    def _select(self, point: QPointF) -> None:
        side = min(self.width(), self.height())
        cx, cy = self.width() / 2, self.height() / 2
        dx, dy = point.x() - cx, cy - point.y()
        radius = side / 2 - 1
        distance = min(1.0, math.hypot(dx, dy) / radius)
        self._hue = int((math.degrees(math.atan2(dy, dx)) + 360) % 360)
        self._saturation = int(distance * 100)
        self.colorSelected.emit(self._hue, self._saturation)
        self.update()
