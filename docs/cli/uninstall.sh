#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot CLI (server edition) — Uninstaller
#  https://github.com/kalfian/tunnel-pilot
#
#  Usage:   curl -fsSL https://kalfian.github.io/tunnel-pilot/cli/uninstall.sh | bash
#           ... | bash -s -- --purge        also delete forwards, settings, the web
#                                           admin password and stored SSH passwords
#           ... | bash -s -- --purge --yes  no confirmation prompt
#
#  Stops and removes the background service (systemd / launchd), stops a running
#  daemon, and removes the `tunnel-pilot-cli` binary. Without --purge the config
#  stays, so a reinstall picks it up again. The desktop app (tunnel-pilot) is a
#  separate product and is never touched.
#  A system-wide install (root, /usr/local/bin, system unit) needs sudo.
# ══════════════════════════════════════════════════════════════════════════════

BINARY_NAME="tunnel-pilot-cli"
IDENTIFIER="com.kalfian.tunnelpilot.cli"
UNIT_NAME="tunnel-pilot-cli.service"
LAUNCHD_LABEL="com.kalfian.tunnelpilot.cli.daemon"
KEYCHAIN_SERVICE="tunnel-pilot-cli"   # the server product's own keychain service
SYSTEM_BIN_DIR="${TP_SYSTEM_BIN_DIR:-/usr/local/bin}"
SYSTEM_UNIT_DIR="${TP_SYSTEM_UNIT_DIR:-/etc/systemd/system}"

PURGE=false
ASSUME_YES=false

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; DIM='\033[2m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}✓${NC}  $1"; }
warn() { echo -e "  ${YELLOW}⚠${NC}  $1"; }
die()  { echo -e "  ${RED}✗${NC}  $1" >&2; exit 1; }

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --purge)   PURGE=true ;;
      --yes|-y)  ASSUME_YES=true ;;
      --help|-h) sed -n '4,18p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null | sed 's/^#  \{0,1\}//'; exit 0 ;;
      *) die "Unknown option: $1 (try --help)" ;;
    esac
    shift
  done
}

is_root() { [ "$(id -u 2>/dev/null || echo 1)" = "0" ]; }

as_root() {
  if is_root; then "$@"; elif command -v sudo >/dev/null; then sudo "$@"; else return 1; fi
}

confirm_purge() {
  $PURGE || return 0
  $ASSUME_YES && return 0
  local answer="n"
  if [ -r /dev/tty ]; then
    read -r -p "  Delete ALL tunnel-pilot-cli forwards, settings and passwords? [y/N] " answer < /dev/tty || answer="n"
  fi
  [[ "$answer" =~ ^[Yy]$ ]] || die "Purge cancelled (nothing was removed). Re-run without --purge to keep your data."
}

remove_path() {
  local path="$1" label="$2"
  if [ -e "$path" ] || [ -L "$path" ]; then
    rm -rf -- "$path" && ok "Removed $label ${DIM}$path${NC}"
  fi
}

config_dir() {
  case "$(uname -s)" in
    Darwin) echo "$HOME/Library/Application Support/$IDENTIFIER" ;;
    *)      echo "${XDG_CONFIG_HOME:-$HOME/.config}/$IDENTIFIER" ;;
  esac
}

# The installed binaries we own (plain executables, not symlinks to elsewhere).
installed_bins() {
  local f
  for f in "$HOME/.local/bin/$BINARY_NAME" "$SYSTEM_BIN_DIR/$BINARY_NAME"; do
    [ -e "$f" ] && echo "$f"
  done
  return 0
}

# Stop + remove the service definitions, whichever exist.
remove_services() {
  case "$(uname -s)" in
    Darwin)
      local plist="$HOME/Library/LaunchAgents/$LAUNCHD_LABEL.plist"
      if [ -f "$plist" ]; then
        launchctl bootout "gui/$(id -u)/$LAUNCHD_LABEL" >/dev/null 2>&1 \
          || launchctl unload "$plist" >/dev/null 2>&1 || true
        remove_path "$plist" "launchd agent"
      fi ;;
    *)
      local user_unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$UNIT_NAME"
      if [ -f "$user_unit" ]; then
        systemctl --user disable --now "$UNIT_NAME" >/dev/null 2>&1 || true
        remove_path "$user_unit" "systemd user unit"
        systemctl --user daemon-reload >/dev/null 2>&1 || true
      fi
      local system_unit="$SYSTEM_UNIT_DIR/$UNIT_NAME"
      if [ -f "$system_unit" ]; then
        as_root systemctl disable --now "$UNIT_NAME" >/dev/null 2>&1 || true
        if as_root rm -f -- "$system_unit"; then
          ok "Removed systemd system unit ${DIM}$system_unit${NC}"
          as_root systemctl daemon-reload >/dev/null 2>&1 || true
        else
          warn "Could not remove $system_unit — re-run with sudo"
        fi
      fi ;;
  esac
}

stop_daemon() {
  if pgrep -f "$BINARY_NAME daemon" >/dev/null 2>&1; then
    pkill -TERM -f "$BINARY_NAME daemon" >/dev/null 2>&1 || true
    sleep 1
    ok "Stopped the running daemon"
  fi
}

remove_binaries() {
  # Read line by line: a path with spaces (e.g. a HOME like "/Users/my name")
  # must stay one path. Binaries are files, so `rm -f`, never `rm -rf`.
  local f
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if [ -w "$(dirname "$f")" ] && rm -f -- "$f"; then
      ok "Removed binary ${DIM}$f${NC}"
    elif as_root rm -f -- "$f"; then
      ok "Removed binary ${DIM}$f${NC}"
    else
      warn "Could not remove $f — re-run with sudo"
    fi
  done < <(installed_bins)
}

purge_data() {
  local dir id count=0
  dir=$(config_dir)
  if [ "$(uname -s)" = "Darwin" ] && [ -f "$dir/tunnel_pilot_config.json" ]; then
    # The macOS build stores passwords in the Keychain, one account per forward id.
    for id in $(grep -oE '"id"[[:space:]]*:[[:space:]]*"[0-9a-fA-F-]{36}"' "$dir/tunnel_pilot_config.json" \
                | grep -oE '[0-9a-fA-F-]{36}' | sort -u); do
      security delete-generic-password -s "$KEYCHAIN_SERVICE" -a "$id" >/dev/null 2>&1 && count=$((count + 1))
    done
    [ "$count" -gt 0 ] && ok "Removed $count stored password(s) from the Keychain"
  fi
  remove_path "$dir" "config, secrets and web admin password"
  remove_path "$HOME/Library/Logs/tunnel-pilot-cli.log" "log"
}

main() {
  parse_args "$@"
  echo ""
  echo -e "  ${BOLD}Uninstalling tunnel-pilot-cli${NC}"
  echo ""
  case "$(uname -s)" in
    Linux|Darwin) ;;
    *) die "tunnel-pilot-cli only exists on Linux and macOS." ;;
  esac
  confirm_purge
  remove_services
  stop_daemon
  remove_binaries
  $PURGE && purge_data
  echo ""
  if $PURGE; then
    ok "tunnel-pilot-cli and all its data are removed."
  else
    ok "tunnel-pilot-cli is removed. Config kept in $(config_dir) (use --purge to delete it)."
  fi
  echo ""
}

if [[ "${BASH_SOURCE[0]:-$0}" == "${0}" ]]; then
  main "$@"
fi
