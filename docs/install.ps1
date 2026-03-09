#Requires -Version 5.1
# ══════════════════════════════════════════════════════════════════════════════
#  Tunnel Pilot — Windows Installer (PowerShell)
#  Usage: powershell -ExecutionPolicy Bypass -c "irm https://kalfian.github.io/tunnel-pilot/install.ps1 | iex"
# ══════════════════════════════════════════════════════════════════════════════

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ErrorActionPreference = "Stop"

$REPO     = "kalfian/tunnel-pilot"
$APP_NAME = "Tunnel Pilot"
$DEST     = "$env:APPDATA\$APP_NAME"

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
$asset   = $release.assets | Where-Object { $_.name -like "*.zip" } | Select-Object -First 1

if (-not $asset) {
  Write-Err "No Windows release asset found for $VERSION.`n  Visit: https://github.com/$REPO/releases"
}

# ── Version comparison ────────────────────────────────────────────────────────
$exe = "$DEST\tunnel_pilot.exe"
if (Test-Path $exe) {
  $installed = (Get-Item $exe).VersionInfo.ProductVersion
  $newVer    = $VERSION -replace '^v', ''
  if ($installed -eq $newVer) {
    Write-Ok "Already on latest version $VERSION"
    Write-Host ""
    $answer = Read-Host "  Reinstall anyway? [y/N]"
    if ($answer -notmatch '^[Yy]$') {
      Write-Host "  Nothing to do." -ForegroundColor DarkGray
      Write-Host ""
      exit 0
    }
  } else {
    Write-Ok "Update available: v$installed -> $VERSION"
  }
} else {
  Write-Ok "Latest release: $VERSION"
}

Write-Host ""

# ── Download ──────────────────────────────────────────────────────────────────
$tmpFile = "$env:TEMP\TunnelPilot-install.zip"
$sizeMB  = [math]::Round($asset.size / 1MB, 1)
Write-Step "Downloading $($asset.name) ($sizeMB MB)..."
try {
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmpFile
} catch {
  Write-Err "Download failed. Check your internet connection or try again."
}
Write-Ok "Download complete"
Write-Host ""

# ── Install ───────────────────────────────────────────────────────────────────
# Quit running instance if any
$running = Get-Process -Name "tunnel_pilot" -ErrorAction SilentlyContinue
if ($running) {
  Write-Step "Quitting running instance..."
  $running | Stop-Process -Force
  Start-Sleep -Seconds 1
}

if (Test-Path $DEST) {
  Write-Step "Removing previous installation..."
  Remove-Item -Recurse -Force $DEST
}

New-Item -ItemType Directory -Force -Path $DEST | Out-Null

Write-Step "Extracting archive..."
Expand-Archive -Force -Path $tmpFile -DestinationPath $DEST
Remove-Item $tmpFile -ErrorAction SilentlyContinue

Write-Ok "Extracted to $DEST"

# ── Desktop shortcut ──────────────────────────────────────────────────────────
Write-Step "Creating desktop shortcut..."
$desktopPath = [Environment]::GetFolderPath("Desktop")
$shortcutFile = "$desktopPath\Tunnel Pilot.lnk"
try {
  $shell = New-Object -ComObject WScript.Shell
  $shortcut = $shell.CreateShortcut($shortcutFile)
  $shortcut.TargetPath = $exe
  $shortcut.WorkingDirectory = $DEST
  $shortcut.Description = "SSH Local Port Forwarding Manager"
  $shortcut.IconLocation = "$exe,0"
  $shortcut.Save()
  Write-Ok "Desktop shortcut created: $shortcutFile"
} catch {
  Write-Warn "Could not create desktop shortcut: $_"
}

# ── Start Menu shortcut ───────────────────────────────────────────────────────
$startMenuPath = [Environment]::GetFolderPath("StartMenu") + "\Programs"
$startShortcut = "$startMenuPath\Tunnel Pilot.lnk"
try {
  New-Item -ItemType Directory -Force -Path $startMenuPath | Out-Null
  $shell2 = New-Object -ComObject WScript.Shell
  $sc2 = $shell2.CreateShortcut($startShortcut)
  $sc2.TargetPath = $exe
  $sc2.WorkingDirectory = $DEST
  $sc2.Description = "SSH Local Port Forwarding Manager"
  $sc2.IconLocation = "$exe,0"
  $sc2.Save()
  Write-Ok "Start Menu shortcut created"
} catch {
  Write-Warn "Could not create Start Menu shortcut: $_"
}

# ── Launch ────────────────────────────────────────────────────────────────────
if (Test-Path $exe) {
  Write-Step "Launching Tunnel Pilot..."
  Start-Process $exe
} else {
  Write-Warn "tunnel_pilot.exe not found in archive — check the release at https://github.com/$REPO/releases"
}

# ── Summary ───────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  ─────────────────────────────────────" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  v  Tunnel Pilot $VERSION installed!" -ForegroundColor Green
Write-Host ""
Write-Host "  Run from: $DEST" -ForegroundColor DarkGray
Write-Host "  Docs:     https://github.com/$REPO" -ForegroundColor DarkGray
Write-Host ""
