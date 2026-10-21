# Contributing to Hyprland Mod

Thanks for helping out. Bug reports, presets, schema fixes and code are all welcome.

## Run from source

```bash
git clone https://github.com/danielaufnahme-cmd/hyprland-mod
cd hyprland-mod
python3 -m hyprland_mod
```

Requirements: Python 3.11+, PyGObject, GTK 4, libadwaita 1.5+.

## Layout

- `hyprland_mod/core/` - config model, parsing, writing, backups, live apply
- `hyprland_mod/ui/` - GTK4 / libadwaita pages and widgets
- `data/schema/` - the setting definitions (one JSON file per page)
- `data/presets/` - built-in presets
- `styles/` - CSS
- `tests/` - test suite (`python3 -m pytest`)

## Guidelines

- The app must only ever write its own `hyprland-mod.lua` / `hyprland-mod.conf` and back up before writing. Never edit the user's main config beyond the single include line.
- Keep the UI clean and minimal, with fast animations.
- Add or update tests for core changes.
- One focused change per pull request, with a clear description.

## Sharing a preset

Export a preset from the Presets page and open a pull request adding it to `data/presets/` with a screenshot.

## Reporting bugs

Please include your Hyprland version, whether you use a Lua or `.conf` config, and the output of `python3 -m hyprland_mod` run from a terminal.
