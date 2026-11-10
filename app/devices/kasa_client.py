"""Small async controller for Tapo/Kasa bulbs using python-kasa's public API."""

from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager
from dataclasses import dataclass
from typing import AsyncIterator

from kasa import Device, Discover, Module
from kasa.exceptions import KasaException


@dataclass(frozen=True)
class BulbState:
    host: str
    name: str
    model: str
    is_on: bool
    features: tuple[str, ...]
    brightness: int | None = None
    hue: int | None = None
    saturation: int | None = None
    value: int | None = None
    color_temperature: int | None = None
    color_temperature_range: tuple[int, int] | None = None


class CapabilityError(ValueError):
    """The selected bulb does not advertise the requested operation."""


class KasaClient:
    """Serialize operations per IP and inspect capabilities before each change."""

    def __init__(self, *, timeout: float = 6.0) -> None:
        self.timeout = timeout
        self._locks: dict[str, asyncio.Lock] = {}

    def _lock_for(self, host: str) -> asyncio.Lock:
        return self._locks.setdefault(host, asyncio.Lock())

    @asynccontextmanager
    async def _device(
        self, host: str, *, username: str | None, password: str | None
    ) -> AsyncIterator[Device]:
        device = await asyncio.wait_for(
            Discover.discover_single(host, username=username or None, password=password or None),
            timeout=self.timeout,
        )
        try:
            await asyncio.wait_for(device.update(), timeout=self.timeout)
            yield device
        finally:
            await device.disconnect()

    @staticmethod
    def _light(device: Device):
        if Module.Light not in device.modules:
            raise CapabilityError(f"{device.alias or device.model} does not expose a light-control module")
        return device.modules[Module.Light]

    @staticmethod
    def _state(device: Device) -> BulbState:
        light = KasaClient._light(device)
        features = tuple(sorted(str(feature) for feature in device.features))
        hsv = light.hsv if light.has_feature("hsv") else None
        temp = light.get_feature("color_temp")
        temp_range = (temp.minimum_value, temp.maximum_value) if temp else None
        return BulbState(
            host=str(device.host),
            name=str(device.alias or device.model or device.host),
            model=str(device.model or "Unknown"),
            is_on=bool(device.is_on),
            features=features,
            brightness=light.brightness if light.has_feature("brightness") else None,
            hue=hsv.hue if hsv else None,
            saturation=hsv.saturation if hsv else None,
            value=hsv.value if hsv else None,
            color_temperature=light.color_temp if temp else None,
            color_temperature_range=temp_range,
        )

    async def state(
        self, host: str, *, username: str | None = None, password: str | None = None
    ) -> BulbState:
        async with self._lock_for(host):
            async with self._device(host, username=username, password=password) as device:
                return self._state(device)

    async def set_power(
        self, host: str, on: bool, *, username: str | None = None, password: str | None = None
    ) -> BulbState:
        async with self._lock_for(host):
            async with self._device(host, username=username, password=password) as device:
                await (device.turn_on() if on else device.turn_off())
                await asyncio.wait_for(device.update(), timeout=self.timeout)
                return self._state(device)

    async def set_brightness(
        self, host: str, value: int, *, username: str | None = None, password: str | None = None
    ) -> BulbState:
        if not 1 <= value <= 100:
            raise ValueError("Brightness must be between 1 and 100 percent")
        async with self._lock_for(host):
            async with self._device(host, username=username, password=password) as device:
                light = self._light(device)
                self._require(light, "brightness")
                await light.set_brightness(value)
                await asyncio.wait_for(device.update(), timeout=self.timeout)
                return self._state(device)

    async def set_hsv(
        self,
        host: str,
        hue: int,
        saturation: int,
        value: int,
        *,
        username: str | None = None,
        password: str | None = None,
    ) -> BulbState:
        if not 0 <= hue <= 360 or not 0 <= saturation <= 100 or not 1 <= value <= 100:
            raise ValueError("HSV requires hue 0–360, saturation 0–100, and value 1–100")
        async with self._lock_for(host):
            async with self._device(host, username=username, password=password) as device:
                light = self._light(device)
                self._require(light, "hsv")
                await light.set_hsv(hue, saturation, value)
                await asyncio.wait_for(device.update(), timeout=self.timeout)
                return self._state(device)

    async def set_color_temperature(
        self,
        host: str,
        kelvin: int,
        *,
        username: str | None = None,
        password: str | None = None,
    ) -> BulbState:
        async with self._lock_for(host):
            async with self._device(host, username=username, password=password) as device:
                light = self._light(device)
                feature = light.get_feature("color_temp")
                if feature is None:
                    raise CapabilityError("This bulb does not advertise color-temperature control")
                if not feature.minimum_value <= kelvin <= feature.maximum_value:
                    raise ValueError(
                        f"Color temperature must be between {feature.minimum_value} and "
                        f"{feature.maximum_value} K for this bulb"
                    )
                await light.set_color_temp(kelvin)
                await asyncio.wait_for(device.update(), timeout=self.timeout)
                return self._state(device)

    @staticmethod
    def _require(light, feature: str) -> None:
        if not light.has_feature(feature):
            raise CapabilityError(f"This bulb does not advertise {feature} control")
