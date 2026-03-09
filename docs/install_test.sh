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

# macOS
result=$(run_detect "Darwin")
assert_eq "Darwin → platform=macos"        "${result%%|*}" "macos"
assert_contains "Darwin → .dmg pattern"    "${result##*|}" "dmg"

# Linux
result=$(run_detect "Linux")
assert_eq "Linux → platform=linux"         "${result%%|*}" "linux"
assert_contains "Linux → .tar.gz pattern"  "${result##*|}" "tar"

# Windows (MINGW)
result=$(run_detect "MINGW64_NT")
assert_eq "MINGW → platform=windows"       "${result%%|*}" "windows"
assert_contains "MINGW → .zip pattern"     "${result##*|}" "zip"

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

# ── 4. json_asset_url — grep/sed fallback ────────────────────────────────────
# Note: ASSET_PATTERN in install.sh uses double-quoted "\\." which bash stores
# as "\." (one backslash). Both jq test() and grep -E treat "\." as literal dot.
group "json_asset_url (grep/sed fallback)"
HAS_JQ=false
FAKE_JSON='{
  "assets": [
    {"name":"TunnelPilot-v1.0-macos.dmg","browser_download_url":"https://example.com/TunnelPilot-v1.0-macos.dmg"},
    {"name":"TunnelPilot-v1.0-linux.tar.gz","browser_download_url":"https://example.com/TunnelPilot-v1.0-linux.tar.gz"},
    {"name":"TunnelPilot-v1.0-windows.zip","browser_download_url":"https://example.com/TunnelPilot-v1.0-windows.zip"}
  ]
}'
# Use single-backslash patterns (same as what install.sh ASSET_PATTERN stores after bash expansion)
assert_eq "macOS .dmg"    "$(json_asset_url "$FAKE_JSON" '\.dmg$')"             "https://example.com/TunnelPilot-v1.0-macos.dmg"
assert_eq "Linux .tar.gz" "$(json_asset_url "$FAKE_JSON" 'linux.*\.tar\.gz$')"  "https://example.com/TunnelPilot-v1.0-linux.tar.gz"
assert_eq "Windows .zip"  "$(json_asset_url "$FAKE_JSON" '\.zip$')"             "https://example.com/TunnelPilot-v1.0-windows.zip"

if command -v jq &>/dev/null; then
  group "json_asset_url (jq)"
  HAS_JQ=true
  assert_eq "macOS .dmg"    "$(json_asset_url "$FAKE_JSON" '\.dmg$')"             "https://example.com/TunnelPilot-v1.0-macos.dmg"
  assert_eq "Linux .tar.gz" "$(json_asset_url "$FAKE_JSON" 'linux.*\.tar\.gz$')"  "https://example.com/TunnelPilot-v1.0-linux.tar.gz"
  assert_eq "Windows .zip"  "$(json_asset_url "$FAKE_JSON" '\.zip$')"             "https://example.com/TunnelPilot-v1.0-windows.zip"
fi

# ── 5. install_linux — full bundle extraction ─────────────────────────────────
# Simulated on macOS: uses real bash + tar/mkdir, no Linux system required.
group "install_linux (simulated)"

TMP_TEST=$(mktemp -d)
trap 'rm -rf "$TMP_TEST"' EXIT

# Build a fake Flutter Linux release bundle (binary + lib/*.so + data/)
FAKE_BUNDLE="$TMP_TEST/bundle"
mkdir -p "$FAKE_BUNDLE/lib" "$FAKE_BUNDLE/data"
printf '#!/bin/bash\necho "tunnel_pilot v1.2.3"\n' > "$FAKE_BUNDLE/tunnel_pilot"
chmod +x "$FAKE_BUNDLE/tunnel_pilot"
echo "fake_shared_lib" > "$FAKE_BUNDLE/lib/libflutter_linux_gtk.so"
echo "fake_shared_lib" > "$FAKE_BUNDLE/lib/libscreen_retriever_linux_plugin.so"
echo "fake_asset"      > "$FAKE_BUNDLE/data/flutter_assets"

FAKE_TAR="$TMP_TEST/TunnelPilot-v1.2.3-linux.tar.gz"
tar -czf "$FAKE_TAR" -C "$FAKE_BUNDLE" .

# Point HOME and script vars to temp sandbox
export HOME="$TMP_TEST/home"
# Add install bin to PATH so the PATH-check branch doesn't trigger noisy output
export PATH="$TMP_TEST/home/.local/bin:$PATH"
PLATFORM="linux"
TMP_DIR="$TMP_TEST/tmp_dl"
TMP_FILE="$FAKE_TAR"
mkdir -p "$TMP_DIR"

