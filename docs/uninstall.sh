#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot (desktop) — Uninstaller
#  https://github.com/kalfian/tunnel-pilot
#
#  Usage:   curl -fsSL https://kalfian.github.io/tunnel-pilot/uninstall.sh | bash
#           ... | bash -s -- --purge        also delete forwards, settings and
#                                           this app's stored SSH passwords
#           ... | bash -s -- --purge --yes  no confirmation prompt
#
#  Removes the app (macOS .app / Linux AppImage or .deb/.rpm), its launcher and
#  menu entries, the login item, and the `tunnel-pilot` PATH symlink. Without
#  --purge your forwards, settings and passwords stay, so a reinstall picks them
#  up again. The server edition (tunnel-pilot-cli) is a separate product with its
#  own config and is never touched: .../tunnel-pilot/cli/uninstall.sh
# ══════════════════════════════════════════════════════════════════════════════

APP_NAME="Tunnel Pilot"
BINARY_NAME="tunnel-pilot"
IDENTIFIER="com.kalfian.tunnelpilot"
KEYCHAIN_SERVICE="tunnel-pilot"
INSTALL_DIR="${TP_APPLICATIONS_DIR:-/Applications}"

PURGE=false
ASSUME_YES=false

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; DIM='\033[2m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}✓${NC}  $1"; }
skip() { echo -e "  ${DIM}·  $1${NC}"; }
warn() { echo -e "  ${YELLOW}⚠${NC}  $1"; }
die()  { echo -e "  ${RED}✗${NC}  $1" >&2; exit 1; }

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --purge)   PURGE=true ;;
      --yes|-y)  ASSUME_YES=true ;;
      --help|-h) sed -n '4,17p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null | sed 's/^#  \{0,1\}//'; exit 0 ;;
      *) die "Unknown option: $1 (try --help)" ;;
    esac
    shift
  done
}

confirm_purge() {
  $PURGE || return 0
  $ASSUME_YES && return 0
  local answer="n"
  if [ -r /dev/tty ]; then
    read -r -p "  Delete ALL Tunnel Pilot forwards, settings and stored passwords? [y/N] " answer < /dev/tty || answer="n"
  fi
  [[ "$answer" =~ ^[Yy]$ ]] || die "Purge cancelled (nothing was removed). Re-run without --purge to keep your data."
}

# Remove a file or symlink, reporting what happened.
remove_path() {
  local path="$1" label="$2"
  if [ -e "$path" ] || [ -L "$path" ]; then
    rm -rf -- "$path" && ok "Removed $label ${DIM}$path${NC}"
  fi
}

# A `tunnel-pilot` on PATH that is ours: the PATH-shim symlink into the app or
# AppImage, or the installer's launcher script. Anything else is left alone.
is_our_launcher() {
  local f="$1"
  if [ -L "$f" ]; then
    case "$(readlink "$f")" in
      *"$APP_NAME.app"*|*/.local/share/tunnel-pilot/*|*tunnel-pilot*.AppImage|*/usr/bin/tunnel-pilot) return 0 ;;
    esac
    return 1
  fi
  [ -f "$f" ] && grep -q '\.AppImage' "$f" 2>/dev/null
}

remove_launchers() {
  local f
  for f in "$HOME/.local/bin/$BINARY_NAME" "/usr/local/bin/$BINARY_NAME"; do
    if is_our_launcher "$f"; then
      if [ -w "$(dirname "$f")" ]; then
        remove_path "$f" "command"
      elif command -v sudo >/dev/null; then
        sudo rm -f -- "$f" && ok "Removed command ${DIM}$f${NC} (sudo)"
      else
        warn "Cannot remove $f (no write access)"
      fi
    fi
  done
}

# Forward ids from the config file: the keychain/secret-service account names.
forward_ids() {
  local config="$1"
  [ -f "$config" ] || return 0
  grep -oE '"id"[[:space:]]*:[[:space:]]*"[0-9a-fA-F-]{36}"' "$config" \
    | grep -oE '[0-9a-fA-F-]{36}' | sort -u
}

