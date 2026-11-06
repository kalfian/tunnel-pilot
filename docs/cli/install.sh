#!/bin/bash
set -euo pipefail

# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot CLI (server edition) — Installer
#  https://github.com/kalfian/tunnel-pilot
#
#  Usage:   curl -fsSL https://kalfian.github.io/tunnel-pilot/cli/install.sh | bash
#           ... | bash -s -- --system   install to /usr/local/bin (default when root)
#           ... | bash -s -- --yes      reinstall without asking
#  Remove:  curl -fsSL https://kalfian.github.io/tunnel-pilot/cli/uninstall.sh | bash
#
#  Installs `tunnel-pilot-cli`: one static binary with the same features as the
#  desktop app, run as a daemon and managed from the terminal or a web UI. No
#  window, no WebKit/GTK. Linux x86_64 / aarch64 and macOS. The archive is
#  verified against the published SHA-256 before anything is installed.
#  It has its own config (separate from the desktop app), so both can coexist.
# ══════════════════════════════════════════════════════════════════════════════

REPO="kalfian/tunnel-pilot"
BINARY_NAME="tunnel-pilot-cli"

SYSTEM_INSTALL=false
ASSUME_YES=false

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'
DIM='\033[2m'; BOLD='\033[1m'; NC='\033[0m'
print_step()    { echo -e "  ${CYAN}→${NC}  $1"; }
print_success() { echo -e "  ${GREEN}✓${NC}  $1"; }
print_warn()    { echo -e "  ${YELLOW}⚠${NC}  $1"; }
print_info()    { echo -e "  ${DIM}   $1${NC}"; }
print_error()   { echo -e "\n  ${RED}${BOLD}✗  Error${NC}\n  ${DIM}$1${NC}\n" >&2; exit 1; }

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --system)  SYSTEM_INSTALL=true ;;
      --yes|-y)  ASSUME_YES=true ;;
      --help|-h) sed -n '4,17p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null | sed 's/^#  \{0,1\}//'; exit 0 ;;
      *) print_error "Unknown option: $1 (try --help)" ;;
    esac
    shift
  done
}

# ── Platform → release asset ──────────────────────────────────────────────────
# CI publishes tunnel-pilot-cli-{linux-x86_64,linux-aarch64,macos-universal}.tar.gz,
# each with a `.sha256` sidecar. Patterns are '$'-anchored so the sidecar never
# matches.
detect_platform() {
  case "$(uname -s)" in
    Linux)
      PLATFORM="linux"
      case "$(uname -m)" in
        x86_64|amd64)  ASSET_PATTERN='tunnel-pilot-cli-linux-x86_64\.tar\.gz$' ;;
        aarch64|arm64) ASSET_PATTERN='tunnel-pilot-cli-linux-aarch64\.tar\.gz$' ;;
        *) print_error "No tunnel-pilot-cli build for Linux/$(uname -m) yet (x86_64 and aarch64 only)." ;;
      esac ;;
    Darwin)
      PLATFORM="macos"
      ASSET_PATTERN='tunnel-pilot-cli-macos-universal\.tar\.gz$' ;;
    MINGW*|MSYS*|CYGWIN*)
      print_error "tunnel-pilot-cli is not available on Windows. Use the desktop app:\n  https://kalfian.github.io/tunnel-pilot/" ;;
    *) print_error "Unsupported platform: $(uname -s)" ;;
  esac
}

bin_dir() {
  if $SYSTEM_INSTALL || [ "$(id -u 2>/dev/null || echo 1)" = "0" ]; then
    echo "/usr/local/bin"
  else
    echo "$HOME/.local/bin"
  fi
}

check_deps() {
  local tool
  for tool in curl tar; do
    command -v "$tool" >/dev/null || print_error "'$tool' is required but not installed."
  done
  command -v sha256sum >/dev/null || command -v shasum >/dev/null \
    || print_error "'sha256sum' (or 'shasum') is required to verify the download."
}

json_asset_url() {
  local json="$1" pattern="$2"
  if command -v jq >/dev/null; then
    echo "$json" | jq -r --arg pat "$pattern" \
      '.assets[] | select(.name | test($pat)) | .browser_download_url' | head -1
  else
    echo "$json" | grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*"' \
      | sed 's/.*: *"\(.*\)"/\1/' | grep -E "$pattern" | head -1
  fi
}

json_tag() {
  echo "$1" | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 \
    | sed 's/.*: *"\(.*\)"/\1/'
}

# GET a GitHub API URL into $API_BODY (status in $API_STATUS). Authenticated when
# GITHUB_TOKEN is set — 5000 requests/hour instead of 60 per IP, which shared CI
# runners exhaust — and retried with a short backoff. Written for bash 3.2 (macOS).
github_api_get() {
  local url="$1" attempt raw
  local auth=()
  [ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
  API_BODY=""; API_STATUS="000"
  for attempt in 1 2 3; do
    raw=$(curl -sSL -w '\n%{http_code}' -H "Accept: application/vnd.github+json" \
      ${auth[@]+"${auth[@]}"} "$url" 2>/dev/null) || raw=$'\n000'
    API_STATUS="${raw##*$'\n'}"
    API_BODY="${raw%$'\n'*}"
    [ "$API_STATUS" = "200" ] && return 0
    [ "$API_STATUS" = "404" ] && return 1
    [ "$attempt" -lt 3 ] && sleep $((attempt * 2))
  done
  return 1
}

# A human reason for a failed github_api_get.
github_api_error() {
  case "$API_STATUS" in
    000)     echo "Could not reach GitHub. Check your internet connection." ;;
    403|429) echo "GitHub's API rate limit was reached for your network (HTTP $API_STATUS). Try again in a while, or set GITHUB_TOKEN." ;;
    404)     echo "No published release was found." ;;
    *)       echo "GitHub answered HTTP $API_STATUS." ;;
  esac
}

