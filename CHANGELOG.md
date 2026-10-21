# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.1.0] - 2026-10-21

### Added
- 33 built-in presets (was 12), a protected "My original look" preset and a "Back to my original look" button.
- Bar builder for Waybar: 10 bar presets, 18 modules, drag and drop, live preview, start-on-login switch. Writes only `~/.config/waybar/hyprland-mod/` and warns if another bar (for example Caelestia) is running.
- Keyboard shortcuts editor: key recorder, 40 actions, app picker, conflict warnings, read-only view of existing shortcuts.
- Window rules builder: match by app or title, float, size, center, opacity, workspace, pin and more, with a plain-English summary.
- Displays page: drag arrangement, resolution, refresh rate, scale and rotation, with a 15 second "Keep these settings?" auto-revert.
- Autostart management.

### Fixed
- Restoring a backup wrote invalid Lua on Lua configs.
- Original-backup safety: the first backup of your config is now protected.

## [1.0.0] - 2026-10-21

### Added
- Native GTK4 / libadwaita settings app with 350 Hyprland settings in 12 pages and global search.
- Live animated preview and live apply via `hyprctl`.
- 12 built-in presets, custom presets, import and export of presets and whole setups.
- Safe writes: only `hyprland-mod.lua` / `hyprland-mod.conf` plus one include line.
- Automatic backups, undo / redo and one-click restore.
- Lua and classic `.conf` config support.
- One-command user-local installer and uninstaller.
