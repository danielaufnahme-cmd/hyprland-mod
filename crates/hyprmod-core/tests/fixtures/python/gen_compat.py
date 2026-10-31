"""Regenerates the Python-written files in this folder for the Rust cross-compat tests.

Run from anywhere: python3 gen_compat.py lua && python3 gen_compat.py conf (dry-run, temp dirs only).
"""
import json, os, shutil, sys, tempfile
sys.dont_write_bytecode = True
ROOT = "/home/daniel/hyprland-mod-rust"
sys.path.insert(0, ROOT)
FIX = os.path.join(ROOT, "tests", "fixtures")
OUT = os.path.join(ROOT, "crates", "hyprmod-core", "tests", "fixtures", "python")
os.makedirs(OUT, exist_ok=True)

def scenario(mode):
    tmp = tempfile.mkdtemp(prefix="hm-gen-")
    hypr = os.path.join(tmp, "hypr"); os.makedirs(hypr)
    if mode == "lua":
        shutil.copy(os.path.join(FIX, "sample_hyprland.lua"), os.path.join(hypr, "hyprland.lua"))
        shutil.copy(os.path.join(FIX, "colors.lua"), os.path.join(hypr, "colors.lua"))
        cfg = os.path.join(hypr, "hyprland.lua")
    else:
        shutil.copy(os.path.join(FIX, "sample_hyprland.conf"), os.path.join(hypr, "hyprland.conf"))
        shutil.copy(os.path.join(FIX, "extra.conf"), os.path.join(hypr, "extra.conf"))
        cfg = os.path.join(hypr, "hyprland.conf")
    os.environ.update({"HYPRLAND_MOD_DRY_RUN": "1", "HYPRLAND_MOD_CONFIG": cfg,
                       "HYPRLAND_MOD_DATA": os.path.join(tmp, "data"),
                       "HYPRLAND_MOD_LUA_STUBS": os.path.join(FIX, "hl_stubs.lua"), "HOME": os.path.join(tmp, "home")})
    os.environ.pop("HYPRLAND_INSTANCE_SIGNATURE", None)
    from hyprland_mod.core import ConfigManager
    cm = ConfigManager(runner=lambda a: (_ for _ in ()).throw(AssertionError(a)))
    cm.set("decoration:rounding", "8")
    cm.set("decoration:blur:enabled", "false")
    cm.set("decoration:blur:vibrancy", "0.2")
    cm.set("decoration:shadow:color", "0xee1a1a1a")
    cm.set("decoration:shadow:offset", "2 -3")
    cm.set("general:gaps_out", "5 10")
    cm.set("general:col.active_border", "rgba(ff0000ff) rgba(00ff00ff) 90deg")
    cm.set("general:layout", "master")
    cm.set("misc:vfr", "true")
    cm.set("plugin:my-plugin:label", 'a "quoted" #value é ü')
    cm.set_list("bezier", ["snappy, 0.05, 0.9, 0.1, 1.05"])
    cm.set_list("animation", ["windows, 1, 3, snappy, popin 80%", "fade, 0", "border, 1, 2, spring:easy"])
    cm.add_bind({"id": "bind-aaaa0001", "mods": ["SUPER", "SHIFT"], "key": "T", "action": "run_command",
                 "arg": "kitty --title 'x'", "description": "Terminal ✓", "flags": "le"})
    cm.add_bind({"id": "bind-aaaa0002", "mods": "SUPER", "key": "3", "action": "workspace", "arg": "3"})
    cm.add_bind({"id": "bind-aaaa0003", "mods": [], "key": "XF86AudioMute", "action": "close_window"})
    cm.add_window_rule({"id": "rule-bbbb0001", "name": "Calc", "match": {"class": "^(org.gnome.Calculator)$", "floating": False},
                        "effects": {"float": True, "size": "800 600", "rounding": 4, "opacity": "0.9 0.8"}})
    cm.add_window_rule({"id": "rule-bbbb0002", "name": "Off", "match": {"title": "x"}, "effects": {"pin": True}, "enabled": False})
    cm.set_monitor("DP-1", {"mode": "1920x1080@60", "position": "0x0", "scale": "1", "transform": 1})
    cm.set_monitor("HDMI-A-1", {"enabled": False})
    cm.set_autostart("bar", "waybar --style 'a b'")
    res = cm.apply()
    assert res.ok, res.errors
    exported = cm.export_state()
    # Item dicts come back from disk in sorted-key order, so the stable form of the files is
    # the one a fresh setup writes after importing the export (what the Rust test reproduces).
    shutil.rmtree(tmp)
    tmp = tempfile.mkdtemp(prefix="hm-gen-")
    hypr = os.path.join(tmp, "hypr"); os.makedirs(hypr)
    for name in os.listdir(FIX):
        if name in ("sample_hyprland.lua", "colors.lua", "sample_hyprland.conf", "extra.conf"):
            shutil.copy(os.path.join(FIX, name), hypr)
    target = os.path.join(hypr, "hyprland." + ("lua" if mode == "lua" else "conf"))
    os.replace(os.path.join(hypr, "sample_hyprland." + ("lua" if mode == "lua" else "conf")), target)
    os.environ.update({"HYPRLAND_MOD_CONFIG": target, "HYPRLAND_MOD_DATA": os.path.join(tmp, "data")})
    cm = ConfigManager(runner=lambda a: (_ for _ in ()).throw(AssertionError(a)))
    cm.import_state(exported)
    res = cm.apply()
    assert res.ok, res.errors
    fresh = ConfigManager(runner=lambda a: (_ for _ in ()).throw(AssertionError(a)))
    with open(fresh.managed_path) as fh:
        assert fresh._render(fresh._managed) == fh.read(), "re-render is not stable"
    ext = "lua" if mode == "lua" else "conf"
    out = os.path.join(OUT, mode)
    os.makedirs(out, exist_ok=True)
    shutil.copy(cm.managed_path, os.path.join(out, f"hyprland-mod.{ext}"))
    shutil.copy(cm.config_path, os.path.join(out, f"hyprland.{ext}"))
    shutil.copy(os.path.join(tmp, "data", "history.json"), os.path.join(out, "history.json"))
    with open(os.path.join(out, "export.json"), "w") as fh:
        json.dump(exported, fh, indent=1)
    shutil.rmtree(tmp)

scenario(sys.argv[1])
if sys.argv[1] == "conf":
    from hyprland_mod import presets
    p = presets.validate_preset({"format": "hyprland-mod-preset", "version": 1, "id": "café", "name": "Café ✓ Look",
        "author": "me", "description": "Ünïcode", "tags": ["dark", "Warm"],
        "settings": {"general:gaps_in": 4, "decoration:blur:enabled": True, "decoration:active_opacity": 0.9,
                     "general:col.active_border": "rgb(ff0000)  rgba(00ff00ff) 45deg"},
        "lists": {"bezier": ["snappy, 0.2, 1, 0.3, 1"], "animation": ["windows, 1, 3, snappy, popin 80%"]},
        "preview": {"wallpaper": ["#112233"], "window": "#445566"}})
    presets.export_preset(p, os.path.join(OUT, "cafe.hyprmod-preset"))