purge_secrets() {
  local config="$1" id count=0
  for id in $(forward_ids "$config"); do
    case "$(uname -s)" in
      Darwin)
        security delete-generic-password -s "$KEYCHAIN_SERVICE" -a "$id" >/dev/null 2>&1 && count=$((count + 1)) ;;
      *)
        if command -v secret-tool >/dev/null; then
          secret-tool clear service "$KEYCHAIN_SERVICE" username "$id" >/dev/null 2>&1 && count=$((count + 1))
        fi ;;
    esac
  done
  [ "$count" -gt 0 ] && ok "Removed $count stored password(s) from the keychain"
  return 0
}

uninstall_macos() {
  if pgrep -f "$APP_NAME.app" >/dev/null 2>&1; then
    osascript -e "tell application \"$APP_NAME\" to quit" >/dev/null 2>&1 || true
    sleep 1
  fi
  remove_path "$INSTALL_DIR/$APP_NAME.app" "app"
  # Login item: the LaunchAgent the autostart plugin wrote points at the .app.
  local plist
  for plist in "$HOME/Library/LaunchAgents/"*.plist; do
    [ -f "$plist" ] || continue
    if grep -q "$APP_NAME.app" "$plist" 2>/dev/null; then
      launchctl unload "$plist" >/dev/null 2>&1 || true
      remove_path "$plist" "login item"
    fi
  done
  remove_launchers
  if $PURGE; then
    local support="$HOME/Library/Application Support/$IDENTIFIER"
    purge_secrets "$support/tunnel_pilot_config.json"
    remove_path "$support" "config"
    remove_path "$HOME/Library/Caches/$IDENTIFIER" "cache"
    remove_path "$HOME/Library/WebKit/$IDENTIFIER" "web data"
    remove_path "$HOME/Library/Logs/$IDENTIFIER" "logs"
  fi
}

uninstall_linux() {
  pkill -f "$HOME/.local/share/tunnel-pilot/" >/dev/null 2>&1 || true
  if command -v dpkg >/dev/null && dpkg -s "$BINARY_NAME" >/dev/null 2>&1; then
    sudo apt-get remove -y "$BINARY_NAME" && ok "Removed the .deb package"
  elif command -v rpm >/dev/null && rpm -q "$BINARY_NAME" >/dev/null 2>&1; then
    if command -v dnf >/dev/null; then sudo dnf remove -y "$BINARY_NAME"; else sudo rpm -e "$BINARY_NAME"; fi
    ok "Removed the .rpm package"
  fi
  remove_path "$HOME/.local/share/tunnel-pilot" "AppImage"
  remove_path "$HOME/.local/share/applications/tunnel_pilot.desktop" "menu entry"
  remove_path "$HOME/Desktop/tunnel_pilot.desktop" "desktop shortcut"
  remove_path "$HOME/.local/share/icons/tunnel_pilot.png" "icon"
  local entry
  for entry in "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/"*.desktop; do
    [ -f "$entry" ] || continue
    if grep -qE "tunnel-pilot|$APP_NAME" "$entry" 2>/dev/null && ! grep -q "tunnel-pilot-cli" "$entry"; then
      remove_path "$entry" "login item"
    fi
  done
  remove_launchers
  command -v update-desktop-database >/dev/null && \
    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
  if $PURGE; then
    local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/$IDENTIFIER"
    purge_secrets "$config_dir/tunnel_pilot_config.json"
    remove_path "$config_dir" "config"
    remove_path "${XDG_DATA_HOME:-$HOME/.local/share}/$IDENTIFIER" "app data"
    remove_path "${XDG_CACHE_HOME:-$HOME/.cache}/$IDENTIFIER" "cache"
  fi
}

main() {
  parse_args "$@"
  echo ""
  echo -e "  ${BOLD}Uninstalling Tunnel Pilot (desktop)${NC}"
  echo ""
  confirm_purge
  case "$(uname -s)" in
    Darwin) uninstall_macos ;;
    Linux)  uninstall_linux ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "  On Windows use Settings → Apps → Installed apps → Tunnel Pilot → Uninstall."
      echo "  Your config stays in %APPDATA%\\$IDENTIFIER (delete it to remove everything)."
      exit 0 ;;
    *) die "Unsupported platform: $(uname -s)" ;;
  esac
  echo ""
  if $PURGE; then
    ok "Tunnel Pilot and all its data are removed."
  else
    ok "Tunnel Pilot is removed. Your forwards and settings were kept (use --purge to delete them)."
  fi
  echo ""
}

if [[ "${BASH_SOURCE[0]:-$0}" == "${0}" ]]; then
  main "$@"
fi
