#!/usr/bin/env python3
"""Velocity tray controller for Linux desktops.

Requires pystray and pillow: pip install pystray pillow
"""
import subprocess
import signal
import webbrowser

from pystray import Icon, Menu, MenuItem as Item
from PIL import Image, ImageDraw

CONFIG_PATH = "./velocity.toml"
DATA_DIR = "./velocitydb"
VELOCITY_CMD = [
    "velocity",
    "service",
    "run",
    "--config",
    CONFIG_PATH,
    "--data-dir",
    DATA_DIR,
    "--verbose",
]
SERVICE_PROCESS = None
ICON_HANDLE = None


def _create_image() -> Image.Image:
    img = Image.new("RGBA", (64, 64), "#007AFF")
    draw = ImageDraw.Draw(img)
    draw.ellipse((12, 12, 52, 52), fill="white")
    return img


def _start_service(icon, item):
    global SERVICE_PROCESS
    if SERVICE_PROCESS and SERVICE_PROCESS.poll() is None:
        return
    SERVICE_PROCESS = subprocess.Popen(
        VELOCITY_CMD, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    if icon is not None:
        icon.notify("Velocity service started")


def _stop_service(icon, item):
    global SERVICE_PROCESS
    if SERVICE_PROCESS and SERVICE_PROCESS.poll() is None:
        SERVICE_PROCESS.terminate()
        try:
            SERVICE_PROCESS.wait(timeout=5)
        except subprocess.TimeoutExpired:
            SERVICE_PROCESS.kill()
        finally:
            SERVICE_PROCESS = None
        if icon is not None:
            icon.notify("Velocity service stopped")


def _open_studio(icon, item):
    webbrowser.open("http://127.0.0.1:3000")


def _quit(icon, item):
    _stop_service(icon, item)
    if icon is not None:
        icon.stop()


def _signal_handler(signum, frame):
    if ICON_HANDLE is not None:
        ICON_HANDLE.stop()


def main():
    global ICON_HANDLE
    icon = Icon(
        "velocity",
        _create_image(),
        "Velocity Service",
        menu=Menu(
            Item("Start Service", _start_service),
            Item("Stop Service", _stop_service),
            Item("Open Studio", _open_studio),
            Item("Exit", _quit),
        ),
    )
    ICON_HANDLE = icon

    signal.signal(signal.SIGINT, _signal_handler)
    signal.signal(signal.SIGTERM, _signal_handler)

    _start_service(icon, None)
    icon.run()
    _stop_service(icon, None)


if __name__ == "__main__":
    main()
