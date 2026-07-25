#Requires -Version 5.1
# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot — Windows Installer (PowerShell, v2 / Tauri)
#  Usage: powershell -ExecutionPolicy Bypass -c "irm https://kalfian.github.io/tunnel-pilot/install.ps1 | iex"
#
#  Fetches the latest v2 release from the GitHub Releases "latest" endpoint and
#  runs the NSIS installer (…_x64-setup.exe). The installer is configured for a
#  per-user install (no admin prompt). Asset is matched by filename PATTERN, not
#  a hardcoded version. The bundle is UNSIGNED (SmartScreen note printed below);
#  self-updates ARE minisign-signed.
# ══════════════════════════════════════════════════════════════════════════════

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ErrorActionPreference = "Stop"

$REPO     = "kalfian/tunnel-pilot"
$APP_NAME = "Tunnel Pilot"

function Write-Header {
  Write-Host ""
  Write-Host "  +-------------------------------------+" -ForegroundColor DarkGray
  Write-Host "  |  " -NoNewline -ForegroundColor DarkGray
  Write-Host "  Tunnel Pilot" -NoNewline -ForegroundColor Cyan
  Write-Host "                     |" -ForegroundColor DarkGray
  Write-Host "  |  " -NoNewline -ForegroundColor DarkGray
  Write-Host "SSH Local Port Forwarding Manager" -NoNewline -ForegroundColor DarkGray
  Write-Host "  |" -ForegroundColor DarkGray
  Write-Host "  +-------------------------------------+" -ForegroundColor DarkGray
  Write-Host ""
}

function Write-Step  { param($msg) Write-Host "  -> $msg" -ForegroundColor Cyan }
function Write-Ok    { param($msg) Write-Host "  v  $msg" -ForegroundColor Green }
function Write-Warn  { param($msg) Write-Host "  !  $msg" -ForegroundColor Yellow }
function Write-Err   { param($msg) Write-Host "  x  Error: $msg" -ForegroundColor Red; exit 1 }

# ── Header ────────────────────────────────────────────────────────────────────
Write-Header

# ── Fetch latest release ──────────────────────────────────────────────────────
Write-Step "Fetching latest release..."
try {
  $release = Invoke-RestMethod `
    -Uri "https://api.github.com/repos/$REPO/releases/latest" `
    -Headers @{ Accept = "application/vnd.github+json" }
} catch {
  Write-Err "Could not reach GitHub. Check your internet connection."
}

$VERSION = $release.tag_name

# NSIS installer asset: "..._x64-setup.exe". The trailing "-setup.exe" excludes
# the updater sidecar "-setup.exe.sig" and any "latest.json".
$asset = $release.assets |
  Where-Object { $_.name -match '-setup\.exe$' } |
  Select-Object -First 1

if (-not $asset) {
  Write-Err "No Windows installer (-setup.exe) found for $VERSION.`n  Visit: https://github.com/$REPO/releases"
}

Write-Ok "Latest release: $VERSION"
Write-Host ""

# ── Download ──────────────────────────────────────────────────────────────────
$tmpFile = Join-Path $env:TEMP $asset.name
$sizeMB  = [math]::Round($asset.size / 1MB, 1)
Write-Step "Downloading $($asset.name) ($sizeMB MB)..."
try {
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmpFile
} catch {
  Write-Err "Download failed. Check your internet connection or try again."
}
Write-Ok "Download complete"
Write-Host ""

# ── Quit running instance (installer would otherwise fail to replace files) ─────
$running = Get-Process -Name "tunnel-pilot" -ErrorAction SilentlyContinue
if ($running) {
  Write-Step "Quitting running instance..."
  $running | Stop-Process -Force
  Start-Sleep -Seconds 1
}

# ── Run the NSIS installer (per-user, silent) ─────────────────────────────────
Write-Step "Running installer..."
try {
  $proc = Start-Process -FilePath $tmpFile -ArgumentList "/S" -PassThru -Wait
  if ($proc.ExitCode -ne 0) {
    Write-Err "Installer exited with code $($proc.ExitCode). Run it manually: $tmpFile"
  }
} catch {
  Write-Err "Could not run the installer: $_"
}
Remove-Item $tmpFile -ErrorAction SilentlyContinue
Write-Ok "Installed (per-user)"

# ── Summary ───────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  ─────────────────────────────────────" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  v  Tunnel Pilot $VERSION installed!" -ForegroundColor Green
Write-Host ""
Write-Host "  Launch 'Tunnel Pilot' from the Start Menu." -ForegroundColor DarkGray
Write-Host ""
Write-Warn "This build is not code-signed."
Write-Host "     If SmartScreen warns on first run: click 'More info' -> 'Run anyway'." -ForegroundColor DarkGray
Write-Host "     Self-updates ARE cryptographically signed (minisign); only the" -ForegroundColor DarkGray
Write-Host "     initial download is unsigned at the OS level." -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Docs: https://github.com/$REPO" -ForegroundColor DarkGray
Write-Host ""
