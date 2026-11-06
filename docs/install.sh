#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot — Installer (v2 / Tauri)
#  https://github.com/kalfian/tunnel-pilot
#
#  Fetches the latest v2 release from the GitHub Releases "latest" endpoint and
#  installs one of two editions:
#
#  Desktop (default, or --desktop) — the tray app; its binary is also the CLI:
#    macOS   → .dmg          (mount, copy .app to /Applications)
#    Linux   → .AppImage     (chmod +x, launcher + .desktop) — falls back to .deb
#    Windows → NSIS setup.exe (run the installer)   [Git Bash / MSYS / Cygwin]
#
#  CLI only (--cli) — one small static binary for servers and headless boxes:
#    no window, no WebKit/GTK; runs tunnels via `tunnel-pilot daemon` /
#    `tunnel-pilot service install`. Linux x86_64/aarch64 + macOS universal.
#    Installs to ~/.local/bin (or /usr/local/bin with --system or as root).
#    Upgrade later with `tunnel-pilot desktop install` (same config).
#
#  Usage:
#    curl -fsSL https://kalfian.github.io/tunnel-pilot/install.sh | bash
#    curl -fsSL https://kalfian.github.io/tunnel-pilot/install.sh | bash -s -- --cli
#
#  Bundles are UNSIGNED (open-source / unfunded — no paid OS-signing certs). The
#  self-update path IS cryptographically signed (minisign). Gatekeeper /
#  SmartScreen workarounds are printed at the end.
# ══════════════════════════════════════════════════════════════════════════════

REPO="kalfian/tunnel-pilot"
APP_NAME="Tunnel Pilot"
INSTALL_DIR="/Applications"
BINARY_NAME="tunnel-pilot"

# Options (set by parse_args).
MODE="desktop"        # desktop | cli
CLI_SYSTEM=false      # --system: /usr/local/bin instead of ~/.local/bin
ASSUME_YES=false      # -y / --yes: reinstall without asking
FORCE=false           # --force: CLI install may replace a desktop launcher

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

# ── Arguments ─────────────────────────────────────────────────────────────────
print_usage() {
  cat << USAGE_EOF
Tunnel Pilot installer

  install.sh [--desktop | --cli] [--system] [--yes] [--force]

  --desktop   Install the desktop app (default). Its binary is also the CLI.
  --cli       Install only the command-line edition (servers / headless).
  --system    --cli only: install to /usr/local/bin (default ~/.local/bin;
              automatic when run as root).
  --yes, -y   Reinstall without asking when already on the latest version.
  --force     --cli only: replace an existing desktop launcher on PATH.
USAGE_EOF
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --cli)      MODE="cli" ;;
      --desktop)  MODE="desktop" ;;
      --system)   CLI_SYSTEM=true ;;
      --yes|-y)   ASSUME_YES=true ;;
      --force)    FORCE=true ;;
      --help|-h)  print_usage; exit 0 ;;
      *)          print_usage; print_error "Unknown option: $1" ;;
    esac
    shift
  done
  if [ "$MODE" != "cli" ] && { $CLI_SYSTEM || $FORCE; }; then
    print_error "--system and --force only apply together with --cli."
  fi
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
  if [ "$MODE" = "cli" ]; then
    cli_asset_pattern
  fi
}

# CLI-only assets are published by CI as
#   tunnel-pilot-cli-linux-x86_64.tar.gz / -linux-aarch64 / -macos-universal
# each with a `.sha256` sidecar (anchored with '$' so the sidecar never matches).
cli_asset_pattern() {
  LINUX_FALLBACK_PATTERN=""
  case "$PLATFORM" in
    linux)
      case "$ARCH" in
        x86_64|amd64)  CLI_ARCH="x86_64" ;;
        aarch64|arm64) CLI_ARCH="aarch64" ;;
        *) print_error "No CLI-only build for Linux/$ARCH yet.\n  Supported: x86_64, aarch64." ;;
      esac
      ASSET_PATTERN="tunnel-pilot-cli-linux-${CLI_ARCH}\\.tar\\.gz$"
      ;;
    macos)
      ASSET_PATTERN="tunnel-pilot-cli-macos-universal\\.tar\\.gz$"
      ;;
    windows)
      print_error "The CLI-only edition is not available on Windows.\n  Install the desktop app instead (its binary includes the CLI)."
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
  if [ "$MODE" = "cli" ]; then
    # Only ever execute a plain binary here: a desktop launcher (script or
    # symlink into the .app) from an older release would open the GUI.
    local bin
    bin="$(cli_bin_dir)/$BINARY_NAME"
    if is_cli_binary "$bin"; then
      INSTALLED_VERSION=$("$bin" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)
    fi
    return
  fi
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
      local answer="n"
      if $ASSUME_YES; then
        answer="y"
      elif [ -r /dev/tty ]; then
        # `curl | bash` makes stdin the script itself — ask the terminal.
        read -r -p "  Reinstall anyway? [y/N] " answer < /dev/tty || answer="n"
      fi
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

# ── Install: CLI only ─────────────────────────────────────────────────────────
cli_bin_dir() {
  if $CLI_SYSTEM || [ "$(id -u 2>/dev/null || echo 1)" = "0" ]; then
    echo "/usr/local/bin"
  else
    echo "$HOME/.local/bin"
  fi
}

# True for a regular, non-script executable — i.e. a CLI-only binary, not a
# desktop launcher script (Linux AppImage wrapper) or a symlink into the .app.
is_cli_binary() {
  local f="$1"
  [ -f "$f" ] && [ ! -L "$f" ] && [ -x "$f" ] || return 1
  [ "$(head -c 2 "$f" 2>/dev/null)" != "#!" ]
}

