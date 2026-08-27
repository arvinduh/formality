# formality (fml) installer for Windows
# https://github.com/arvinduh/formality

$ErrorActionPreference = 'Stop'

function Write-Info {
    param([string]$Message)
    Write-Host "info: " -ForegroundColor Cyan -NoNewline
    Write-Host $Message
}

function Write-Success {
    param([string]$Message)
    Write-Host "success: " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Warn {
    param([string]$Message)
    Write-Host "warning: " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Write-Err {
    param([string]$Message)
    Write-Host "error: " -ForegroundColor Red -NoNewline
    Write-Host $Message
    exit 1
}

$target = "x86_64-pc-windows-msvc"
$assetName = "fml-$target.zip"
$downloadUrl = "https://github.com/arvinduh/formality/releases/latest/download/$assetName"

$installDir = if ($env:FML_INSTALL_DIR) {
    $env:FML_INSTALL_DIR
} else {
    Join-Path $HOME "bin"
}

Write-Info "Detected platform: $target"
Write-Info "Installing formality into $installDir..."

if (-not (Test-Path -Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
$zipPath = Join-Path $tempDir $assetName

try {
    Write-Info "Downloading $downloadUrl..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -UseBasicParsing

    Expand-Archive -Path $zipPath -DestinationPath $tempDir -Force
    $exeSource = Join-Path $tempDir "fml.exe"
    if (-not (Test-Path -Path $exeSource)) {
        Write-Err "Extracted archive did not contain 'fml.exe'."
    }

    $destExe = Join-Path $installDir "fml.exe"
    Copy-Item -Path $exeSource -Destination $destExe -Force
}
finally {
    if (Test-Path -Path $tempDir) {
        Remove-Item -Recurse -Force -Path $tempDir -ErrorAction SilentlyContinue
    }
}

# Verify execution
$destExe = Join-Path $installDir "fml.exe"
try {
    $versionOutput = & $destExe --version 2>&1
    Write-Success "Successfully installed $versionOutput to $destExe"
}
catch {
    Write-Err "Installed binary at $destExe failed to execute: $_"
}

# Check if installDir is in PATH
$pathEntries = $env:PATH -split [System.IO.Path]::PathSeparator
$inPath = $false
foreach ($entry in $pathEntries) {
    if ($entry.TrimEnd('\/') -eq $installDir.TrimEnd('\/')) {
        $inPath = $true
        break
    }
}

if (-not $inPath) {
    Write-Host ""
    Write-Warn "$installDir is not in your PATH."
    Write-Host "To make 'fml' accessible from PowerShell, add $installDir to your PATH."
    Write-Host ""
    Write-Host "For current session:" -ForegroundColor Gray
    Write-Host "  `$env:PATH = `"`$env:PATH;$installDir`"" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Permanently for current user:" -ForegroundColor Gray
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$installDir', 'User')" -ForegroundColor Cyan
    Write-Host ""
}
