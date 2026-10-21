"""Waybar module catalog: per-module options, generated config and preview labels.

Every module the bar builder offers lives in ``MODULES``. Each option has a
kind (``bool``/``int``/``choice``) so the same spec drives validation, the
option rows in the UI and the generated Waybar config. Users never type free
text here, which keeps every value that reaches config.jsonc/style.css tame.
"""

from __future__ import annotations

import datetime

# Nerd Font v3 glyphs (Material Design range); written as escapes so they survive any editor.
G = {
    "battery": ["\U000F008E", "\U000F007A", "\U000F007B", "\U000F007C", "\U000F007D", "\U000F007E",
                "\U000F007F", "\U000F0080", "\U000F0081", "\U000F0082", "\U000F0079"],
    "charging": "\U000F0084",
    "wifi": "\U000F0928",
    "ethernet": "\U000F0200",
    "offline": "\U000F092D",
    "volume": ["\U000F057F", "\U000F0580", "\U000F057E"],
    "muted": "\U000F075F",
    "cpu": "\U000F035B",
    "memory": "\U000F061A",
    "temperature": "\U000F050F",
    "backlight": "\U000F00E0",
    "bluetooth": "\U000F00AF",
    "bt_connected": "\U000F00B1",
    "bt_off": "\U000F00B2",
    "power": "\U000F0425",
    "coffee": "\U000F0176",
    "coffee_off": "\U000F0FAA",
    "music": "\U000F0386",
    "caps": "\U000F0A9B",
    "num": "\U000F03A0",
    "keyboard": "\U000F030C",
    "lock": "\U000F033E",
    "unlock": "\U000F033F",
}

ROMAN = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"]
KANJI = ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"]

CLOCK_FORMATS = {
    "24h": "%H:%M",
    "12h": "%I:%M %p",
    "day-24h": "%a %d %b  %H:%M",
    "full": "%A, %B %d  %H:%M",
    "iso": "%Y-%m-%d  %H:%M",
}
SAMPLE_TIME = datetime.datetime(2026, 10, 21, 14, 32, 8)


def _o(key, kind, label, default, **extra):
    return {"key": key, "kind": kind, "label": label, "default": default, **extra}


def _choices(*pairs):
    return [{"value": v, "label": lbl} for v, lbl in pairs]


