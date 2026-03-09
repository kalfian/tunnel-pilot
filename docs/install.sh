#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot — Installer
#  https://github.com/kalfian/tunnel-pilot
# ══════════════════════════════════════════════════════════════════════════════

REPO="kalfian/tunnel-pilot"
APP_NAME="Tunnel Pilot"
INSTALL_DIR="/Applications"
BINARY_NAME="tunnel_pilot"

# ── Colors & styles ───────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
WHITE='\033[1;37m'
DIM='\033[2m'
BOLD='\033[1m'
NC='\033[0m'

# ── Spinner ───────────────────────────────────────────────────────────────────
SPINNER_PID=""
SPINNER_FRAMES=("⠋" "⠙" "⠹" "⠸" "⠼" "⠴" "⠦" "⠧" "⠇" "⠏")

spinner_start() {
  local msg="$1"
  printf "  ${CYAN}%s${NC}  %s" "${SPINNER_FRAMES[0]}" "$msg"
  (
    local i=0
    while true; do
      i=$(( (i + 1) % ${#SPINNER_FRAMES[@]} ))
      printf "\r  ${CYAN}%s${NC}  %s" "${SPINNER_FRAMES[$i]}" "$msg"
      sleep 0.08
    done
  ) &
  SPINNER_PID=$!
  disown "$SPINNER_PID" 2>/dev/null || true
}

spinner_stop() {
  if [ -n "$SPINNER_PID" ]; then
    kill "$SPINNER_PID" 2>/dev/null || true
    wait "$SPINNER_PID" 2>/dev/null || true
    SPINNER_PID=""
    printf "\r\033[2K"
  fi
}

# ── Output helpers ────────────────────────────────────────────────────────────
print_header() {
  echo ""
  echo -e "  ${BOLD}${WHITE}┌─────────────────────────────────────┐${NC}"
  echo -e "  ${BOLD}${WHITE}│${NC}  ${CYAN}${BOLD}  Tunnel Pilot${NC}                     ${BOLD}${WHITE}│${NC}"
  echo -e "  ${BOLD}${WHITE}│${NC}  ${DIM}SSH Local Port Forwarding Manager${NC}  ${BOLD}${WHITE}│${NC}"
  echo -e "  ${BOLD}${WHITE}└─────────────────────────────────────┘${NC}"
  echo ""
}

print_step() {
  echo -e "  ${BLUE}→${NC}  $1"
}

print_success() {
  echo -e "  ${GREEN}✓${NC}  $1"
}

print_warn() {
  echo -e "  ${YELLOW}⚠${NC}  $1"
}

print_info() {
  echo -e "  ${DIM}   $1${NC}"
}

print_error() {
  spinner_stop
  echo ""
  echo -e "  ${RED}${BOLD}✗  Error${NC}"
  echo -e "  ${DIM}$1${NC}"
  echo ""
  exit 1
}

print_divider() {
  echo -e "  ${DIM}─────────────────────────────────────${NC}"
}

# ── Platform & architecture ───────────────────────────────────────────────────
detect_platform() {
  case "$(uname -s)" in
    Darwin)
      PLATFORM="macos"
      ARCH="$(uname -m)"
      # Prefer arch-specific asset, fall back to universal
      if [ "$ARCH" = "arm64" ]; then
        ASSET_PATTERN="arm64.*\\.dmg$|aarch64.*\\.dmg$|\\.dmg$"
      else
        ASSET_PATTERN="x86_64.*\\.dmg$|\\.dmg$"
      fi
      # Use a simple .dmg pattern (pick first match)
      ASSET_PATTERN="\\.dmg$"
      ;;
    Linux)
      PLATFORM="linux"
      ARCH="$(uname -m)"
      ASSET_PATTERN="linux.*\\.tar\\.gz$"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORM="windows"
      ARCH="$(uname -m)"
      ASSET_PATTERN="\\.zip$"
      ;;
    *)
      print_error "Unsupported platform: $(uname -s)\nThis installer supports macOS, Linux, and Windows (Git Bash)."
      ;;
  esac
}

