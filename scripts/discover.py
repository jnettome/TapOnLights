"""Discover or probe local TP-Link devices using python-kasa."""

from __future__ import annotations

import argparse
import asyncio
import getpass
import os
import sys

from kasa.exceptions import AuthenticationError, KasaException, UnsupportedDeviceError

from app.devices.discovery import discover_devices, probe_ip


def _credentials(args: argparse.Namespace) -> tuple[str | None, str | None]:
    username = args.username or os.getenv("KASA_USERNAME")
    password = args.password or os.getenv("KASA_PASSWORD")
    if username and not password:
        password = getpass.getpass("Tapo password: ")
    return username, password


def _print_result(device: object) -> None:
    print(f"{device.name or 'Unnamed device'} — {device.model or 'model unknown'} @ {device.host}")
    print(f"  Kind: {'bulb' if device.is_bulb else 'other/unknown'} | Power: {device.is_on if device.is_on is not None else 'unknown'}")
    print(f"  Features: {', '.join(device.features) if device.features else 'not reported'}")
    if device.hardware or device.software:
        print(f"  Hardware: {device.hardware or '?'} | Firmware: {device.software or '?'}")
    if device.error:
        print(f"  Problem: {device.error}")


async def _run(args: argparse.Namespace) -> int:
    username, password = _credentials(args)
    try:
        if args.ip:
            results = [await probe_ip(args.ip, username=username, password=password, timeout=args.timeout)]
        else:
            results = await discover_devices(username=username, password=password, timeout=args.timeout)
    except AuthenticationError as exc:
        print(f"Authentication failed. Check Tapo account credentials. ({exc})", file=sys.stderr)
        return 2
    except UnsupportedDeviceError as exc:
        detail = str(exc)
        if "TPAP" in detail:
            print(
                "The device replied using TPAP encryption, which the installed python-kasa release does not support. "
                "This is a protocol compatibility issue, not an IP or password error. "
                f"Device response: {detail}",
                file=sys.stderr,
            )
        else:
            print(f"The device replied but python-kasa does not recognize it: {detail}", file=sys.stderr)
        return 3
    except (KasaException, asyncio.TimeoutError, OSError) as exc:
        print(f"Unable to connect: {exc}", file=sys.stderr)
        return 1
    if not results:
        print("No supported TP-Link devices found. Confirm the bulb is powered and on the same LAN.")
        return 0
    for result in results:
        _print_result(result)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Probe Tapo/Kasa devices on your local network")
    parser.add_argument("--ip", help="Probe one device by IP instead of broadcasting discovery")
    parser.add_argument("--username", help="Tapo account email (or set KASA_USERNAME)")
    parser.add_argument("--password", help="Tapo account password (prefer prompt or KASA_PASSWORD)")
    parser.add_argument("--timeout", type=float, default=5.0, help="Per-device timeout in seconds (default: 5)")
    return asyncio.run(_run(parser.parse_args()))


if __name__ == "__main__":
    raise SystemExit(main())
