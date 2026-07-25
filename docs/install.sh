#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot — Installer (v2 / Tauri)
#  https://github.com/kalfian/tunnel-pilot
#
#  Fetches the latest v2 release from the GitHub Releases "latest" endpoint and
#  installs the platform-native bundle:
#    macOS   → .dmg          (mount, copy .app to /Applications)
#    Linux   → .AppImage     (chmod +x, launcher + .desktop) — falls back to .deb
#    Windows → NSIS setup.exe (run the installer)   [Git Bash / MSYS / Cygwin]
#
#  Bundles are UNSIGNED (open-source / unfunded — no paid OS-signing certs). The
#  self-update path IS cryptographically signed (minisign). Gatekeeper /
#  SmartScreen workarounds are printed at the end.
# ══════════════════════════════════════════════════════════════════════════════

REPO="kalfian/tunnel-pilot"
APP_NAME="Tunnel Pilot"
INSTALL_DIR="/Applications"
BINARY_NAME="tunnel-pilot"

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
#
# Asset filenames are matched by PATTERN (extension/arch), never a hardcoded
# version — the `releases/latest` endpoint resolves the newest tag on its own.
# GitHub rewrites spaces in asset names to '.', so "Tunnel Pilot_2.0.0_..." is
# uploaded as "Tunnel.Pilot_2.0.0_...". Anchoring each pattern with '$' also
# avoids matching updater sidecars (`*.sig`, `*.tar.gz`) and `latest.json`.
LINUX_FALLBACK_PATTERN=""   # secondary pattern tried if the primary finds nothing
detect_platform() {
  case "$(uname -s)" in
    Darwin)
      PLATFORM="macos"
      ARCH="$(uname -m)"
      # v2 ships a single universal .dmg; match any .dmg (excludes .app.tar.gz/.sig).
      ASSET_PATTERN="\\.dmg$"
      ;;
    Linux)
      PLATFORM="linux"
      ARCH="$(uname -m)"
      # Prefer the portable, no-root AppImage; fall back to .deb if absent.
      ASSET_PATTERN="\\.AppImage$"
      LINUX_FALLBACK_PATTERN="\\.deb$"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORM="windows"
      ARCH="$(uname -m)"
      # NSIS installer: "..._x64-setup.exe" (excludes "-setup.exe.sig").
      ASSET_PATTERN="-setup\\.exe$"
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
  # Linux: fall back to .deb if no AppImage was published.
  if { [ -z "$ASSET_URL" ] || [ "$ASSET_URL" = "null" ]; } && [ -n "$LINUX_FALLBACK_PATTERN" ]; then
    ASSET_URL=$(json_asset_url "$RELEASE_JSON" "$LINUX_FALLBACK_PATTERN")
  fi
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
  # pgrep -f matches full command path; -x alone won't match names with spaces
  if pgrep -f "$APP_NAME" &>/dev/null; then
    print_step "Quitting running instance..."
    osascript -e "tell application \"$APP_NAME\" to quit" &>/dev/null 2>&1 || true
    # Give the app a moment to exit cleanly before we replace its files
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
  case "$ASSET_NAME" in
    *.deb) install_linux_deb ;;
    *)     install_linux_appimage ;;
  esac
}

install_linux_appimage() {
  INSTALL_APP_DIR="$HOME/.local/share/$BINARY_NAME"
  INSTALL_BIN="$HOME/.local/bin"
  mkdir -p "$INSTALL_APP_DIR" "$INSTALL_BIN"

  local APPIMAGE_DEST="$INSTALL_APP_DIR/$ASSET_NAME"

  # Replace any previous AppImage(s) so we don't leave stale versions behind.
  find "$INSTALL_APP_DIR" -maxdepth 1 -name '*.AppImage' -delete 2>/dev/null || true

  spinner_start "Installing AppImage"
  cp "$TMP_FILE" "$APPIMAGE_DEST"
  chmod +x "$APPIMAGE_DEST"
  spinner_stop

  # Launcher wrapper on PATH → the AppImage. Kept indirect so the on-PATH name is
  # stable ("tunnel-pilot") even though the AppImage filename carries the version.
  local LAUNCHER="$INSTALL_BIN/$BINARY_NAME"
  cat > "$LAUNCHER" << LAUNCHER_EOF
#!/bin/bash
exec "$APPIMAGE_DEST" "\$@"
LAUNCHER_EOF
  chmod +x "$LAUNCHER"

  print_success "Installed to ${DIM}$APPIMAGE_DEST${NC}"
  print_success "Launcher at ${DIM}$LAUNCHER${NC}"

  # Best-effort .desktop entry (icon extracted from the AppImage if possible).
  local DESKTOP_DIR="$HOME/.local/share/applications"
  local ICON_DEST="$HOME/.local/share/icons/tunnel_pilot.png"
  mkdir -p "$DESKTOP_DIR" "$HOME/.local/share/icons"

  # `--appimage-extract` runs without FUSE; pull the embedded .DirIcon if present.
  local EXTRACT_TMP
  EXTRACT_TMP=$(mktemp -d)
  if ( cd "$EXTRACT_TMP" && "$APPIMAGE_DEST" --appimage-extract '.DirIcon' &>/dev/null ); then
    local EXTRACTED_ICON
    EXTRACTED_ICON=$(find "$EXTRACT_TMP/squashfs-root" -maxdepth 1 -name '.DirIcon' 2>/dev/null | head -1 || true)
    [ -n "$EXTRACTED_ICON" ] && cp "$EXTRACTED_ICON" "$ICON_DEST" 2>/dev/null || true
  fi
  rm -rf "$EXTRACT_TMP" 2>/dev/null || true

  cat > "$DESKTOP_DIR/tunnel_pilot.desktop" << DESKTOP_EOF
[Desktop Entry]
Name=Tunnel Pilot
Comment=SSH Local Port Forwarding Manager
Exec=$LAUNCHER %U
Icon=${ICON_DEST}
Terminal=false
Type=Application
Categories=Network;Utility;
StartupNotify=false
DESKTOP_EOF
  chmod +x "$DESKTOP_DIR/tunnel_pilot.desktop"

  # Also place shortcut on Desktop if the directory exists
  local USER_DESKTOP="$HOME/Desktop"
  if [ -d "$USER_DESKTOP" ]; then
    cp "$DESKTOP_DIR/tunnel_pilot.desktop" "$USER_DESKTOP/tunnel_pilot.desktop"
    chmod +x "$USER_DESKTOP/tunnel_pilot.desktop"
    print_success "Desktop shortcut created at ${DIM}$USER_DESKTOP/tunnel_pilot.desktop${NC}"
  fi

  # Refresh app menu (best-effort)
  command -v update-desktop-database &>/dev/null && \
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true

  # Check PATH
  if ! echo ":$PATH:" | grep -q ":$INSTALL_BIN:"; then
    print_warn "$INSTALL_BIN is not in your PATH"
    print_info "Add this to your ~/.bashrc or ~/.zshrc:"
    echo ""
    echo -e "    ${CYAN}export PATH=\"\$HOME/.local/bin:\$PATH\"${NC}"
    echo ""
  fi
}