fetch_latest() {
  print_step "Looking up the latest release"
  github_api_get "https://api.github.com/repos/${REPO}/releases/latest" \
    || print_error "$(github_api_error)"
  RELEASE_JSON="$API_BODY"
  VERSION=$(json_tag "$RELEASE_JSON")
  ASSET_URL=$(json_asset_url "$RELEASE_JSON" "$ASSET_PATTERN")
  if [ -z "$ASSET_URL" ] || [ "$ASSET_URL" = "null" ]; then
    print_error "Release ${VERSION:-?} has no tunnel-pilot-cli build for this platform yet.\n  See https://github.com/${REPO}/releases"
  fi
  ASSET_NAME=$(basename "$ASSET_URL")
  print_success "Latest release: ${BOLD}${VERSION}${NC}"
}

# A plain (non-script, non-symlink) executable — i.e. an earlier tunnel-pilot-cli.
installed_version() {
  local bin="$1"
  [ -f "$bin" ] && [ ! -L "$bin" ] && [ -x "$bin" ] || return 0
  [ "$(head -c 2 "$bin" 2>/dev/null)" = "#!" ] && return 0
  "$bin" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true
}

confirm_reinstall() {
  local dest="$1" current
  current=$(installed_version "$dest")
  [ -n "$current" ] || return 0
  if [ "$current" != "${VERSION#v}" ]; then
    print_success "Updating ${DIM}v$current${NC} → ${BOLD}${VERSION}${NC}"
    return 0
  fi
  local answer="n"
  if $ASSUME_YES; then
    answer="y"
  elif [ -r /dev/tty ]; then
    read -r -p "  Already on ${VERSION}. Reinstall? [y/N] " answer < /dev/tty || answer="n"
  fi
  [[ "$answer" =~ ^[Yy]$ ]] || { echo -e "  ${DIM}Nothing to do.${NC}"; exit 0; }
}

verify_checksum() {
  local file="$1" expected actual
  expected=$(curl -fsSL "${ASSET_URL}.sha256" 2>/dev/null | awk '{print $1}' | head -1 || true)
  [ -n "$expected" ] || print_error "No checksum published for $ASSET_NAME — refusing to install an unverified binary."
  if command -v sha256sum >/dev/null; then
    actual=$(sha256sum "$file" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$file" | awk '{print $1}')
  fi
  [ "$expected" = "$actual" ] \
    || print_error "Checksum mismatch for $ASSET_NAME.\n  expected $expected\n  got      $actual"
  print_success "Checksum verified (SHA-256)"
}

install_binary() {
  local dir dest sudo_cmd=""
  dir=$(bin_dir)
  dest="$dir/$BINARY_NAME"
  confirm_reinstall "$dest"

  TMP_DIR=$(mktemp -d)
  local archive="$TMP_DIR/$ASSET_NAME"
  print_step "Downloading $ASSET_NAME"
  curl -fL --progress-bar "$ASSET_URL" -o "$archive" || print_error "Download failed."
  verify_checksum "$archive"
  tar -xzf "$archive" -C "$TMP_DIR" || print_error "Could not unpack $ASSET_NAME."
  [ -f "$TMP_DIR/$BINARY_NAME" ] || print_error "$ASSET_NAME does not contain $BINARY_NAME."

  if ! mkdir -p "$dir" 2>/dev/null || [ ! -w "$dir" ]; then
    command -v sudo >/dev/null || print_error "$dir is not writable and sudo is unavailable."
    print_step "$dir needs administrator rights — using sudo"
    sudo_cmd="sudo"
    $sudo_cmd mkdir -p "$dir"
  fi
  # Never write THROUGH a symlink someone placed at the destination.
  $sudo_cmd rm -f -- "$dest"
  $sudo_cmd install -m 0755 "$TMP_DIR/$BINARY_NAME" "$dest" || print_error "Could not install to $dest."
  print_success "Installed ${DIM}$dest${NC}"
  INSTALLED_AT="$dest"

  if ! echo ":$PATH:" | grep -q ":$dir:"; then
    print_warn "$dir is not on your PATH — add: ${CYAN}export PATH=\"$dir:\$PATH\"${NC}"
  fi
}

cleanup() { rm -rf "${TMP_DIR:-}" 2>/dev/null || true; }

print_next_steps() {
  echo ""
  echo -e "  ${GREEN}${BOLD}✓  tunnel-pilot-cli ${VERSION} installed${NC}"
  echo ""
  print_info "1. Set the web UI admin password:"
  echo -e "       ${CYAN}tunnel-pilot-cli web set-password${NC}"
  print_info "2. Run it in the background with the web UI (port 1855):"
  echo -e "       ${CYAN}tunnel-pilot-cli service install --web${NC}            ${DIM}# this machine only${NC}"
  echo -e "       ${CYAN}tunnel-pilot-cli service install --web --bind 0.0.0.0${NC} ${DIM}# reachable via IP${NC}"
  print_info "   Over a network, put HTTPS in front (Caddy / nginx) — see the docs."
  print_info "3. Or manage forwards from the terminal: ${CYAN}tunnel-pilot-cli help${NC}"
  echo ""
  print_info "Docs → https://kalfian.github.io/tunnel-pilot/cli/"
  echo ""
}

main() {
  parse_args "$@"
  echo ""
  echo -e "  ${BOLD}Tunnel Pilot CLI — installer${NC}"
  echo ""
  detect_platform
  check_deps
  fetch_latest
  install_binary
  print_next_steps
}

if [[ "${BASH_SOURCE[0]:-$0}" == "${0}" ]]; then
  trap cleanup EXIT
  main "$@"
fi
