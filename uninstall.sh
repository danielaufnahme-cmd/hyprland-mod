#!/usr/bin/env bash
# Removes exactly what install.sh installed. Your backups, presets and Hyprland
# config are never touched (unless you pass --purge and confirm, which removes
# only Hyprland Mod's own data folder, never your Hyprland config).
set -euo pipefail

APP_ID="io.github.HyprlandMod"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
BASE="$DATA_HOME/hyprland-mod"
APP_DIR="$BASE/app"
LAUNCHER="$HOME/.local/bin/hyprland-mod"
DESKTOP_FILE="$DATA_HOME/applications/$APP_ID.desktop"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"

if [ -t 1 ]; then
  B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; C=$'\033[36m'; N=$'\033[0m'
else
  B=""; G=""; Y=""; C=""; N=""
fi
ok() { printf '  %s✓%s %s\n' "$G" "$N" "$*"; }

PURGE=0
for a in "$@"; do
  case "$a" in
    --purge) PURGE=1 ;;
    -h|--help) printf 'Usage: uninstall.sh [--purge]\n'; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$a" >&2; exit 2 ;;
  esac
done

printf '\n%sUninstalling Hyprland Mod%s\n\n' "$B" "$N"
rm -rf "$APP_DIR";                                ok "Application files"
rm -f "$LAUNCHER";                                ok "Launcher"
rm -f "$DESKTOP_FILE";                            ok "Menu entry"
rm -f "$ICON_DIR/$APP_ID.svg" "$ICON_DIR/$APP_ID-symbolic.svg"; ok "Icon"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DATA_HOME/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -d "$DATA_HOME/icons/hicolor" ]; then
  gtk-update-icon-cache -q -t -f "$DATA_HOME/icons/hicolor" >/dev/null 2>&1 || true
fi

if [ "$PURGE" -eq 1 ] && [ -d "$BASE" ]; then
  printf '\n  Remove ALL Hyprland Mod data in %s (backups, presets)? [y/N] ' "$BASE"
  reply=""
  { read -r reply </dev/tty; } 2>/dev/null || read -r reply || true
  case "$reply" in
    y|Y|yes|YES) rm -rf "$BASE"; ok "Removed $BASE" ;;
    *) printf '  Kept %s\n' "$BASE" ;;
  esac
fi

printf '\n%sDone.%s Your Hyprland config was not modified.\n' "$G$B" "$N"
if [ -d "$BASE" ]; then
  printf '  Your backups and presets are kept in: %s\n' "$BASE"
  printf '  Delete that folder yourself, or re-run with --purge.\n'
fi
printf '\n  Hyprland Mod added a single include line to your Hyprland config. To remove it,\n'
printf '  delete this line (Lua configs) from hyprland.lua:\n'
printf '      pcall(require, "hyprland-mod")\n'
printf '  or this line (classic configs) from hyprland.conf:\n'
printf '      source = ~/.config/hypr/hyprland-mod.conf\n'
printf '  You can also delete hyprland-mod.lua / hyprland-mod.conf from ~/.config/hypr/.\n\n'