install_linux_deb() {
  print_step "Installing .deb package (requires sudo)..."
  if command -v apt-get &>/dev/null; then
    sudo apt-get install -y "$TMP_FILE" \
      || print_error "apt-get failed to install the .deb.\n  Try manually: sudo apt-get install -y '$TMP_FILE'"
  elif command -v dpkg &>/dev/null; then
    sudo dpkg -i "$TMP_FILE" || sudo apt-get -f install -y \
      || print_error "dpkg failed to install the .deb.\n  Try manually: sudo dpkg -i '$TMP_FILE'"
  else
    print_error "No apt-get/dpkg found — install the .deb manually:\n  $TMP_FILE"
  fi
  print_success "Installed via package manager"
}

# ── Install: Windows (Git Bash / MSYS / Cygwin) ────────────────────────────────
install_windows() {
  # The v2 Windows artifact is an NSIS installer (…-setup.exe). Run it; NSIS is
  # configured for a per-user install (no admin prompt). We launch it silently
  # (/S) so the one-liner stays non-interactive.
  print_step "Running installer..."
  chmod +x "$TMP_FILE" 2>/dev/null || true
  if command -v cmd.exe &>/dev/null; then
    MSYS2_ARG_CONV_EXCL="*" cmd.exe /c "$(cygpath -w "$TMP_FILE" 2>/dev/null || echo "$TMP_FILE")" /S \
      || print_error "Installer exited with an error.\n  Run it manually: $ASSET_NAME"
  else
    "$TMP_FILE" /S \
      || print_error "Installer exited with an error.\n  Run it manually: $ASSET_NAME"
  fi
  print_success "Installer finished (per-user NSIS install)"
}

# ── Cleanup ───────────────────────────────────────────────────────────────────
cleanup() {
  spinner_stop
  rm -rf "${TMP_DIR:-}" 2>/dev/null || true
}

# ── Unsigned-install note (bundles are not OS-code-signed) ─────────────────────
print_unsigned_note() {
  case "$PLATFORM" in
    macos)
      echo ""
      print_warn "This build is not notarized by Apple."
      print_info "If macOS blocks the first launch: right-click the app in"
      print_info "/Applications → ${BOLD}Open${NC} → confirm ${BOLD}Open${NC}. (This installer"
      print_info "already strips the quarantine flag, so a plain launch usually works.)"
      ;;
    windows)
      echo ""
      print_warn "This build is not code-signed."
      print_info "If Windows SmartScreen warns on first run: click"
      print_info "${BOLD}More info${NC} → ${BOLD}Run anyway${NC}."
      ;;
    linux)
      : # No OS-signing gatekeeper on Linux.
      ;;
  esac
  print_info "Self-updates ARE cryptographically signed (minisign); only the"
  print_info "initial download is unsigned at the OS level."
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
      print_info "Find it in the menu bar — look for the tray icon"
      ;;
    linux)
      print_info "Run it with: ${CYAN}$BINARY_NAME${NC}"
      case "$ASSET_NAME" in
        *.deb) print_info "Installed via your package manager" ;;
        *)     print_info "AppImage: ${DIM}$HOME/.local/share/$BINARY_NAME${NC}" ;;
      esac
      ;;
    windows)
      print_info "Launch ${BOLD}Tunnel Pilot${NC} from the Start Menu"
      ;;
  esac
  print_unsigned_note
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
if [[ "${BASH_SOURCE[0]:-$0}" == "${0}" ]]; then
  trap cleanup EXIT
  main
fi
