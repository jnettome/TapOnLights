from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import pytest
from kasa import Module

from app.devices.kasa_client import CapabilityError, KasaClient


def _fake_light(*features):
    feature_set = set(features)
    return SimpleNamespace(
        has_feature=lambda name: name in feature_set,
        get_feature=lambda name: None,
        set_brightness=AsyncMock(),
        set_hsv=AsyncMock(),
        set_color_temp=AsyncMock(),
        brightness=55,
        hsv=SimpleNamespace(hue=35, saturation=70, value=55),
        color_temp=3000,
    )


def _fake_device(light):
    return SimpleNamespace(
        host="192.0.2.10",
        alias="Test bulb",
        model="L530",
        is_on=True,
        features={"brightness", "hsv"},
        modules={Module.Light: light},
        update=AsyncMock(),
        disconnect=AsyncMock(),
    )


@pytest.mark.asyncio
async def test_set_brightness_uses_light_module_and_disconnects():
    light = _fake_light("brightness")
    device = _fake_device(light)
    with patch("app.devices.kasa_client.Discover.discover_single", AsyncMock(return_value=device)):
        state = await KasaClient().set_brightness(device.host, 63)
    light.set_brightness.assert_awaited_once_with(63)
    device.disconnect.assert_awaited_once()
    assert state.brightness == 55


@pytest.mark.asyncio
async def test_control_rejects_unadvertised_feature_and_disconnects():
    light = _fake_light()
    device = _fake_device(light)
    with patch("app.devices.kasa_client.Discover.discover_single", AsyncMock(return_value=device)):
        with pytest.raises(CapabilityError, match="brightness"):
            await KasaClient().set_brightness(device.host, 63)
    device.disconnect.assert_awaited_once()


@pytest.mark.asyncio
async def test_invalid_brightness_is_rejected_before_connect():
    with patch("app.devices.kasa_client.Discover.discover_single", AsyncMock()) as connect:
        with pytest.raises(ValueError, match="between 1 and 100"):
            await KasaClient().set_brightness("192.0.2.10", 0)
    connect.assert_not_awaited()