# ── Dependency check (jq optional — fallback to grep/sed) ────────────────────
HAS_JQ=false
check_deps() {
  if ! command -v curl &>/dev/null; then
    print_error "'curl' is required but not installed.\n  macOS: brew install curl\n  Linux: sudo apt install curl"
  fi
  if [ "$PLATFORM" = "windows" ]; then
    if ! command -v unzip &>/dev/null && ! command -v powershell &>/dev/null && ! command -v pwsh &>/dev/null; then
      print_error "'unzip' or PowerShell is required on Windows.\n  Install Git for Windows or ensure PowerShell is in PATH."
    fi
  fi
  if command -v jq &>/dev/null; then
    HAS_JQ=true
  else
    print_warn "jq not found — using built-in parser (install jq for best results)"
  fi
}

# ── JSON helpers (jq or grep/sed fallback) ────────────────────────────────────
json_get() {
  local json="$1" key="$2"
  if $HAS_JQ; then
    echo "$json" | jq -r ".$key // empty"
  else
    # Simple grep-based extraction for flat string values
    echo "$json" | grep -o "\"${key}\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" \
      | head -1 | sed 's/.*: *"\(.*\)"/\1/'
  fi
}

json_asset_url() {
  local json="$1" pattern="$2"
  if $HAS_JQ; then
    echo "$json" | jq -r --arg pat "$pattern" \
      '.assets[] | select(.name | test($pat)) | .browser_download_url' | head -1
  else
    # Extract browser_download_url lines and grep for pattern
    echo "$json" | grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*"' \
      | sed 's/.*: *"\(.*\)"/\1/' \
      | grep -E "$pattern" | head -1
  fi
}

# ── Version comparison ─────────────────────────────────────────────────────────
get_installed_version() {
  INSTALLED_VERSION=""
  if [ "$PLATFORM" = "macos" ]; then
    local plist="$INSTALL_DIR/$APP_NAME.app/Contents/Info.plist"
    if [ -f "$plist" ]; then
      INSTALLED_VERSION=$(defaults read "$plist" CFBundleShortVersionString 2>/dev/null || true)
    fi
  elif [ "$PLATFORM" = "linux" ]; then
    if command -v "$HOME/.local/bin/$BINARY_NAME" &>/dev/null; then
      INSTALLED_VERSION=$("$HOME/.local/bin/$BINARY_NAME" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)
    fi
  fi
}

# ── Fetch latest release from GitHub ──────────────────────────────────────────
fetch_latest() {
  spinner_start "Fetching latest release"
  local api_url="https://api.github.com/repos/${REPO}/releases/latest"
  RELEASE_JSON=$(curl -fsSL \
    -H "Accept: application/vnd.github+json" \
    "$api_url" 2>/dev/null) || {
    spinner_stop
    print_error "Could not reach GitHub. Check your internet connection.\n  URL: $api_url"
  }
  spinner_stop

  VERSION=$(json_get "$RELEASE_JSON" "tag_name")
  if [ -z "$VERSION" ] || [ "$VERSION" = "null" ]; then
    print_error "No release found in repository.\n  Visit: https://github.com/${REPO}/releases"
  fi

  ASSET_URL=$(json_asset_url "$RELEASE_JSON" "$ASSET_PATTERN")
  if [ -z "$ASSET_URL" ] || [ "$ASSET_URL" = "null" ]; then
    print_error "No $PLATFORM release asset found for $VERSION.\n  Visit: https://github.com/${REPO}/releases"
  fi

  ASSET_NAME=$(basename "$ASSET_URL")

  get_installed_version
  if [ -n "$INSTALLED_VERSION" ]; then
    local clean_new="${VERSION#v}"
    if [ "$INSTALLED_VERSION" = "$clean_new" ]; then
      spinner_stop
      print_success "Already on latest version ${BOLD}$VERSION${NC}"
      echo ""
      read -r -p "  Reinstall anyway? [y/N] " answer
      echo ""
      [[ "$answer" =~ ^[Yy]$ ]] || { echo -e "  ${DIM}Nothing to do.${NC}"; echo ""; exit 0; }
    else
      print_success "Update available: ${DIM}v$INSTALLED_VERSION${NC} → ${BOLD}${GREEN}$VERSION${NC}"
    fi
  else
    print_success "Latest release: ${BOLD}$VERSION${NC}"
  fi
}

