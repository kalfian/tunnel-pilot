#!/bin/bash
# ══════════════════════════════════════════════════════════════════════════════
#  Tests for docs/cli/install.sh and docs/cli/uninstall.sh (tunnel-pilot-cli).
#  Network-free: curl, systemctl, launchctl, sudo and id are stubbed, and HOME
#  points at a throwaway sandbox. Run: bash docs/cli/install_test.sh
# ══════════════════════════════════════════════════════════════════════════════
set -eo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
PASS=0; FAIL=0
group() { echo ""; echo "  $1"; }
pass()  { PASS=$((PASS + 1)); echo "    ✓  $1"; }
fail()  { FAIL=$((FAIL + 1)); echo "    ✗  $1"; [ -n "${2:-}" ] && echo "       got: $2"; }
assert_eq() { [ "$2" = "$3" ] && pass "$1" || fail "$1" "$2 (expected $3)"; }
assert_exists() { [ -e "$2" ] && pass "$1" || fail "$1" "missing $2"; }
assert_gone()   { [ ! -e "$2" ] && [ ! -L "$2" ] && pass "$1" || fail "$1" "still there: $2"; }

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) echo "  tunnel-pilot-cli is Unix-only — skipping"; exit 0 ;;
esac

SANDBOX=$(mktemp -d)
trap 'rm -rf "$SANDBOX"' EXIT
export HOME="$SANDBOX/home"
mkdir -p "$HOME"
export TP_SYSTEM_BIN_DIR="$SANDBOX/usr-local-bin" TP_SYSTEM_UNIT_DIR="$SANDBOX/etc-systemd"
export XDG_CONFIG_HOME="$HOME/.config"

# ── install.sh ────────────────────────────────────────────────────────────────
# shellcheck source=install.sh
source "$DIR/install.sh"
print_step() { :; }; print_success() { :; }; print_warn() { :; }; print_info() { :; }
print_error() { echo "ERROR: $1" >&2; exit 1; }
id() { echo 1000; }   # never touch the real /usr/local/bin

group "detect_platform"
run_detect() {
  local os="$1" arch="$2"
  ( uname() { case "${1:-}" in -m) echo "$arch" ;; *) echo "$os" ;; esac; }
    detect_platform; echo "$ASSET_PATTERN" )
}
assert_eq "Linux x86_64"  "$(run_detect Linux x86_64)"  'tunnel-pilot-cli-linux-x86_64\.tar\.gz$'
assert_eq "Linux arm64"   "$(run_detect Linux arm64)"   'tunnel-pilot-cli-linux-aarch64\.tar\.gz$'
assert_eq "macOS"         "$(run_detect Darwin arm64)"  'tunnel-pilot-cli-macos-universal\.tar\.gz$'
( run_detect MINGW64_NT x86_64 ) >/dev/null 2>&1 && fail "Windows must be refused" || pass "Windows is refused"
( run_detect Linux riscv64 ) >/dev/null 2>&1 && fail "unknown arch must be refused" || pass "unknown arch is refused"

group "json_asset_url skips the .sha256 sidecar"
JSON='{"tag_name":"v2.3.0","assets":[
 {"name":"tunnel-pilot-cli-linux-x86_64.tar.gz.sha256","browser_download_url":"https://x/tunnel-pilot-cli-linux-x86_64.tar.gz.sha256"},
 {"name":"tunnel-pilot-cli-linux-x86_64.tar.gz","browser_download_url":"https://x/tunnel-pilot-cli-linux-x86_64.tar.gz"}]}'
assert_eq "x86_64 tarball" "$(json_asset_url "$JSON" 'tunnel-pilot-cli-linux-x86_64\.tar\.gz$')" "https://x/tunnel-pilot-cli-linux-x86_64.tar.gz"
assert_eq "tag" "$(json_tag "$JSON")" "v2.3.0"

group "install_binary (simulated download)"
mkdir -p "$SANDBOX/pkg"
printf '\177ELF fake tunnel-pilot-cli\n' > "$SANDBOX/pkg/tunnel-pilot-cli"
chmod +x "$SANDBOX/pkg/tunnel-pilot-cli"
tar -czf "$SANDBOX/cli.tar.gz" -C "$SANDBOX/pkg" tunnel-pilot-cli
if command -v sha256sum >/dev/null; then GOOD=$(sha256sum "$SANDBOX/cli.tar.gz" | awk '{print $1}'); else GOOD=$(shasum -a 256 "$SANDBOX/cli.tar.gz" | awk '{print $1}'); fi
SUM="$GOOD"
curl() {
  local out="" url=""
  while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift ;; http*) url="$1" ;; esac; shift; done
  case "$url" in
    *.sha256) [ -n "$SUM" ] && echo "$SUM  tunnel-pilot-cli-linux-x86_64.tar.gz" || return 22 ;;
    *) cp "$SANDBOX/cli.tar.gz" "$out" ;;
  esac
}
ASSET_URL="https://x/tunnel-pilot-cli-linux-x86_64.tar.gz"; ASSET_NAME="tunnel-pilot-cli-linux-x86_64.tar.gz"; VERSION="v2.3.0"
DEST="$HOME/.local/bin/tunnel-pilot-cli"
( install_binary ) >/dev/null 2>&1 && pass "installs with a good checksum" || fail "install with good checksum"
assert_exists "binary is in ~/.local/bin" "$DEST"
[ -x "$DEST" ] && pass "binary is executable" || fail "binary is executable"