# Verify the download against the `.sha256` sidecar CI publishes next to it.
verify_checksum() {
  local sum_url="${ASSET_URL}.sha256" expected actual
  expected=$(curl -fsSL "$sum_url" 2>/dev/null | awk '{print $1}' | head -1 || true)
  if [ -z "$expected" ]; then
    print_warn "No checksum published for $ASSET_NAME — skipping verification"
    return
  fi
  if command -v sha256sum &>/dev/null; then
    actual=$(sha256sum "$TMP_FILE" | awk '{print $1}')
  elif command -v shasum &>/dev/null; then
    actual=$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')
  else
    print_warn "No sha256sum/shasum available — skipping checksum verification"
    return
  fi
  [ "$expected" = "$actual" ] || print_error "Checksum mismatch for $ASSET_NAME.\n  expected $expected\n  got      $actual"
  print_success "Checksum verified (sha256)"
}

install_cli() {
  local bin_dir dest
  bin_dir="$(cli_bin_dir)"
  dest="$bin_dir/$BINARY_NAME"

  # A desktop install already ships the CLI (same binary). Do not silently
  # replace its launcher with the CLI-only edition.
  if { [ -L "$dest" ] || [ -e "$dest" ]; } && ! is_cli_binary "$dest" && ! $FORCE; then
    print_success "The desktop edition is already on PATH at ${DIM}$dest${NC} — it includes the CLI."
    print_info "Nothing to do. Re-run with --force to replace it with the CLI-only edition."
    CLI_SKIPPED=true
    return
  fi

  verify_checksum

  spinner_start "Unpacking"
  tar -xzf "$TMP_FILE" -C "$TMP_DIR" || { spinner_stop; print_error "Could not unpack $ASSET_NAME."; }
  spinner_stop
  [ -f "$TMP_DIR/$BINARY_NAME" ] || print_error "$ASSET_NAME does not contain '$BINARY_NAME'."

  local sudo_cmd=""
  if ! mkdir -p "$bin_dir" 2>/dev/null || [ ! -w "$bin_dir" ]; then
    if command -v sudo &>/dev/null; then
      print_step "$bin_dir is not writable — using sudo"
      sudo_cmd="sudo"
      $sudo_cmd mkdir -p "$bin_dir"
    else
      print_error "$bin_dir is not writable and sudo is unavailable.\n  Re-run without --system to install into ~/.local/bin."
    fi
  fi
  # Replace atomically-ish: drop a symlink/launcher first so `install` never
  # writes THROUGH a symlink into the desktop .app (only reachable with --force).
  $sudo_cmd rm -f "$dest"
  $sudo_cmd install -m 0755 "$TMP_DIR/$BINARY_NAME" "$dest" \
    || print_error "Could not install to $dest."
  print_success "Installed to ${DIM}$dest${NC}"

  if ! echo ":$PATH:" | grep -q ":$bin_dir:"; then
    print_warn "$bin_dir is not in your PATH"
    print_info "Add this to your ~/.bashrc or ~/.zshrc:"
    echo ""
    echo -e "    ${CYAN}export PATH=\"$bin_dir:\$PATH\"${NC}"
    echo ""
  fi
}

# Desktop install on top of a CLI-only one: the desktop binary is a superset,
# so point the CLI-only binary's PATH entries at the app (macOS) and tell the
# user what happened. Linux's AppImage launcher already replaces ~/.local/bin.
adopt_cli_after_desktop() {
  [ "$PLATFORM" = "macos" ] || return 0
  local app_bin="$INSTALL_DIR/$APP_NAME.app/Contents/MacOS/$BINARY_NAME"
  [ -x "$app_bin" ] || return 0
  local f
  for f in "$HOME/.local/bin/$BINARY_NAME" "/usr/local/bin/$BINARY_NAME"; do
    if is_cli_binary "$f" && [ -w "$(dirname "$f")" ]; then
      ln -sf "$app_bin" "$f" && print_success "Pointed ${DIM}$f${NC} at the desktop app (it includes the CLI)"
    fi
  done
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
print_cli_summary() {
  echo ""
  print_divider
  echo ""
  if ${CLI_SKIPPED:-false}; then
    echo -e "  ${GREEN}${BOLD}✓  Tunnel Pilot is already installed (desktop edition)${NC}"
    echo ""
    return
  fi
  echo -e "  ${GREEN}${BOLD}✓  Tunnel Pilot CLI $VERSION installed!${NC}"
  echo ""
  print_info "Add a forward:"
  echo -e "    ${CYAN}$BINARY_NAME add prod-db --ssh deploy@bastion.example.com \\${NC}"
  echo -e "    ${CYAN}    --forward 5432:db.internal:5432 --identity ~/.ssh/id_ed25519${NC}"
  print_info "Keep tunnels up in the background (systemd / launchd):"
  echo -e "    ${CYAN}$BINARY_NAME service install --connect-all${NC}"
  print_info "Check them:  ${CYAN}$BINARY_NAME list${NC}"
  print_info "Want the tray app later?  ${CYAN}$BINARY_NAME desktop install${NC}"
  echo ""
  print_info "Docs & support → https://github.com/$REPO"
  echo ""
}

print_summary() {
  if [ "$MODE" = "cli" ]; then
    print_cli_summary
    return
  fi
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
  parse_args "$@"
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

  if [ "$MODE" = "cli" ]; then
    install_cli
  else
    case "$PLATFORM" in
      macos)   install_macos; adopt_cli_after_desktop ;;
      linux)   install_linux   ;;
      windows) install_windows ;;
    esac
  fi

  cleanup
  print_summary
}

# Allow sourcing for tests without running main
if [[ "${BASH_SOURCE[0]:-$0}" == "${0}" ]]; then
  trap cleanup EXIT
  main "$@"
fi
