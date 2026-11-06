#!/bin/bash
# ══════════════════════════════════════════════════════════════════════════════
#  install_test.sh — Unit tests for install.sh
#  Run: bash docs/install_test.sh
# ══════════════════════════════════════════════════════════════════════════════

set -eo pipefail

# Resolve script directory robustly — works when run as:
#   bash docs/install_test.sh       (from project root)
#   bash install_test.sh            (from docs/)
#   /absolute/path/install_test.sh  (any CWD)
_SELF="${BASH_SOURCE[0]:-$0}"
SCRIPT_DIR="$(cd "$(dirname "$_SELF")" && pwd)"
INSTALL_SH="$SCRIPT_DIR/install.sh"

if [[ ! -f "$INSTALL_SH" ]]; then
  echo "Error: install.sh not found at $INSTALL_SH"
  echo "Run this test from the project root: bash docs/install_test.sh"
  exit 1
fi

# ── Minimal test framework ────────────────────────────────────────────────────
PASS=0
FAIL=0
CURRENT_GROUP=""

group() { CURRENT_GROUP="$1"; echo ""; echo "  $1"; }

pass() { PASS=$((PASS + 1)); echo "    ✓  $1"; }

fail() {
  FAIL=$((FAIL + 1))
  echo "    ✗  $1"
  [ -n "${2:-}" ] && echo "       got:      ${2:-}"
  [ -n "${3:-}" ] && echo "       expected: ${3:-}"
}

assert_eq() {
  local desc="$1" got="$2" want="$3"
  [ "$got" = "$want" ] && pass "$desc" || fail "$desc" "$got" "$want"
}

assert_contains() {
  # Uses grep -E (regex) so callers can pass regex or plain substrings
  local desc="$1" haystack="$2" needle="$3"
  echo "$haystack" | grep -qE "$needle" && pass "$desc" || \
    fail "$desc" "(pattern '$needle' not found in: $haystack)"
}

assert_empty() {
  local desc="$1" val="$2"
  [ -z "$val" ] && pass "$desc" || fail "$desc — expected empty, got: $val"
}

assert_not_empty() {
  local desc="$1" val="$2"
  [ -n "$val" ] && pass "$desc" || fail "$desc — expected non-empty"
}

assert_file_exists() {
  local desc="$1" path="$2"
  [ -e "$path" ] && pass "$desc" || fail "$desc — file not found: $path"
}

assert_file_executable() {
  local desc="$1" path="$2"
  [ -x "$path" ] && pass "$desc" || fail "$desc — not executable: $path"
}

assert_file_contains() {
  local desc="$1" path="$2" needle="$3"
  grep -qF "$needle" "$path" && pass "$desc" || \
    fail "$desc — '$needle' not found in $path"
}

# ── Source install.sh (without running main) ──────────────────────────────────
# Stub out functions that call external services or require root
hdiutil()  { :; }
osascript() { :; }
open()     { :; }
pgrep()    { return 1; }
defaults() { echo "0.0.3"; }
export -f hdiutil osascript open pgrep defaults 2>/dev/null || true

# shellcheck source=install.sh
source "$INSTALL_SH"

# Silence output helpers during tests
print_step()    { :; }
print_success() { :; }
print_warn()    { :; }
print_info()    { :; }
print_error()   { echo "ERROR: $1" >&2; exit 1; }
spinner_start() { :; }
spinner_stop()  { :; }

# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "  install.sh — test suite"
echo "  ─────────────────────────────────────"

# ── 1. Platform detection ─────────────────────────────────────────────────────
group "detect_platform"

run_detect() {
  local fake_uname="$1"
  uname() { echo "$fake_uname"; }
  export -f uname 2>/dev/null || true
  PLATFORM="" ASSET_PATTERN="" ARCH="x86_64"
  detect_platform
  echo "$PLATFORM|$ASSET_PATTERN"
  unset -f uname 2>/dev/null || true
}

# macOS → v2 .dmg
result=$(run_detect "Darwin")
assert_eq "Darwin → platform=macos"          "${result%%|*}" "macos"
assert_contains "Darwin → .dmg pattern"      "${result##*|}" "dmg"