# id -> spec. "waybar" is the module name in config.jsonc, "css" the #id Waybar gives its widget.
MODULES = {
    "workspaces": {
        "waybar": "hyprland/workspaces", "css": "workspaces", "label": "Workspaces",
        "icon": "view-grid-symbolic", "description": "Buttons for your workspaces",
        "options": [
            _o("style", "choice", "Style", "numbers", choices=_choices(
                ("numbers", "Numbers"), ("dots", "Dots"), ("roman", "Roman numerals"), ("kanji", "Kanji"))),
            _o("persistent", "int", "Always show", 5, min=0, max=10, unit="workspaces",
               description="Keep this many workspaces visible even when empty"),
            _o("scroll", "bool", "Scroll to switch", True),
        ],
    },
    "window": {
        "waybar": "hyprland/window", "css": "window", "label": "Window title",
        "icon": "focus-windows-symbolic", "description": "Title of the focused window",
        "options": [_o("max_length", "int", "Longest title", 48, min=10, max=120, unit="characters")],
    },
    "clock": {
        "waybar": "clock", "css": "clock", "label": "Clock",
        "icon": "preferences-system-time-symbolic", "description": "Time and date",
        "options": [
            _o("format", "choice", "Format", "24h", choices=_choices(
                ("24h", "14:32"), ("12h", "2:32 PM"), ("day-24h", "Tue 21 Oct  14:32"),
                ("full", "Tuesday, October 21  14:32"), ("iso", "2026-10-21  14:32"))),
            _o("seconds", "bool", "Show seconds", False),
            _o("calendar", "bool", "Calendar on hover", True),
        ],
    },
    "battery": {
        "waybar": "battery", "css": "battery", "label": "Battery",
        "icon": "battery-good-symbolic", "description": "Charge level and warnings",
        "options": [
            _o("warning", "int", "Warning below", 30, min=5, max=60, unit="%"),
            _o("critical", "int", "Critical below", 15, min=3, max=40, unit="%"),
            _o("percent", "bool", "Show percentage", True),
        ],
    },
    "network": {
        "waybar": "network", "css": "network", "label": "Network",
        "icon": "network-wireless-symbolic", "description": "Wi-Fi or wired connection",
        "options": [_o("label", "choice", "Show", "ssid", choices=_choices(
            ("icon", "Icon only"), ("ssid", "Network name"), ("ip", "IP address")))],
    },
    "pulseaudio": {
        "waybar": "pulseaudio", "css": "pulseaudio", "label": "Volume",
        "icon": "audio-volume-high-symbolic", "description": "Volume (PulseAudio / PipeWire)",
        "options": [
            _o("percent", "bool", "Show percentage", True),
            _o("step", "int", "Scroll step", 5, min=1, max=20, unit="%"),
        ],
    },
    "wireplumber": {
        "waybar": "wireplumber", "css": "wireplumber", "label": "Volume (WirePlumber)",
        "icon": "audio-speakers-symbolic", "description": "Volume straight from WirePlumber",
        "options": [
            _o("percent", "bool", "Show percentage", True),
            _o("step", "int", "Scroll step", 5, min=1, max=20, unit="%"),
        ],
    },
    "cpu": {
        "waybar": "cpu", "css": "cpu", "label": "CPU",
        "icon": "power-profile-performance-symbolic", "description": "Processor usage",
        "options": [_o("interval", "int", "Update every", 5, min=1, max=60, unit="s")],
    },
    "memory": {
        "waybar": "memory", "css": "memory", "label": "Memory",
        "icon": "drive-multidisk-symbolic", "description": "RAM in use",
        "options": [_o("interval", "int", "Update every", 10, min=1, max=60, unit="s")],
    },
    "temperature": {
        "waybar": "temperature", "css": "temperature", "label": "Temperature",
        "icon": "weather-clear-symbolic", "description": "CPU temperature",
        "options": [_o("critical", "int", "Hot above", 80, min=50, max=110, unit="°C")],
    },
    "tray": {
        "waybar": "tray", "css": "tray", "label": "System tray",
        "icon": "view-app-grid-symbolic", "description": "Icons of background apps",
        "options": [
            _o("icon_size", "int", "Icon size", 16, min=12, max=32, unit="px"),
            _o("spacing", "int", "Icon spacing", 8, min=0, max=20, unit="px"),
        ],
    },
    "backlight": {
        "waybar": "backlight", "css": "backlight", "label": "Brightness",
        "icon": "display-brightness-symbolic", "description": "Screen brightness",
        "options": [_o("percent", "bool", "Show percentage", True)],
    },
    "bluetooth": {
        "waybar": "bluetooth", "css": "bluetooth", "label": "Bluetooth",
        "icon": "bluetooth-active-symbolic", "description": "Bluetooth state",
        "options": [_o("device", "bool", "Show connected device", False)],
    },
    "power": {
        "waybar": "custom/power", "css": "custom-power", "label": "Power menu",
        "icon": "system-shutdown-symbolic", "description": "Lock, log out, restart, shut down",
        "options": [
            _o("lock", "bool", "Lock in menu", True),
            _o("suspend", "bool", "Suspend in menu", True),
        ],
    },
    "idle_inhibitor": {
        "waybar": "idle_inhibitor", "css": "idle_inhibitor", "label": "Stay awake",
        "icon": "night-light-symbolic", "description": "Keep the screen on with one click",
        "options": [],
    },
    "mpris": {
        "waybar": "mpris", "css": "mpris", "label": "Now playing",
        "icon": "media-playback-start-symbolic", "description": "Song from any media player",
        "options": [_o("max_length", "int", "Longest text", 30, min=10, max=80, unit="characters")],
    },
    "keyboard_state": {
        "waybar": "keyboard-state", "css": "keyboard-state", "label": "Caps & Num Lock",
        "icon": "input-keyboard-symbolic", "description": "Shows when a lock key is on",
        "options": [
            _o("capslock", "bool", "Caps Lock", True),
            _o("numlock", "bool", "Num Lock", False),
        ],
    },
    "language": {
        "waybar": "hyprland/language", "css": "language", "label": "Keyboard layout",
        "icon": "preferences-desktop-locale-symbolic", "description": "Current keyboard language",
        "options": [_o("style", "choice", "Show", "short", choices=_choices(
            ("short", "Short (EN)"), ("long", "Full name")))],
    },
}

POWER_ACTIONS = [
    ("lock", "Lock", "loginctl lock-session"),
    ("suspend", "Suspend", "systemctl suspend"),
    ("logout", "Log Out", "hyprctl dispatch exit"),
    ("reboot", "Restart", "systemctl reboot"),
    ("shutdown", "Shut Down", "systemctl poweroff"),
]


def option_spec(mtype: str, key: str) -> dict | None:
    for opt in MODULES[mtype]["options"]:
        if opt["key"] == key:
            return opt
    return None


def defaults(mtype: str) -> dict:
    return {o["key"]: o["default"] for o in MODULES[mtype]["options"]}


def power_actions(opts: dict) -> list[tuple[str, str, str]]:
    return [a for a in POWER_ACTIONS if opts.get(a[0], True)]


