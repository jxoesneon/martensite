<#
.SYNOPSIS
    Builds the native Windows WiX .msi installer for the Martensite toolchain and Widget Catalog.

.DESCRIPTION
    Compiles cargo-martensite.exe and widget_catalog.exe into a Windows Installer (.msi)
    using WiX Toolset v3 or v4+. Configures per-machine installation, start menu shortcuts,
    and adds the installation bin directory to the system PATH.

.PARAMETER Version
    The version string to embed in the installer (e.g. "0.20.0").
    Defaults to the workspace version in Cargo.toml.

.PARAMETER Arch
    Target architecture ("x64", "x86", or "arm64"). Defaults to "x64".

.PARAMETER BinDir
    Path to directory containing cargo-martensite.exe and widget_catalog.exe.
    If unspecified, automatically resolves from target/release or target/<target>/release.

.PARAMETER OutputDir
    Destination directory for the emitted .msi package. Defaults to repo dist/.

.PARAMETER SkipBuild
    Skip running `cargo build` even if binaries are missing in BinDir.

.EXAMPLE
    .\packaging\windows\build-msi.ps1 -Version "0.20.0"
#>

[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$Arch = "x64",
    [string]$BinDir = "",
    [string]$OutputDir = "$PSScriptRoot\..\..\dist",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path "$PSScriptRoot\..\..").Path
$WxsFile = "$PSScriptRoot\wix\main.wxs"

Write-Host "=== Martensite Windows MSI Packaging ===" -ForegroundColor Cyan

# 1. Determine Version
if (-not $Version) {
    $CargoToml = Get-Content "$RepoRoot\Cargo.toml" -Raw
    if ($CargoToml -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
    } else {
        $Version = "0.20.0"
    }
}
# Strip leading 'v' if present for WiX version compatibility (Major.Minor.Build.Revision)
$WixVersion = $Version.TrimStart("v")
# Ensure at least 3 parts for WiX (e.g. 0.20.0)
$VersionParts = $WixVersion.Split(".")
while ($VersionParts.Count -lt 3) {
    $WixVersion = "$WixVersion.0"
    $VersionParts = $WixVersion.Split(".")
}
Write-Host "Target Version: $Version (WiX product version: $WixVersion)" -ForegroundColor Green

# 2. Resolve Binary Directory
if (-not $BinDir) {
    $CandidateDirs = @(
        "$RepoRoot\target\x86_64-pc-windows-msvc\release",
        "$RepoRoot\target\release"
    )
    foreach ($Candidate in $CandidateDirs) {
        if ((Test-Path "$Candidate\cargo-martensite.exe") -and (Test-Path "$Candidate\widget_catalog.exe")) {
            $BinDir = $Candidate
            break
        }
    }
}

if (-not $BinDir -or -not (Test-Path "$BinDir\cargo-martensite.exe") -or -not (Test-Path "$BinDir\widget_catalog.exe")) {
    if ($SkipBuild) {
        Write-Error "Required binaries (cargo-martensite.exe, widget_catalog.exe) not found in '$BinDir' and -SkipBuild was specified."
        exit 1
    }
    Write-Host "Binaries not found. Building release binaries with Cargo..." -ForegroundColor Yellow
    Push-Location $RepoRoot
    try {
        & cargo build --release --locked -p cargo-martensite -p widget_catalog
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Cargo build failed with exit code $LASTEXITCODE"
            exit $LASTEXITCODE
        }
    } finally {
        Pop-Location
    }
    $BinDir = "$RepoRoot\target\release"
}

$ResolvedBinDir = (Resolve-Path $BinDir).Path
Write-Host "Using Binaries from: $ResolvedBinDir" -ForegroundColor Green

# Verify binary existence
$CargoMartensiteExe = "$ResolvedBinDir\cargo-martensite.exe"
$WidgetCatalogExe = "$ResolvedBinDir\widget_catalog.exe"
if (-not (Test-Path $CargoMartensiteExe)) {
    Write-Error "Missing binary: $CargoMartensiteExe"
    exit 1
}
if (-not (Test-Path $WidgetCatalogExe)) {
    Write-Error "Missing binary: $WidgetCatalogExe"
    exit 1
}

# 3. Locate WiX Toolset
$WixExe = Get-Command "wix.exe" -ErrorAction SilentlyContinue
$CandleExe = Get-Command "candle.exe" -ErrorAction SilentlyContinue
$LightExe = Get-Command "light.exe" -ErrorAction SilentlyContinue

$WixToolType = "" # "v4" or "v3"