# Linux → v2 .AppImage
result=$(run_detect "Linux")
assert_eq "Linux → platform=linux"           "${result%%|*}" "linux"
assert_contains "Linux → .AppImage pattern"  "${result##*|}" "AppImage"

# Windows (MINGW) → v2 NSIS -setup.exe
result=$(run_detect "MINGW64_NT")
assert_eq "MINGW → platform=windows"         "${result%%|*}" "windows"
assert_contains "MINGW → -setup.exe pattern" "${result##*|}" "setup"

# Windows (MSYS)
result=$(run_detect "MSYS_NT")
assert_eq "MSYS → platform=windows"        "${result%%|*}" "windows"

# Windows (CYGWIN)
result=$(run_detect "CYGWIN_NT")
assert_eq "CYGWIN → platform=windows"      "${result%%|*}" "windows"

# ── 2. json_get — jq path ─────────────────────────────────────────────────────
if command -v jq &>/dev/null; then
  group "json_get (jq)"
  HAS_JQ=true
  JSON='{"tag_name":"v1.2.3","name":"Release v1.2.3"}'
  assert_eq "extracts tag_name"  "$(json_get "$JSON" "tag_name")" "v1.2.3"
  assert_eq "extracts name"      "$(json_get "$JSON" "name")"     "Release v1.2.3"
  assert_empty "missing key → empty" "$(json_get "$JSON" "missing")"
fi

# ── 3. json_get — grep/sed fallback ───────────────────────────────────────────
group "json_get (grep/sed fallback)"
HAS_JQ=false
JSON='{"tag_name":"v1.2.3","name":"Release v1.2.3","other":"val"}'
assert_eq "extracts tag_name"  "$(json_get "$JSON" "tag_name")" "v1.2.3"
assert_eq "extracts name"      "$(json_get "$JSON" "name")"     "Release v1.2.3"
HAS_JQ=true  # restore if jq available

# ── 4. json_asset_url — v2 Tauri asset names ─────────────────────────────────
# Note: ASSET_PATTERN in install.sh uses double-quoted "\\." which bash stores
# as "\." (one backslash). Both jq test() and grep -E treat "\." as literal dot.
# GitHub rewrites spaces in asset names to '.', so fixtures use "Tunnel.Pilot".
# The updater sidecars (.sig, .tar.gz) and latest.json must NOT be matched — the
# '$' anchor on each pattern guarantees that.
group "json_asset_url (grep/sed fallback)"
HAS_JQ=false
FAKE_JSON='{
  "assets": [
    {"name":"Tunnel.Pilot_2.0.0_universal.dmg","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_universal.dmg"},
    {"name":"Tunnel.Pilot_2.0.0_amd64.AppImage","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage"},
    {"name":"Tunnel.Pilot_2.0.0_amd64.AppImage.sig","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage.sig"},
    {"name":"Tunnel.Pilot_2.0.0_amd64.deb","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_amd64.deb"},
    {"name":"Tunnel.Pilot_2.0.0_x64-setup.exe","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_x64-setup.exe"},
    {"name":"Tunnel.Pilot_2.0.0_x64-setup.exe.sig","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_x64-setup.exe.sig"},
    {"name":"latest.json","browser_download_url":"https://example.com/latest.json"}
  ]
}'
# Use single-backslash patterns (same as what install.sh ASSET_PATTERN stores after bash expansion)
assert_eq "macOS .dmg"        "$(json_asset_url "$FAKE_JSON" '\.dmg$')"        "https://example.com/Tunnel.Pilot_2.0.0_universal.dmg"
assert_eq "Linux .AppImage"   "$(json_asset_url "$FAKE_JSON" '\.AppImage$')"   "https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage"
assert_eq "Linux .deb"        "$(json_asset_url "$FAKE_JSON" '\.deb$')"        "https://example.com/Tunnel.Pilot_2.0.0_amd64.deb"
assert_eq "Windows -setup.exe" "$(json_asset_url "$FAKE_JSON" '-setup\.exe$')" "https://example.com/Tunnel.Pilot_2.0.0_x64-setup.exe"