# ── Download ──────────────────────────────────────────────────────────────────
download_asset() {
  TMP_DIR=$(mktemp -d)
  TMP_FILE="$TMP_DIR/$ASSET_NAME"

  local size_bytes
  size_bytes=$(curl -fsSLI "$ASSET_URL" 2>/dev/null \
    | grep -i 'content-length' | tail -1 | awk '{print $2}' | tr -d '\r' || echo "")

  local size_label=""
  if [ -n "$size_bytes" ] && [ "$size_bytes" -gt 0 ] 2>/dev/null; then
    size_label=" $(awk "BEGIN{printf \"%.1f MB\", $size_bytes/1048576}")"
  fi

  print_step "Downloading${size_label:- $ASSET_NAME}..."
  curl -fL --progress-bar "$ASSET_URL" -o "$TMP_FILE" 2>&1 || \
    print_error "Download failed.\n  Check your internet connection or try again."
  print_success "Download complete"
}

# ── Quit app if running (macOS) ───────────────────────────────────────────────
quit_if_running_macos() {
  if pgrep -x "$APP_NAME" &>/dev/null || \
     osascript -e "tell application \"$APP_NAME\" to quit" &>/dev/null 2>&1; then
    print_step "Quitting running instance..."
    sleep 1
  fi
}

# ── Install: macOS ─────────────────────────────────────────────────────────────
install_macos() {
  quit_if_running_macos

  spinner_start "Mounting disk image"
  # Use -plist output for reliable mount point parsing (avoids -quiet suppressing output)
  PLIST_OUT=$(hdiutil attach "$TMP_FILE" -nobrowse -plist 2>/dev/null) || {
    spinner_stop
    print_error "Failed to mount DMG.\n  The download may be corrupt — try again."
  }

  MOUNT_POINT=$(echo "$PLIST_OUT" | grep -A1 'mount-point' | grep '<string>' | \
    sed 's|.*<string>||;s|</string>.*||' | tail -1)
  spinner_stop

  if [ -z "$MOUNT_POINT" ]; then
    print_error "Could not determine DMG mount point.\n  Try running the DMG manually."
  fi
  print_success "Mounted at ${DIM}$MOUNT_POINT${NC}"

  APP_SRC=$(find "$MOUNT_POINT" -name "*.app" -maxdepth 2 | head -1)
  if [ -z "$APP_SRC" ]; then
    hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
    print_error "No .app bundle found in DMG."
  fi

  DEST="$INSTALL_DIR/$APP_NAME.app"
  if [ -d "$DEST" ]; then
    print_step "Removing previous installation..."
    rm -rf "$DEST"
  fi

  spinner_start "Copying to /Applications"
  cp -R "$APP_SRC" "$INSTALL_DIR/"
  spinner_stop
  print_success "Copied to ${DIM}$DEST${NC}"

  hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

  spinner_start "Removing quarantine flag"
  xattr -rd com.apple.quarantine "$DEST" 2>/dev/null || true
  spinner_stop
  print_success "Quarantine removed"

  print_step "Launching $APP_NAME..."
  open "$DEST"
}