if ($WixExe) {
    $WixToolType = "v4"
    Write-Host "Found WiX v4+: $($WixExe.Source)" -ForegroundColor Green
} elseif ($CandleExe -and $LightExe) {
    $WixToolType = "v3"
    Write-Host "Found WiX v3: $($CandleExe.Source)" -ForegroundColor Green
} else {
    # Check standard install locations for WiX v3
    $CommonPaths = @(
        "${env:WIX}bin",
        "${env:ProgramFiles(x86)}\WiX Toolset v3.14\bin",
        "${env:ProgramFiles(x86)}\WiX Toolset v3.11\bin",
        "${env:ProgramFiles}\WiX Toolset v3.14\bin",
        "${env:ProgramFiles}\WiX Toolset v3.11\bin"
    )
    foreach ($Path in $CommonPaths) {
        if ($Path -and (Test-Path "$Path\candle.exe") -and (Test-Path "$Path\light.exe")) {
            $CandleExe = Get-Item "$Path\candle.exe"
            $LightExe = Get-Item "$Path\light.exe"
            $WixToolType = "v3"
            Write-Host "Found WiX v3 in: $Path" -ForegroundColor Green
            break
        }
    }
}

if (-not $WixToolType) {
    # Try dotnet tool wix if dotnet CLI is available
    $DotnetCmd = Get-Command "dotnet" -ErrorAction SilentlyContinue
    if ($DotnetCmd) {
        $DotnetWix = & $DotnetCmd.Source tool run wix --version 2>$null
        if ($LASTEXITCODE -eq 0) {
            $WixToolType = "dotnet-v4"
            Write-Host "Found WiX via dotnet tool run wix ($DotnetWix)" -ForegroundColor Green
        }
    }
}

if (-not $WixToolType) {
    Write-Error @"
WiX Toolset not found!
To install WiX Toolset:
  - WiX v4+ (recommended): dotnet tool install --global wix
  - WiX v3: winget install WiX.Toolset or choco install wixtoolset
"@
    exit 1
}

# 4. Prepare Output Directory
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
}
$ResolvedOutputDir = (Resolve-Path $OutputDir).Path
$MsiFileName = "martensite-$Version-$Arch.msi"
$OutputMsi = "$ResolvedOutputDir\$MsiFileName"

Write-Host "Building MSI: $OutputMsi" -ForegroundColor Cyan

# 5. Compile WiX
$IntermediateDir = "$RepoRoot\target\wix"
if (-not (Test-Path $IntermediateDir)) {
    New-Item -ItemType Directory -Force -Path $IntermediateDir | Out-Null
}

if ($WixToolType -eq "v4") {
    & $WixExe build "$WxsFile" `
        -arch $Arch `
        -d Version="$WixVersion" `
        -d BinDir="$ResolvedBinDir" `
        -o "$OutputMsi"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "WiX v4 compilation failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
} elseif ($WixToolType -eq "dotnet-v4") {
    & dotnet tool run wix build "$WxsFile" `
        -arch $Arch `
        -d Version="$WixVersion" `
        -d BinDir="$ResolvedBinDir" `
        -o "$OutputMsi"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "dotnet wix compilation failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
} elseif ($WixToolType -eq "v3") {
    $WixObj = "$IntermediateDir\main.wixobj"
    & $CandleExe.Source -nologo `
        -arch $Arch `
        -dVersion="$WixVersion" `
        -dBinDir="$ResolvedBinDir" `
        -out "$WixObj" `
        "$WxsFile"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "WiX candle compilation failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    & $LightExe.Source -nologo `
        -ext WixUIExtension `
        -out "$OutputMsi" `
        "$WixObj"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "WiX light linking failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
}

# 6. Verify and Compute Hash
if (Test-Path $OutputMsi) {
    $MsiItem = Get-Item $OutputMsi
    $Sha256 = (Get-FileHash -Path $OutputMsi -Algorithm SHA256).Hash.ToLower()
    $Sha256File = "$OutputMsi.sha256"
    [System.IO.File]::WriteAllText($Sha256File, "$Sha256  $MsiFileName`n")

    Write-Host "`nSUCCESS: Windows MSI package created successfully!" -ForegroundColor Green
    Write-Host "File:   $OutputMsi" -ForegroundColor Green
    Write-Host "Size:   $($MsiItem.Length) bytes" -ForegroundColor Green
    Write-Host "SHA256: $Sha256" -ForegroundColor Green
} else {
    Write-Error "Expected MSI was not created at: $OutputMsi"
    exit 1
}