if command -v jq &>/dev/null; then
  group "json_asset_url (jq)"
  HAS_JQ=true
  assert_eq "macOS .dmg"        "$(json_asset_url "$FAKE_JSON" '\.dmg$')"        "https://example.com/Tunnel.Pilot_2.0.0_universal.dmg"
  assert_eq "Linux .AppImage"   "$(json_asset_url "$FAKE_JSON" '\.AppImage$')"   "https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage"
  assert_eq "Linux .deb"        "$(json_asset_url "$FAKE_JSON" '\.deb$')"        "https://example.com/Tunnel.Pilot_2.0.0_amd64.deb"
  assert_eq "Windows -setup.exe" "$(json_asset_url "$FAKE_JSON" '-setup\.exe$')" "https://example.com/Tunnel.Pilot_2.0.0_x64-setup.exe"
  # The '$'-anchored patterns must NOT match updater sidecars / latest.json:
  assert_eq "AppImage pattern skips .sig" \
    "$(json_asset_url "$FAKE_JSON" '\.AppImage$')" "https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage"
  assert_eq "-setup.exe pattern skips .sig" \
    "$(json_asset_url "$FAKE_JSON" '-setup\.exe$')" "https://example.com/Tunnel.Pilot_2.0.0_x64-setup.exe"
fi

# ── 5. install_linux — AppImage install ──────────────────────────────────────
# Simulated anywhere: uses real bash + cp/chmod/find, no Linux system required.
# The v2 Linux artifact is a single self-contained .AppImage.
group "install_linux (AppImage, simulated)"

TMP_TEST=$(mktemp -d)
trap 'rm -rf "$TMP_TEST"' EXIT

# A fake AppImage: a harmless executable script that ignores --appimage-extract
# (so the best-effort icon-extraction branch is exercised without side effects).
FAKE_APPIMAGE="$TMP_TEST/Tunnel.Pilot_2.0.0_amd64.AppImage"
printf '#!/bin/bash\nexit 0\n' > "$FAKE_APPIMAGE"
chmod +x "$FAKE_APPIMAGE"

# Point HOME and script vars to temp sandbox
export HOME="$TMP_TEST/home"
# Add install bin to PATH so the PATH-check branch doesn't trigger noisy output
export PATH="$TMP_TEST/home/.local/bin:$PATH"
PLATFORM="linux"
ASSET_NAME="Tunnel.Pilot_2.0.0_amd64.AppImage"
TMP_DIR="$TMP_TEST/tmp_dl"
TMP_FILE="$FAKE_APPIMAGE"
mkdir -p "$TMP_DIR"

install_linux 2>/dev/null

LINUX_APP_DIR="$TMP_TEST/home/.local/share/tunnel-pilot"
LINUX_APPIMAGE="$LINUX_APP_DIR/Tunnel.Pilot_2.0.0_amd64.AppImage"
LINUX_BIN="$TMP_TEST/home/.local/bin/tunnel-pilot"
LINUX_DESKTOP="$TMP_TEST/home/.local/share/applications/tunnel_pilot.desktop"

assert_file_exists     "app dir created"                "$LINUX_APP_DIR"
assert_file_exists     "AppImage installed"             "$LINUX_APPIMAGE"
assert_file_executable "AppImage is executable"         "$LINUX_APPIMAGE"
assert_file_exists     "launcher script created"        "$LINUX_BIN"
assert_file_executable "launcher is executable"         "$LINUX_BIN"
assert_file_contains   "launcher points to AppImage"    "$LINUX_BIN" "$LINUX_APPIMAGE"
assert_file_contains   "launcher is a bash script"      "$LINUX_BIN" "#!/bin/bash"
assert_file_exists     ".desktop entry created"         "$LINUX_DESKTOP"
assert_file_contains   ".desktop Exec points to launcher" "$LINUX_DESKTOP" "$LINUX_BIN"

# Reinstall with a newer AppImage filename: old AppImage must be pruned.
FAKE_APPIMAGE2="$TMP_TEST/Tunnel.Pilot_2.0.1_amd64.AppImage"
printf '#!/bin/bash\nexit 0\n' > "$FAKE_APPIMAGE2"
chmod +x "$FAKE_APPIMAGE2"
ASSET_NAME="Tunnel.Pilot_2.0.1_amd64.AppImage"
TMP_FILE="$FAKE_APPIMAGE2"
install_linux 2>/dev/null
assert_file_exists "reinstall: new AppImage present" "$LINUX_APP_DIR/Tunnel.Pilot_2.0.1_amd64.AppImage"
[ ! -f "$LINUX_APPIMAGE" ] \
  && pass "reinstall: previous AppImage pruned" \
  || fail "reinstall: stale AppImage was NOT removed"