# A symlink at the destination is replaced, never written through.
rm -f "$DEST"; echo "keep me" > "$SANDBOX/victim"; ln -s "$SANDBOX/victim" "$DEST"
( install_binary ) >/dev/null 2>&1 || true
assert_eq "symlink target untouched" "$(cat "$SANDBOX/victim")" "keep me"
[ ! -L "$DEST" ] && pass "destination is a real file now" || fail "destination is still a symlink"

SUM="0000000000000000000000000000000000000000000000000000000000000000"
rm -f "$DEST"
( install_binary ) >/dev/null 2>&1 && fail "bad checksum must abort" || pass "bad checksum aborts"
assert_gone "nothing installed on mismatch" "$DEST"
SUM=""
( install_binary ) >/dev/null 2>&1 && fail "missing checksum must abort" || pass "missing checksum aborts"
unset -f curl

# ── uninstall.sh ──────────────────────────────────────────────────────────────
# shellcheck source=uninstall.sh
source "$DIR/uninstall.sh"
ok() { :; }; warn() { :; }
systemctl() { echo "systemctl $*" >> "$SANDBOX/calls"; return 0; }
launchctl() { echo "launchctl $*" >> "$SANDBOX/calls"; return 0; }
pgrep() { return 1; }
sudo() { "$@"; }
id() { echo 1000; }

setup_install() {
  mkdir -p "$HOME/.local/bin" "$TP_SYSTEM_BIN_DIR" "$XDG_CONFIG_HOME/systemd/user" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot"
  printf '\177ELF\n' > "$HOME/.local/bin/tunnel-pilot-cli"
  printf '\177ELF\n' > "$TP_SYSTEM_BIN_DIR/tunnel-pilot-cli"
  echo "[Unit]" > "$XDG_CONFIG_HOME/systemd/user/tunnel-pilot-cli.service"
  echo '{"forwards":[]}' > "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli/tunnel_pilot_config.json"
  echo '{"forwards":[]}' > "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot/tunnel_pilot_config.json"
  printf '#!/bin/bash\nexec desktop\n' > "$HOME/.local/bin/tunnel-pilot"
  : > "$SANDBOX/calls"
}

group "uninstall (keep data)"
setup_install
PURGE=false; ASSUME_YES=false
( uname() { echo Linux; }; remove_services; remove_binaries ) >/dev/null 2>&1
assert_gone "user unit removed" "$XDG_CONFIG_HOME/systemd/user/tunnel-pilot-cli.service"
grep -q "disable --now tunnel-pilot-cli.service" "$SANDBOX/calls" && pass "service stopped + disabled" || fail "service not disabled"
assert_gone "~/.local/bin binary removed" "$HOME/.local/bin/tunnel-pilot-cli"
assert_gone "system binary removed" "$TP_SYSTEM_BIN_DIR/tunnel-pilot-cli"
assert_exists "config kept without --purge" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli/tunnel_pilot_config.json"
assert_exists "desktop launcher untouched" "$HOME/.local/bin/tunnel-pilot"

group "uninstall --purge"
setup_install
( uname() { echo Linux; }; PURGE=true; purge_data ) >/dev/null 2>&1
assert_gone "cli config purged" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot.cli"
assert_exists "desktop config untouched" "$XDG_CONFIG_HOME/com.kalfian.tunnelpilot/tunnel_pilot_config.json"

group "uninstall with a space in HOME"
SPACED="$SANDBOX/my home"
mkdir -p "$SANDBOX/my/precious" "$SPACED/.local/bin"
echo data > "$SANDBOX/my/precious/file"
printf '\177ELF\n' > "$SPACED/.local/bin/tunnel-pilot-cli"
( HOME="$SPACED"; uname() { echo Linux; }; remove_binaries ) >/dev/null 2>&1
assert_gone "binary under the spaced HOME removed" "$SPACED/.local/bin/tunnel-pilot-cli"
assert_exists "unrelated sibling directory untouched" "$SANDBOX/my/precious/file"

echo ""
TOTAL=$((PASS + FAIL))
if [ "$FAIL" -eq 0 ]; then echo "  ✓  All $TOTAL tests passed"; else echo "  ✗  $FAIL of $TOTAL tests failed"; exit 1; fi