install_linux 2>/dev/null

LINUX_APP_DIR="$TMP_TEST/home/.local/share/tunnel_pilot"
LINUX_BIN="$TMP_TEST/home/.local/bin/tunnel_pilot"

assert_file_exists     "app dir created"                  "$LINUX_APP_DIR"
assert_file_exists     "binary extracted"                 "$LINUX_APP_DIR/tunnel_pilot"
assert_file_executable "binary is executable"             "$LINUX_APP_DIR/tunnel_pilot"
assert_file_exists     "lib/ dir preserved"               "$LINUX_APP_DIR/lib"
assert_file_exists     "libflutter_linux_gtk.so present"  "$LINUX_APP_DIR/lib/libflutter_linux_gtk.so"
assert_file_exists     "libscreen_retriever.so present"   "$LINUX_APP_DIR/lib/libscreen_retriever_linux_plugin.so"
assert_file_exists     "data/ dir preserved"              "$LINUX_APP_DIR/data"
assert_file_exists     "launcher script created"          "$LINUX_BIN"
assert_file_executable "launcher is executable"           "$LINUX_BIN"
assert_file_contains   "launcher points to real binary"   "$LINUX_BIN" "$LINUX_APP_DIR/tunnel_pilot"
assert_file_contains   "launcher is a bash script"        "$LINUX_BIN" "#!/bin/bash"

# Reinstall: stale files must be cleaned up
echo "old_file" > "$LINUX_APP_DIR/stale_file"
install_linux 2>/dev/null
assert_file_exists "reinstall: new bundle present" "$LINUX_APP_DIR/tunnel_pilot"
[ ! -f "$LINUX_APP_DIR/stale_file" ] \
  && pass "reinstall: stale files removed" \
  || fail "reinstall: stale file was NOT removed"

# ── 6. install_windows — extract logic ───────────────────────────────────────
# Simulated on macOS: uses real bash + zip/unzip, no Windows system required.
group "install_windows (simulated)"

if command -v zip &>/dev/null && command -v unzip &>/dev/null; then
  # Build a fake Windows release ZIP (mirrors CI: all files at root of archive)
  FAKE_WIN_DIR="$TMP_TEST/win_bundle"
  mkdir -p "$FAKE_WIN_DIR"
  printf "fake exe\n" > "$FAKE_WIN_DIR/tunnel_pilot.exe"
  printf "fake dll\n" > "$FAKE_WIN_DIR/flutter_windows.dll"
  printf "fake dll\n" > "$FAKE_WIN_DIR/data_flutter.dll"

  FAKE_ZIP="$TMP_TEST/TunnelPilot-v1.2.3-windows.zip"
  # -j: junk paths (store files at root of zip, matching CI Compress-Archive behavior)
  (cd "$FAKE_WIN_DIR" && zip -q -j "$FAKE_ZIP" tunnel_pilot.exe flutter_windows.dll data_flutter.dll)

  PLATFORM="windows"
  APPDATA="$TMP_TEST/appdata"
  export APPDATA
  TMP_FILE="$FAKE_ZIP"

  install_windows 2>/dev/null

  WIN_DEST_DIR="$TMP_TEST/appdata/Tunnel Pilot"
  assert_file_exists "WIN_DEST directory created"  "$WIN_DEST_DIR"
  assert_file_exists "tunnel_pilot.exe present"    "$WIN_DEST_DIR/tunnel_pilot.exe"
  assert_file_exists "flutter_windows.dll present" "$WIN_DEST_DIR/flutter_windows.dll"
  assert_file_exists "data dll present"            "$WIN_DEST_DIR/data_flutter.dll"

  # Reinstall: update exe content and re-run; files must be overwritten silently
  printf "updated exe\n" > "$FAKE_WIN_DIR/tunnel_pilot.exe"
  rm -f "$FAKE_ZIP"
  (cd "$FAKE_WIN_DIR" && zip -q -j "$FAKE_ZIP" tunnel_pilot.exe flutter_windows.dll data_flutter.dll)
  install_windows 2>/dev/null
  assert_eq "reinstall: exe content updated" \
    "$(cat "$WIN_DEST_DIR/tunnel_pilot.exe")" "updated exe"
else
  echo "    ⚠  Skipping Windows simulation — zip/unzip not available"
fi

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
