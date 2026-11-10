"""Control one Tapo/Kasa bulb over the local network."""

from __future__ import annotations

import argparse
import asyncio
import getpass
import os
import sys

from kasa.exceptions import AuthenticationError, KasaException, UnsupportedDeviceError

from app.devices.kasa_client import BulbState, CapabilityError, KasaClient


def _credentials() -> tuple[str | None, str | None]:
    username = os.getenv("KASA_USERNAME")
    password = os.getenv("KASA_PASSWORD")
    if username and not password:
        password = getpass.getpass("Tapo password: ")
    return username, password


def _print_state(state: BulbState) -> None:
    print(f"{state.name} — {state.model} @ {state.host}")
    print(f"  Power: {'ON' if state.is_on else 'OFF'}")
    if state.brightness is not None:
        print(f"  Brightness: {state.brightness}%")
    if state.color_temperature is not None:
        limits = state.color_temperature_range
        suffix = f" (range {limits[0]}–{limits[1]} K)" if limits else ""
        print(f"  White temperature: {state.color_temperature} K{suffix}")
    if state.hue is not None:
        print(f"  HSV: {state.hue}°, {state.saturation}%, {state.value}%")
    print(f"  Features: {', '.join(state.features)}")


async def _run(args: argparse.Namespace) -> int:
    username, password = _credentials()
    client = KasaClient()
    try:
        if args.command == "on":
            state = await client.set_power(args.ip, True, username=username, password=password)
        elif args.command == "off":
            state = await client.set_power(args.ip, False, username=username, password=password)
        elif args.command == "brightness":
            state = await client.set_brightness(args.ip, args.percent, username=username, password=password)
        elif args.command == "color":
            state = await client.set_hsv(
                args.ip, args.hue, args.saturation, args.value, username=username, password=password
            )
        elif args.command == "temperature":
            state = await client.set_color_temperature(args.ip, args.kelvin, username=username, password=password)
        else:
            state = await client.state(args.ip, username=username, password=password)
        _print_state(state)
        return 0
    except AuthenticationError as exc:
        print(f"Authentication failed. Check the Tapo account email/password and Third-Party Compatibility setting. ({exc})", file=sys.stderr)
        return 2
    except UnsupportedDeviceError as exc:
        print(f"python-kasa does not support this device's advertised protocol: {exc}", file=sys.stderr)
        return 3
    except (CapabilityError, ValueError, KasaException, asyncio.TimeoutError, OSError) as exc:
        print(f"Could not complete the bulb operation: {exc}", file=sys.stderr)
        return 1


def main() -> int:
    parser = argparse.ArgumentParser(description="Control a Tapo bulb over your local network")
    parser.add_argument("--ip", required=True, help="Bulb IP address")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("state", help="Read current state and advertised features")
    commands.add_parser("on", help="Turn the bulb on")
    commands.add_parser("off", help="Turn the bulb off")
    brightness = commands.add_parser("brightness", help="Set brightness")
    brightness.add_argument("percent", type=int)
    color = commands.add_parser("color", help="Set HSV color")
    color.add_argument("hue", type=int, help="Hue 0–360 degrees")
    color.add_argument("saturation", type=int, help="Saturation 0–100 percent")
    color.add_argument("value", type=int, help="Value/brightness 1–100 percent")
    temperature = commands.add_parser("temperature", help="Set white color temperature")
    temperature.add_argument("kelvin", type=int, help="Temperature within the bulb's advertised range")
    return asyncio.run(_run(parser.parse_args()))


if __name__ == "__main__":
    raise SystemExit(main())