def _with_seconds(fmt: str) -> str:
    return fmt.replace("%M", "%M:%S", 1)


def clock_strftime(opts: dict) -> str:
    fmt = CLOCK_FORMATS.get(opts.get("format"), CLOCK_FORMATS["24h"])
    return _with_seconds(fmt) if opts.get("seconds") else fmt


def workspace_labels(style: str) -> list[str]:
    if style == "roman":
        return ROMAN
    if style == "kanji":
        return KANJI
    return [str(i) for i in range(1, 11)]


# ---------------------------------------------------------------- waybar config

def module_config(mtype: str, opts: dict, icons: bool, menu_file: str) -> dict:
    """The block for one module in config.jsonc (``opts`` already validated)."""
    o = {**defaults(mtype), **opts}
    if mtype == "workspaces":
        cfg = {"disable-scroll": not o["scroll"], "all-outputs": False, "on-click": "activate"}
        if o["style"] == "numbers":
            cfg["format"] = "{id}"
        elif o["style"] == "dots":
            cfg["format"] = "{icon}"
            cfg["format-icons"] = {"active": "●", "urgent": "●", "default": "○",
                                   "empty": "○", "persistent": "○"}
        else:
            cfg["format"] = "{icon}"
            cfg["format-icons"] = {str(i + 1): lbl for i, lbl in enumerate(workspace_labels(o["style"]))}
        if o["persistent"]:
            cfg["persistent-workspaces"] = {"*": o["persistent"]}
        return cfg
    if mtype == "window":
        return {"format": "{title}", "max-length": o["max_length"], "separate-outputs": True}
    if mtype == "clock":
        cfg = {"format": "{:" + clock_strftime(o) + "}", "format-alt": "{:%A, %d %B %Y}",
               "interval": 1 if o["seconds"] else 60}
        if o["calendar"]:
            cfg["tooltip-format"] = "<tt><small>{calendar}</small></tt>"
            cfg["calendar"] = {"mode": "month", "weeks-pos": "", "on-scroll": 1}
        else:
            cfg["tooltip"] = False
        return cfg
    if mtype == "battery":
        pct = " {capacity}%" if o["percent"] else ""
        cfg = {"states": {"warning": o["warning"], "critical": o["critical"]},
               "tooltip-format": "{capacity}% — {timeTo}"}
        if icons:
            cfg.update({"format": "{icon}" + pct, "format-charging": G["charging"] + pct,
                        "format-plugged": G["charging"] + pct, "format-icons": G["battery"]})
        else:
            cfg.update({"format": "BAT" + (pct or " {capacity}%"), "format-charging": "CHR {capacity}%"})
        return cfg
    if mtype == "network":
        label = {"icon": "", "ssid": " {essid}", "ip": " {ipaddr}"}[o["label"]]
        wired = {"icon": "", "ssid": " {ifname}", "ip": " {ipaddr}"}[o["label"]]
        if icons:
            return {"format-wifi": G["wifi"] + label, "format-ethernet": G["ethernet"] + wired,
                    "format-disconnected": G["offline"], "format-linked": G["ethernet"],
                    "tooltip-format": "{ifname} — {ipaddr}\n{essid} {signalStrength}%"}
        return {"format-wifi": ("WIFI" + label) if label else "WIFI", "format-ethernet": "ETH" + wired,
                "format-disconnected": "OFFLINE", "tooltip-format": "{ifname} — {ipaddr}"}
    if mtype in ("pulseaudio", "wireplumber"):
        pct = " {volume}%" if o["percent"] else ""
        cfg = {"scroll-step": o["step"], "on-click": "pavucontrol", "tooltip-format": "{volume}% — {desc}"} \
            if mtype == "pulseaudio" else {"on-click": "pavucontrol", "tooltip-format": "{volume}% — {node_name}"}
        if mtype == "wireplumber":
            cfg["scroll-step"] = float(o["step"])
        if icons:
            cfg.update({"format": "{icon}" + pct, "format-muted": G["muted"], "format-icons":
                        {"default": G["volume"]} if mtype == "pulseaudio" else G["volume"]})
        else:
            cfg.update({"format": "VOL" + (pct or " {volume}%"), "format-muted": "MUTE"})
        return cfg
    if mtype == "cpu":
        return {"format": (G["cpu"] if icons else "CPU") + " {usage}%", "interval": o["interval"]}
    if mtype == "memory":
        return {"format": (G["memory"] if icons else "MEM") + " {percentage}%", "interval": o["interval"],
                "tooltip-format": "{used:0.1f} GiB of {total:0.1f} GiB"}
    if mtype == "temperature":
        return {"format": (G["temperature"] if icons else "TEMP") + " {temperatureC}°C",
                "critical-threshold": o["critical"]}
    if mtype == "tray":
        return {"icon-size": o["icon_size"], "spacing": o["spacing"]}
    if mtype == "backlight":
        pct = " {percent}%" if o["percent"] else ""
        return {"format": (G["backlight"] if icons else "BRI") + (pct if icons else (pct or " {percent}%"))}
    if mtype == "bluetooth":
        dev = " {device_alias}" if o["device"] else ""
        if icons:
            return {"format": G["bluetooth"], "format-connected": G["bt_connected"] + dev,
                    "format-disabled": G["bt_off"], "format-off": G["bt_off"],
                    "tooltip-format-connected": "{device_enumerate}", "on-click": "blueman-manager"}
        return {"format": "BT", "format-connected": "BT" + (dev or " on"), "format-off": "BT off",
                "format-disabled": "BT off", "on-click": "blueman-manager"}
    if mtype == "power":
        return {"format": G["power"] if icons else "POWER", "tooltip": False, "menu": "on-click",
                "menu-file": menu_file,
                "menu-actions": {aid: cmd for aid, _lbl, cmd in power_actions(o)}}
    if mtype == "idle_inhibitor":
        return {"format": "{icon}", "format-icons": {"activated": G["coffee"] if icons else "AWAKE",
                                                     "deactivated": G["coffee_off"] if icons else "IDLE"},
                "tooltip-format-activated": "Staying awake", "tooltip-format-deactivated": "Normal sleep"}
    if mtype == "mpris":
        return {"format": (G["music"] + " " if icons else "") + "{dynamic}", "dynamic-len": o["max_length"],
                "dynamic-order": ["title", "artist"], "format-paused": (G["music"] + " " if icons else "")
                + "<i>{dynamic}</i>"}
    if mtype == "keyboard_state":
        return {"capslock": o["capslock"], "numlock": o["numlock"],
                "format": {"capslock": "CAPS {icon}", "numlock": "NUM {icon}"},
                "format-icons": {"locked": G["lock"] if icons else "on",
                                 "unlocked": G["unlock"] if icons else "off"}}
    if mtype == "language":
        fmt = "{short}" if o["style"] == "short" else "{long}"
        return {"format": (G["keyboard"] + " " if icons else "") + fmt}
    raise KeyError(mtype)