# ── Install: Linux ─────────────────────────────────────────────────────────────
install_linux() {
  INSTALL_APP_DIR="$HOME/.local/share/$BINARY_NAME"
  INSTALL_BIN="$HOME/.local/bin"

  # Remove previous installation before recreating
  if [ -d "$INSTALL_APP_DIR" ]; then
    print_step "Removing previous installation..."
    rm -rf "$INSTALL_APP_DIR"
  fi
  mkdir -p "$INSTALL_APP_DIR" "$INSTALL_BIN"

  spinner_start "Extracting archive"
  # Extract full bundle (binary + lib/*.so + data/) into the app dir
  # so the binary can find shared libs via $ORIGIN/lib at runtime
  tar -xzf "$TMP_FILE" -C "$INSTALL_APP_DIR"
  spinner_stop

  LINUX_BINARY=$(find "$INSTALL_APP_DIR" -name "$BINARY_NAME" -maxdepth 2 -type f | head -1)
  if [ -z "$LINUX_BINARY" ]; then
    print_error "Binary '$BINARY_NAME' not found in archive."
  fi

  chmod +x "$LINUX_BINARY"

  # Create a launcher wrapper so shared libs resolve via $ORIGIN/lib correctly.
  # A plain symlink would set $ORIGIN to ~/.local/bin and miss the lib/ directory.
  local LAUNCHER="$INSTALL_BIN/$BINARY_NAME"
  cat > "$LAUNCHER" << LAUNCHER_EOF
#!/bin/bash
exec "$LINUX_BINARY" "\$@"
LAUNCHER_EOF
  chmod +x "$LAUNCHER"

  print_success "Installed to ${DIM}$INSTALL_APP_DIR${NC}"
  print_success "Launcher at ${DIM}$LAUNCHER${NC}"

  # Check PATH
  if ! echo ":$PATH:" | grep -q ":$INSTALL_BIN:"; then
    print_warn "$INSTALL_BIN is not in your PATH"
    print_info "Add this to your ~/.bashrc or ~/.zshrc:"
    echo ""
    echo -e "    ${CYAN}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC}"
    echo ""
  fi
}

# ── Install: Windows ───────────────────────────────────────────────────────────
install_windows() {
  WIN_DEST="$APPDATA/Tunnel Pilot"
  mkdir -p "$WIN_DEST"

  spinner_start "Extracting archive"
  if command -v unzip &>/dev/null; then
    unzip -q "$TMP_FILE" -d "$WIN_DEST"
  elif command -v pwsh &>/dev/null; then
    pwsh -Command "Expand-Archive -Force -Path '$TMP_FILE' -DestinationPath '$WIN_DEST'"
  else
    powershell -Command "Expand-Archive -Force -Path '$TMP_FILE' -DestinationPath '$WIN_DEST'"
  fi
  spinner_stop
  print_success "Extracted to ${DIM}$WIN_DEST${NC}"
}

# ── Cleanup ───────────────────────────────────────────────────────────────────
cleanup() {
  spinner_stop
  rm -rf "${TMP_DIR:-}" 2>/dev/null || true
}

# ── Summary ───────────────────────────────────────────────────────────────────
print_summary() {
  echo ""
  print_divider
  echo ""
  echo -e "  ${GREEN}${BOLD}✓  Tunnel Pilot $VERSION installed!${NC}"
  echo ""
  case "$PLATFORM" in
    macos)
      print_info "App is launching from /Applications/$APP_NAME.app"
      print_info "Find it in the menu bar — look for the  icon"
      ;;
    linux)
      print_info "Run it with: ${CYAN}$BINARY_NAME${NC}"
      print_info "App bundle: ${DIM}$HOME/.local/share/$BINARY_NAME${NC}"
      ;;
    windows)
      print_info "Run tunnel_pilot.exe from:"
      print_info "${WIN_DEST:-$APPDATA/Tunnel Pilot}"
      ;;
  esac
  echo ""
  print_info "Docs & support → https://github.com/$REPO"
  echo ""
}

# ── Main ──────────────────────────────────────────────────────────────────────
main() {
  print_header
  detect_platform
  check_deps

  print_divider
  echo ""

  fetch_latest
  echo ""
  download_asset
  echo ""

  print_divider
  echo ""

  case "$PLATFORM" in
    macos)   install_macos   ;;
    linux)   install_linux   ;;
    windows) install_windows ;;
  esac

  cleanup
  print_summary
}

# Allow sourcing for tests without running main
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  trap cleanup EXIT
  main
fi
