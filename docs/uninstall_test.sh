#!/bin/bash
# ══════════════════════════════════════════════════════════════════════════════
#  Tests for docs/uninstall.sh (desktop). Network-free, sandboxed HOME, stubbed
#  system tools. Run: bash docs/uninstall_test.sh
# ══════════════════════════════════════════════════════════════════════════════
set -eo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
PASS=0; FAIL=0
group() { echo ""; echo "  $1"; }
pass()  { PASS=$((PASS + 1)); echo "    ✓  $1"; }
fail()  { FAIL=$((FAIL + 1)); echo "    ✗  $1"; [ -n "${2:-}" ] && echo "       got: $2"; }
assert_exists() { [ -e "$2" ] && pass "$1" || fail "$1" "missing $2"; }
assert_gone()   { [ ! -e "$2" ] && [ ! -L "$2" ] && pass "$1" || fail "$1" "still there: $2"; }

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) echo "  Windows uses the NSIS uninstaller — skipping"; exit 0 ;;
esac

SANDBOX=$(mktemp -d)
trap 'rm -rf "$SANDBOX"' EXIT
export HOME="$SANDBOX/home" XDG_CONFIG_HOME="$SANDBOX/home/.config"
export XDG_DATA_HOME="$SANDBOX/home/.local/share" XDG_CACHE_HOME="$SANDBOX/home/.cache"
export TP_APPLICATIONS_DIR="$SANDBOX/Applications"

# shellcheck source=uninstall.sh
source "$DIR/uninstall.sh"
ok() { :; }; warn() { :; }
pkill() { return 0; }; pgrep() { return 1; }; sudo() { "$@"; }
dpkg() { return 1; }; rpm() { return 1; }; launchctl() { return 0; }; osascript() { return 0; }
update-desktop-database() { return 0; }
SECRET_CALLS="$SANDBOX/secret-calls"
secret-tool() { echo "$*" >> "$SECRET_CALLS"; return 0; }
security() { echo "$*" >> "$SECRET_CALLS"; return 0; }

ID1="11111111-2222-3333-4444-555555555555"

setup_linux() {
  rm -rf "$HOME"; mkdir -p "$HOME/.local/bin" "$HOME/.local/share/tunnel-pilot" \
    "$HOME/.local/share/applications" "$HOME/.local/share/icons" "$XDG_CONFIG_HOME/autostart" \
    "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli"
  touch "$HOME/.local/share/tunnel-pilot/Tunnel.Pilot_2.3.0_amd64.AppImage"
  printf '#!/bin/bash\nexec "%s" "$@"\n' "$HOME/.local/share/tunnel-pilot/Tunnel.Pilot_2.3.0_amd64.AppImage" > "$HOME/.local/bin/tunnel-pilot"
  printf '\177ELF\n' > "$HOME/.local/bin/tunnel-pilot-cli"
  echo "[Desktop Entry]" > "$HOME/.local/share/applications/tunnel_pilot.desktop"
  touch "$HOME/.local/share/icons/tunnel_pilot.png"
  printf '[Desktop Entry]\nName=Tunnel Pilot\nExec=tunnel-pilot --minimized\n' > "$XDG_CONFIG_HOME/autostart/Tunnel Pilot.desktop"
  printf '[Desktop Entry]\nName=Other\nExec=other-app\n' > "$XDG_CONFIG_HOME/autostart/other.desktop"
  echo "{\"forwards\":[{\"id\":\"$ID1\"}]}" > "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot/tunnel_pilot_config.json"
  echo '{}' > "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli/tunnel_pilot_config.json"
  : > "$SECRET_CALLS"
}

group "Linux AppImage install (keep data)"
setup_linux
PURGE=false
uninstall_linux >/dev/null 2>&1
assert_gone "AppImage removed" "$HOME/.local/share/tunnel-pilot"
assert_gone "launcher removed" "$HOME/.local/bin/tunnel-pilot"
assert_gone "menu entry removed" "$HOME/.local/share/applications/tunnel_pilot.desktop"
assert_gone "icon removed" "$HOME/.local/share/icons/tunnel_pilot.png"
assert_gone "login item removed" "$XDG_CONFIG_HOME/autostart/Tunnel Pilot.desktop"
assert_exists "unrelated autostart entry kept" "$XDG_CONFIG_HOME/autostart/other.desktop"
assert_exists "tunnel-pilot-cli binary untouched" "$HOME/.local/bin/tunnel-pilot-cli"
assert_exists "config kept" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot/tunnel_pilot_config.json"
[ ! -s "$SECRET_CALLS" ] && pass "no secrets touched without --purge" || fail "secrets touched"

group "Linux --purge"
setup_linux
PURGE=true
uninstall_linux >/dev/null 2>&1
assert_gone "desktop config purged" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot"
assert_exists "cli config untouched" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli/tunnel_pilot_config.json"
grep -q "clear service tunnel-pilot username $ID1" "$SECRET_CALLS" && pass "secret cleared for each forward id" || fail "secret not cleared"

group "a foreign tunnel-pilot on PATH is left alone"
setup_linux
printf '#!/bin/bash\necho someone else\n' > "$HOME/.local/bin/tunnel-pilot"
PURGE=false
uninstall_linux >/dev/null 2>&1
assert_exists "foreign command kept" "$HOME/.local/bin/tunnel-pilot"

group "macOS"
rm -rf "$HOME" "$TP_APPLICATIONS_DIR"
mkdir -p "$TP_APPLICATIONS_DIR/Tunnel Pilot.app/Contents/MacOS" "$HOME/.local/bin" "$HOME/Library/LaunchAgents" \
  "$HOME/Library/Application Support/com.kalfian.tunnelpilot"
touch "$TP_APPLICATIONS_DIR/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot"
ln -s "$TP_APPLICATIONS_DIR/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot" "$HOME/.local/bin/tunnel-pilot"
echo "<string>/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot</string>" > "$HOME/Library/LaunchAgents/Tunnel Pilot.plist"
echo "<string>/usr/bin/other</string>" > "$HOME/Library/LaunchAgents/other.plist"
echo "{\"forwards\":[{\"id\":\"$ID1\"}]}" > "$HOME/Library/Application Support/com.kalfian.tunnelpilot/tunnel_pilot_config.json"
: > "$SECRET_CALLS"
PURGE=true
uname() { echo Darwin; }
uninstall_macos >/dev/null 2>&1
unset -f uname
assert_gone "app removed" "$TP_APPLICATIONS_DIR/Tunnel Pilot.app"
assert_gone "PATH shim removed" "$HOME/.local/bin/tunnel-pilot"
assert_gone "login item removed" "$HOME/Library/LaunchAgents/Tunnel Pilot.plist"
assert_exists "other LaunchAgent kept" "$HOME/Library/LaunchAgents/other.plist"
assert_gone "config purged" "$HOME/Library/Application Support/com.kalfian.tunnelpilot"
grep -q "delete-generic-password -s tunnel-pilot -a $ID1" "$SECRET_CALLS" && pass "keychain entry deleted" || fail "keychain entry not deleted"

echo ""
TOTAL=$((PASS + FAIL))
if [ "$FAIL" -eq 0 ]; then echo "  ✓  All $TOTAL tests passed"; else echo "  ✗  $FAIL of $TOTAL tests failed"; exit 1; fi