# ---------------------------------------------------------------- preview labels

def sample_label(mtype: str, opts: dict, icons: bool) -> str:
    """What the module roughly shows on a real bar; used by the live preview."""
    o = {**defaults(mtype), **opts}
    ic = (lambda g: g + " ") if icons else (lambda _g: "")
    if mtype == "window":
        title = "Firefox — Hyprland Wiki"
        return title if len(title) <= o["max_length"] else title[:o["max_length"] - 1] + "…"
    if mtype == "clock":
        text = SAMPLE_TIME.strftime(clock_strftime(o))
        return text[1:] if text.startswith("0") and o["format"] == "12h" else text
    if mtype == "battery":
        pct = "87%" if o["percent"] else ""
        return (G["battery"][8] + (" " + pct if pct else "")) if icons else "BAT 87%"
    if mtype == "network":
        tail = {"icon": "", "ssid": "Home Wi-Fi", "ip": "192.168.1.24"}[o["label"]]
        return (G["wifi"] + (" " + tail if tail else "")) if icons else ("WIFI " + tail).strip()
    if mtype in ("pulseaudio", "wireplumber"):
        return (G["volume"][2] + (" 64%" if o["percent"] else "")) if icons else "VOL 64%"
    if mtype == "cpu":
        return ic(G["cpu"]) + ("12%" if icons else "CPU 12%")
    if mtype == "memory":
        return ic(G["memory"]) + ("41%" if icons else "MEM 41%")
    if mtype == "temperature":
        return ic(G["temperature"]) + ("52°C" if icons else "TEMP 52°C")
    if mtype == "backlight":
        return (G["backlight"] + (" 70%" if o["percent"] else "")) if icons else "BRI 70%"
    if mtype == "bluetooth":
        if icons:
            return G["bt_connected"] + (" Headphones" if o["device"] else "")
        return "BT Headphones" if o["device"] else "BT on"
    if mtype == "power":
        return G["power"] if icons else "POWER"
    if mtype == "idle_inhibitor":
        return G["coffee_off"] if icons else "IDLE"
    if mtype == "mpris":
        song = "Midnight City — M83"
        song = song if len(song) <= o["max_length"] else song[:o["max_length"] - 1] + "…"
        return ic(G["music"]) + song
    if mtype == "keyboard_state":
        return ("CAPS " if o["capslock"] else "NUM ") + (G["lock"] if icons else "on")
    if mtype == "language":
        return ic(G["keyboard"]) + ("EN" if o["style"] == "short" else "English (US)")
    return MODULES[mtype]["label"]
