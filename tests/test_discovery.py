from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import pytest

from app.devices.discovery import discover_devices, probe_ip


@pytest.mark.asyncio
async def test_discovery_updates_and_reports_features():
    device = SimpleNamespace(
        host="192.0.2.8", alias="Desk", model="L530", is_on=True,
        features={"brightness", "color"},
        device_info={"hw_version": "3.0", "sw_version": "1.1.6", "device_type": "bulb"},
        update=AsyncMock(), disconnect=AsyncMock(),
    )
    with patch("app.devices.discovery.Discover.discover", AsyncMock(return_value={device.host: device})):
        result = await discover_devices()
    assert result[0].host == device.host
    assert result[0].name == "Desk"
    assert result[0].features == ("brightness", "color")
    assert result[0].is_on is True
    assert result[0].is_bulb is True


@pytest.mark.asyncio
async def test_ip_probe_uses_direct_discovery():
    device = SimpleNamespace(
        host="192.0.2.8", alias="Desk", model="L530", is_on=False,
        features=set(), device_info={}, update=AsyncMock(),
        disconnect=AsyncMock(),
    )
    with patch("app.devices.discovery.Discover.discover_single", AsyncMock(return_value=device)) as connect:
        result = await probe_ip(device.host)
    connect.assert_awaited_once_with(device.host, username=None, password=None)
    assert result.is_on is False
