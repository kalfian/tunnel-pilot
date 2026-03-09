#!/bin/bash
set -e

REPO="kalfian/tunnel-pilot"
APP_NAME="Tunnel Pilot"
INSTALL_DIR="/Applications"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

print_header() {
  echo ""
  echo -e "${BOLD}  Tunnel Pilot Installer${NC}"
  echo -e "  SSH Local Port Forwarding Manager"
  echo ""
}

print_step() {
  echo -e "${BLUE}=>${NC} $1"
}

print_success() {
  echo -e "${GREEN}✓${NC} $1"
}

print_error() {
  echo -e "${RED}✗ Error:${NC} $1" >&2
  exit 1
}

print_warn() {
  echo -e "${YELLOW}!${NC} $1"
}

# ── Platform check ────────────────────────────────────────────────────────────
detect_platform() {
  case "$(uname -s)" in
    Darwin)
      PLATFORM="macos"
      ASSET_PATTERN="\\.dmg$"
      ;;
    Linux)
      PLATFORM="linux"
      ASSET_PATTERN="linux.*\\.tar\\.gz$"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      PLATFORM="windows"
      ASSET_PATTERN="\\.zip$"
      ;;
    *)
      print_error "Unsupported platform: $(uname -s)"
      ;;
  esac
}

# ── Dependency checks ─────────────────────────────────────────────────────────
check_deps() {
  for cmd in curl jq; do
    if ! command -v "$cmd" &>/dev/null; then
      print_error "'$cmd' is required but not installed. Install it and try again."
    fi
  done
}

# ── Fetch latest release ──────────────────────────────────────────────────────
fetch_latest() {
  print_step "Fetching latest release..."
  RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest") || \
    print_error "Failed to fetch release info. Check your internet connection."

  VERSION=$(echo "$RELEASE_JSON" | jq -r '.tag_name')
  ASSET_URL=$(echo "$RELEASE_JSON" | jq -r --arg pat "$ASSET_PATTERN" \
    '.assets[] | select(.name | test($pat)) | .browser_download_url' | head -1)

  if [ -z "$ASSET_URL" ] || [ "$ASSET_URL" = "null" ]; then
    print_error "No release asset found for your platform ($PLATFORM). Visit https://github.com/${REPO}/releases"
  fi

  ASSET_NAME=$(basename "$ASSET_URL")
  print_success "Found $VERSION"
}

# ── Download ──────────────────────────────────────────────────────────────────
download_asset() {
  TMP_DIR=$(mktemp -d)
  TMP_FILE="$TMP_DIR/$ASSET_NAME"

  print_step "Downloading $ASSET_NAME..."
  curl -fL --progress-bar "$ASSET_URL" -o "$TMP_FILE" || \
    print_error "Download failed."
  print_success "Downloaded"
}

# ── Install: macOS ─────────────────────────────────────────────────────────────
install_macos() {
  print_step "Mounting disk image..."
  MOUNT_POINT=$(hdiutil attach "$TMP_FILE" -nobrowse -quiet | grep "/Volumes/" | awk '{print $NF}')

  if [ -z "$MOUNT_POINT" ]; then
    print_error "Failed to mount DMG."
  fi

  APP_SRC=$(find "$MOUNT_POINT" -name "*.app" -maxdepth 1 | head -1)
  if [ -z "$APP_SRC" ]; then
    hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true
    print_error "No .app found in DMG."
  fi

  DEST="$INSTALL_DIR/$APP_NAME.app"
  if [ -d "$DEST" ]; then
    print_warn "Existing installation found. Replacing..."
    rm -rf "$DEST"
  fi

  print_step "Installing to $INSTALL_DIR..."
  cp -R "$APP_SRC" "$INSTALL_DIR/"

  hdiutil detach "$MOUNT_POINT" -quiet 2>/dev/null || true

  # Remove quarantine flag
  xattr -rd com.apple.quarantine "$DEST" 2>/dev/null || true

  print_success "Installed to $DEST"

  print_step "Launching Tunnel Pilot..."
  open "$DEST"
}

# ── Install: Linux ─────────────────────────────────────────────────────────────
install_linux() {
  INSTALL_BIN="$HOME/.local/bin"
  mkdir -p "$INSTALL_BIN"

  print_step "Extracting..."
  tar -xzf "$TMP_FILE" -C "$TMP_DIR"

  BINARY=$(find "$TMP_DIR" -name "tunnel_pilot" -type f | head -1)
  if [ -z "$BINARY" ]; then
    print_error "Binary not found in archive."
  fi

  chmod +x "$BINARY"
  cp "$BINARY" "$INSTALL_BIN/tunnel_pilot"

  print_success "Installed to $INSTALL_BIN/tunnel_pilot"
  print_warn "Make sure $INSTALL_BIN is in your PATH."
  echo ""
  echo "  Run with: tunnel_pilot"
}

# ── Install: Windows ──────────────────────────────────────────────────────────
install_windows() {
  DEST="$APPDATA/Tunnel Pilot"
  mkdir -p "$DEST"

  print_step "Extracting..."
  unzip -q "$TMP_FILE" -d "$DEST"

  print_success "Extracted to $DEST"
  echo ""
  echo "  Run tunnel_pilot.exe from $DEST"
}

# ── Cleanup ───────────────────────────────────────────────────────────────────
cleanup() {
  rm -rf "$TMP_DIR" 2>/dev/null || true
}

# ── Main ──────────────────────────────────────────────────────────────────────
main() {
  print_header
  detect_platform
  check_deps
  fetch_latest
  download_asset

  case "$PLATFORM" in
    macos)   install_macos   ;;
    linux)   install_linux   ;;
    windows) install_windows ;;
  esac

  cleanup

  echo ""
  echo -e "${GREEN}${BOLD}  Tunnel Pilot $VERSION installed successfully!${NC}"
  echo ""
}

trap cleanup EXIT
main