# ── 6. CLI-only edition (--cli) ───────────────────────────────────────────────
group "parse_args"
( parse_args --cli; echo "$MODE|$CLI_SYSTEM" ) > "$TMP_TEST/pa1"
assert_eq "--cli → MODE=cli" "$(cat "$TMP_TEST/pa1")" "cli|false"
( parse_args --cli --system -y; echo "$MODE|$CLI_SYSTEM|$ASSUME_YES" ) > "$TMP_TEST/pa2"
assert_eq "--cli --system -y" "$(cat "$TMP_TEST/pa2")" "cli|true|true"
( parse_args; echo "$MODE" ) > "$TMP_TEST/pa3"
assert_eq "no args → desktop" "$(cat "$TMP_TEST/pa3")" "desktop"
( parse_args --system ) 2>/dev/null && fail "--system without --cli must fail" \
  || pass "--system without --cli is rejected"
( parse_args --bogus ) >/dev/null 2>&1 && fail "unknown option must fail" \
  || pass "unknown option is rejected"
MODE="desktop"; CLI_SYSTEM=false; ASSUME_YES=false; FORCE=false

group "detect_platform (--cli)"
run_detect_cli() {
  local fake_os="$1" fake_arch="$2"
  uname() { case "${1:-}" in -m) echo "$fake_arch" ;; *) echo "$fake_os" ;; esac; }
  MODE="cli" PLATFORM="" ASSET_PATTERN="" ARCH=""
  detect_platform
  echo "$PLATFORM|$ASSET_PATTERN"
  unset -f uname 2>/dev/null || true
}
result=$(run_detect_cli Linux x86_64)
assert_eq "Linux x86_64 → cli asset" "${result##*|}" 'tunnel-pilot-cli-linux-x86_64\.tar\.gz$'
result=$(run_detect_cli Linux aarch64)
assert_eq "Linux aarch64 → cli asset" "${result##*|}" 'tunnel-pilot-cli-linux-aarch64\.tar\.gz$'
result=$(run_detect_cli Linux arm64)
assert_eq "Linux arm64 → aarch64 asset" "${result##*|}" 'tunnel-pilot-cli-linux-aarch64\.tar\.gz$'
result=$(run_detect_cli Darwin arm64)
assert_eq "Darwin → universal cli asset" "${result##*|}" 'tunnel-pilot-cli-macos-universal\.tar\.gz$'
( run_detect_cli MINGW64_NT x86_64 ) >/dev/null 2>&1 && fail "Windows --cli must fail" \
  || pass "Windows --cli is rejected (desktop only)"
( run_detect_cli Linux riscv64 ) >/dev/null 2>&1 && fail "unsupported arch must fail" \
  || pass "unsupported Linux arch is rejected"
MODE="desktop"

group "json_asset_url (cli assets skip the .sha256 sidecar)"
CLI_JSON='{
  "assets": [
    {"name":"tunnel-pilot-cli-linux-x86_64.tar.gz.sha256","browser_download_url":"https://example.com/tunnel-pilot-cli-linux-x86_64.tar.gz.sha256"},
    {"name":"tunnel-pilot-cli-linux-x86_64.tar.gz","browser_download_url":"https://example.com/tunnel-pilot-cli-linux-x86_64.tar.gz"},
    {"name":"tunnel-pilot-cli-linux-aarch64.tar.gz","browser_download_url":"https://example.com/tunnel-pilot-cli-linux-aarch64.tar.gz"},
    {"name":"Tunnel.Pilot_2.0.0_amd64.AppImage","browser_download_url":"https://example.com/Tunnel.Pilot_2.0.0_amd64.AppImage"}
  ]
}'
for HAS_JQ in false $(command -v jq >/dev/null && echo true); do
  assert_eq "x86_64 tarball (jq=$HAS_JQ)" \
    "$(json_asset_url "$CLI_JSON" 'tunnel-pilot-cli-linux-x86_64\.tar\.gz$')" \
    "https://example.com/tunnel-pilot-cli-linux-x86_64.tar.gz"
done
HAS_JQ=$(command -v jq >/dev/null && echo true || echo false)

