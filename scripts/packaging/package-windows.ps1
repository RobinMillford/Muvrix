# ==============================================================================
# Muvrix Windows Packaging Script (Phase 1.3)
# Assembles portable distribution archive for Windows x86_64.
# ==============================================================================
param (
    [string]$MpvDir = $env:MPV_DIR,
    [string]$OutputDir = "$PSScriptRoot\..\..\target\dist\muvrix-windows-x86_64"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Muvrix Windows Packaging Pipeline ==="
$RepoRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
Write-Host "Repository Root: $RepoRoot"
Write-Host "Target Dist Dir: $OutputDir"

if (-not $MpvDir) {
    if (Test-Path "C:\mpv-dev") {
        $MpvDir = "C:\mpv-dev"
    } else {
        throw "MPV_DIR environment variable is not set and C:\mpv-dev does not exist."
    }
}
Write-Host "Using libmpv Dev Kit from: $MpvDir"

# 1. Build release binary for Windows MSVC target
Write-Host "Building release binary..."
Push-Location $RepoRoot
try {
    cargo build --release --bin muvrix-ui
} finally {
    Pop-Location
}

# 2. Stage distribution directory
if (Test-Path $OutputDir) {
    Remove-Item -Recurse -Force $OutputDir
}
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
New-Item -ItemType Directory -Force -Path "$OutputDir\assets" | Out-Null

# 3. Copy executable
$ExeSource = "$RepoRoot\target\release\muvrix-ui.exe"
if (-not (Test-Path $ExeSource)) {
    throw "Release binary not found at $ExeSource"
}
Copy-Item $ExeSource -Destination "$OutputDir\muvrix.exe"

# 4. Copy libmpv-2.dll and runtime dependencies
$DllSource = "$MpvDir\libmpv-2.dll"
if (-not (Test-Path $DllSource)) {
    throw "libmpv-2.dll missing at $DllSource"
}
Copy-Item $DllSource -Destination "$OutputDir\libmpv-2.dll"

# Copy any additional runtime DLLs bundled with mpv-dev (e.g. angle, vulkan, etc.)
Get-ChildItem -Path $MpvDir -Filter "*.dll" | ForEach-Object {
    if ($_.Name -ne "libmpv-2.dll") {
        Copy-Item $_.FullName -Destination $OutputDir -Force
    }
}

# 5. Copy shader assets and brand icon
Copy-Item -Recurse "$RepoRoot\assets\shaders" -Destination "$OutputDir\assets\shaders"
Copy-Item "$RepoRoot\assets\branding\muvrix.ico" -Destination "$OutputDir\muvrix.ico"

# 6. Copy licenses and notices
Copy-Item "$RepoRoot\LICENSE" -Destination $OutputDir
Copy-Item "$RepoRoot\THIRD-PARTY-NOTICES.md" -Destination $OutputDir
Copy-Item "$RepoRoot\README.md" -Destination $OutputDir

# 7. Compress into portable zip
$ZipPath = "$RepoRoot\target\dist\muvrix-windows-x86_64.zip"
Write-Host "Creating portable archive: $ZipPath"
Compress-Archive -Path "$OutputDir\*" -DestinationPath $ZipPath -Force

Write-Host "Windows packaging completed successfully: $ZipPath"
