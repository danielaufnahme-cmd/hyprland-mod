-- Hyprland Mod managed settings. Generated file: manual edits may be overwritten.
-- Loaded from the end of hyprland.lua so these values take precedence.
-- hyprland-mod-state: {"autostart": {"bar": "waybar --style 'a b'"}, "binds": [{"action": "run_command", "arg": "kitty --title 'x'", "description": "Terminal \u2713", "flags": "le", "id": "bind-aaaa0001", "key": "T", "mods": ["SUPER", "SHIFT"]}, {"action": "workspace", "arg": "3", "description": "", "flags": "", "id": "bind-aaaa0002", "key": "3", "mods": ["SUPER"]}, {"action": "close_window", "arg": null, "description": "", "flags": "", "id": "bind-aaaa0003", "key": "XF86AudioMute", "mods": []}], "lists": {"animation": ["windows, 1, 3, snappy, popin 80%", "fade, 0", "border, 1, 2, spring:easy"], "bezier": ["snappy, 0.05, 0.9, 0.1, 1.05"]}, "monitors": {"DP-1": {"enabled": true, "mode": "1920x1080@60", "position": "0x0", "scale": "1", "transform": 1}, "HDMI-A-1": {"enabled": false, "mode": "preferred", "position": "auto", "scale": "auto", "transform": 0}}, "settings": {"decoration:blur:enabled": "false", "decoration:blur:vibrancy": "0.2", "decoration:rounding": "8", "decoration:shadow:color": "0xee1a1a1a", "decoration:shadow:offset": "2 -3", "general:col.active_border": "rgba(ff0000ff) rgba(00ff00ff) 90deg", "general:gaps_out": "5 10", "general:layout": "master", "misc:vfr": "true", "plugin:my-plugin:label": "a \"quoted\" #value \u00e9 \u00fc"}, "version": 1, "window_rules": [{"effects": {"float": true, "opacity": "0.9 0.8", "rounding": "4", "size": "800 600"}, "enabled": true, "id": "rule-bbbb0001", "match": {"class": "^(org.gnome.Calculator)$", "floating": false}, "name": "Calc"}, {"effects": {"pin": true}, "enabled": false, "id": "rule-bbbb0002", "match": {"title": "x"}, "name": "Off"}]}

hl.config({
    decoration = {
        blur = {
            enabled = false,
            vibrancy = 0.2,
        },
        rounding = 8,
        shadow = {
            color = "rgba(1a1a1aee)",
            offset = { 2, -3 },
        },
    },
    general = {
        col = {
            active_border = { colors = { "rgba(ff0000ff)", "rgba(00ff00ff)" }, angle = 90 },
        },
        gaps_out = { top = 5, right = 10, bottom = 5, left = 10 },
        layout = "master",
    },
    misc = {
        vfr = true,
    },
    plugin = {
        ["my-plugin"] = {
            label = "a \"quoted\" #value é ü",
        },
    },
})

hl.curve("snappy", { type = "bezier", points = { {0.05, 0.9}, {0.1, 1.05} } })

hl.animation({ leaf = "windows", enabled = true, speed = 3, bezier = "snappy", style = "popin 80%" })
hl.animation({ leaf = "fade", enabled = false })
hl.animation({ leaf = "border", enabled = true, speed = 2, spring = "easy" })

hl.monitor({ output = "DP-1", mode = "1920x1080@60", position = "0x0", scale = 1, transform = 1 })
hl.monitor({ output = "HDMI-A-1", disabled = true })
hl.window_rule({ name = "hyprland-mod-rule-bbbb0001", match = { class = "^(org.gnome.Calculator)$", float = false }, float = true, opacity = "0.9 0.8", rounding = 4, size = "800 600" })
hl.bind("SUPER + SHIFT + T", hl.dsp.exec_cmd("kitty --title 'x'"), { description = "Terminal ✓", locked = true, repeating = true })
hl.bind("SUPER + 3", hl.dsp.focus({ workspace = 3 }))
hl.bind("XF86AudioMute", hl.dsp.window.close())
hl.on("hyprland.start", function() hl.exec_cmd("waybar --style 'a b'") end)