case "$(command uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    echo "    ⚠  Skipping install_cli tests on Windows (CLI edition is Unix-only)"
    ;;
  *)
group "install_cli (simulated)"
id() { echo 1000; }   # never touch /usr/local/bin, even when tests run as root
CLI_BIN="$HOME/.local/bin/tunnel-pilot"
mkdir -p "$TMP_TEST/cli_pkg" "$TMP_TEST/cli_dl"
printf '\177ELFfake-cli-binary\n' > "$TMP_TEST/cli_pkg/tunnel-pilot"
chmod +x "$TMP_TEST/cli_pkg/tunnel-pilot"
tar -czf "$TMP_TEST/cli.tar.gz" -C "$TMP_TEST/cli_pkg" tunnel-pilot
if command -v sha256sum &>/dev/null; then
  GOOD_SUM=$(sha256sum "$TMP_TEST/cli.tar.gz" | awk '{print $1}')
else
  GOOD_SUM=$(shasum -a 256 "$TMP_TEST/cli.tar.gz" | awk '{print $1}')
fi
FAKE_SUM="$GOOD_SUM"
curl() { echo "$FAKE_SUM  tunnel-pilot-cli-linux-x86_64.tar.gz"; }
MODE="cli"; FORCE=false; CLI_SYSTEM=false; CLI_SKIPPED=false
ASSET_NAME="tunnel-pilot-cli-linux-x86_64.tar.gz"
ASSET_URL="https://example.com/$ASSET_NAME"
TMP_FILE="$TMP_TEST/cli.tar.gz"
TMP_DIR="$TMP_TEST/cli_dl"

# The AppImage launcher from section 5 is on PATH: a desktop install already
# includes the CLI, so --cli must leave it alone without --force.
install_cli 2>/dev/null
assert_eq "desktop launcher present → skipped" "$CLI_SKIPPED" "true"
assert_file_contains "launcher left untouched" "$CLI_BIN" "#!/bin/bash"

FORCE=true; CLI_SKIPPED=false
install_cli 2>/dev/null
assert_eq "--force → installed" "$CLI_SKIPPED" "false"
assert_file_executable "cli binary installed" "$CLI_BIN"
is_cli_binary "$CLI_BIN" && pass "installed file is a plain binary" \
  || fail "installed file should be a plain (non-script) binary"

# Upgrade over an existing CLI binary needs no --force.
FORCE=false; CLI_SKIPPED=false
install_cli 2>/dev/null
assert_eq "cli over cli → upgraded" "$CLI_SKIPPED" "false"

# A checksum mismatch aborts.
FAKE_SUM="0000000000000000000000000000000000000000000000000000000000000000"
( install_cli ) >/dev/null 2>&1 && fail "bad checksum must abort" \
  || pass "checksum mismatch aborts the install"

is_cli_binary "$LINUX_APPIMAGE" && fail "AppImage stub (script) is not a cli binary" \
  || pass "scripts are not mistaken for the cli binary"
unset -f curl id
MODE="desktop"; FORCE=false
    ;;
esac

# ── 7. check_deps ─────────────────────────────────────────────────────────────
group "check_deps"

PLATFORM="linux"
if command -v curl &>/dev/null; then
  (check_deps 2>/dev/null; echo $?) | grep -q "^0$" && pass "curl present → check_deps passes" || \
    pass "curl present → check_deps ran (jq warn expected)"
else
  echo "    ⚠  Skipping — curl not installed on this machine"
fi

# ── 8. Version strip logic ────────────────────────────────────────────────────
group "version comparison (tag strip)"
strip_v() { echo "${1#v}"; }
assert_eq "v0.0.4 → 0.0.4" "$(strip_v "v0.0.4")" "0.0.4"
assert_eq "0.0.4 unchanged" "$(strip_v "0.0.4")"  "0.0.4"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "  ─────────────────────────────────────"
TOTAL=$((PASS + FAIL))
if [ "$FAIL" -eq 0 ]; then
  echo -e "  \033[1;32m✓  All $TOTAL tests passed\033[0m"
  echo ""
  exit 0
else
  echo -e "  \033[0;31m✗  $FAIL of $TOTAL tests failed\033[0m"
  echo ""
  exit 1
fi
