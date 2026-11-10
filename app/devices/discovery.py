"""Async discovery and probe helpers backed by python-kasa."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any

from kasa import Device, Discover
from kasa.exceptions import AuthenticationError, KasaException


@dataclass(frozen=True)
class ProbeResult:
    host: str
    name: str | None
    model: str | None
    is_bulb: bool
    is_on: bool | None
    features: tuple[str, ...]
    hardware: str | None = None
    software: str | None = None
    error: str | None = None


def _feature_names(device: Device) -> tuple[str, ...]:
    """Return advertised feature ids without assuming a bulb capability set."""
    features = getattr(device, "features", None)
    if features is None:
        return ()
    try:
        return tuple(sorted(str(feature) for feature in features))
    except TypeError:
        return ()


def _result(device: Device, host: str | None = None) -> ProbeResult:
    info = getattr(device, "device_info", None)
    model = getattr(device, "model", None) or _info_value(info, "model", "short_name")
    features = _feature_names(device)
    device_type = info.get("device_type") if isinstance(info, dict) else getattr(info, "device_type", None)
    is_bulb = (
        getattr(device_type, "value", device_type) == "bulb"
        or "bulb" in type(device).__name__.lower()
        or "bulb" in str(model).lower()
    )
    return ProbeResult(
        host=str(getattr(device, "host", None) or host or "unknown"),
        name=getattr(device, "alias", None) or getattr(device, "name", None),
        model=str(model) if model else None,
        is_bulb=is_bulb,
        is_on=getattr(device, "is_on", None),
        features=features,
        hardware=_info_value(info, "hardware_version", "hw_version"),
        software=_info_value(info, "firmware_version", "sw_version"),
    )


def _info_value(info: Any, *names: str) -> str | None:
    """Read version fields across python-kasa's typed and legacy info shapes."""
    for name in names:
        value = info.get(name) if isinstance(info, dict) else getattr(info, name, None)
        if value:
            return str(value)
    return None


async def discover_devices(
    *, username: str | None = None, password: str | None = None, timeout: float = 5.0
) -> list[ProbeResult]:
    """Broadcast-discover supported devices, then update each device's state."""
    devices = await Discover.discover(
        username=username or None,
        password=password or None,
        discovery_timeout=timeout,
    )
    results: list[ProbeResult] = []
    for host, device in devices.items():
        try:
            try:
                await asyncio.wait_for(device.update(), timeout=timeout)
                results.append(_result(device, host))
            except AuthenticationError as exc:
                results.append(ProbeResult(host=str(host), name=None, model=None, is_bulb=False,
                                           is_on=None, features=(), error=f"Authentication required or rejected: {exc}"))
            except (KasaException, asyncio.TimeoutError, OSError) as exc:
                results.append(ProbeResult(host=str(host), name=None, model=None, is_bulb=False,
                                           is_on=None, features=(), error=f"Could not query device: {exc}"))
        finally:
            await device.disconnect()
    return results


async def probe_ip(
    host: str, *, username: str | None = None, password: str | None = None, timeout: float = 5.0
) -> ProbeResult:
    """Connect directly to a known IP using python-kasa's protocol detection."""
    device = await asyncio.wait_for(
        Discover.discover_single(host, username=username or None, password=password or None),
        timeout=timeout,
    )
    try:
        await asyncio.wait_for(device.update(), timeout=timeout)
        return _result(device, host)
    finally:
        await device.disconnect()
