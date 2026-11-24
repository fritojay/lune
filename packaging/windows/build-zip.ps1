# Builds dist/Lune (lune.exe with Qt and the MSVC runtime beside it, placed
# by windeployqt) and dist/Lune-<version>-windows-x64.zip. Nothing is
# installed: unzip the folder anywhere and run lune.exe. Its data folder makes
# it portable, so the library and settings are kept there too.
#
# Needs x64 Windows with Visual Studio's C++ tools, Python 3.11+, and Qt 6.10
# (msvc2022_64) from aqtinstall with QMAKE pointing at it (DEVELOPMENT.md).
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Native {
    & $args[0] @($args | Select-Object -Skip 1)
    if ($LASTEXITCODE -ne 0) { throw "$($args[0]) exited with code $LASTEXITCODE" }
}

$Root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $Root

# $IsWindows only exists in PowerShell 7; Windows PowerShell 5.1 runs only on Windows.
if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
    throw 'build-zip.ps1 builds the Windows app on Windows'
}
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
    throw "build-zip.ps1 builds the x64 app on x64 Windows, not $env:PROCESSOR_ARCHITECTURE"
}
if (-not $env:QMAKE) { throw 'set QMAKE to the qmake of Qt 6.10, see DEVELOPMENT.md' }

$QtBins = & $env:QMAKE -query QT_INSTALL_BINS
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$Root\target" }
$Dist = "$Root\dist"
$App = "$Dist\Lune"

$Version = Invoke-Native python packaging/check-version.py
Write-Host "Lune $Version, Qt $(& $env:QMAKE -query QT_VERSION)"

Invoke-Native cargo build --locked --release -p lune-app --bin lune

if (Test-Path $App) { Remove-Item -Recurse -Force $App }
New-Item -ItemType Directory -Path $App | Out-Null
Copy-Item "$TargetDir\release\lune.exe" $App

# Qt's DLLs, plugins, and the QML modules the app's QML files import. Windows
# 10 and later have the D3D compiler, and the RHI doesn't need software OpenGL.
Invoke-Native "$QtBins\windeployqt.exe" --release --qmldir "$Root\crates\app\qml" `
    --no-translations --no-system-d3d-compiler --no-opengl-sw --no-compiler-runtime `
    --verbose 1 "$App\lune.exe"

# The MSVC runtime, app-local, so nothing needs installing. windeployqt would
# only add the vc_redist installer.
$Vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$VsPath = Invoke-Native $Vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$Redist = Get-ChildItem "$VsPath\VC\Redist\MSVC" -Directory |
    Where-Object Name -Match '^\d+(\.\d+)+$' |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1
if (-not $Redist) { throw "no MSVC redistributable under $VsPath" }
$Crt = Get-ChildItem "$($Redist.FullName)\x64" -Directory -Filter 'Microsoft.VC*.CRT' |
    Select-Object -First 1
if (-not $Crt) { throw "no x64 CRT in $($Redist.FullName)" }
Copy-Item "$($Crt.FullName)\*.dll" $App

# The portable data folder, with a file in it so the zip keeps it.
New-Item -ItemType Directory -Path "$App\data" | Out-Null
Set-Content -Path "$App\data\README.txt" -Value @(
    "Lune keeps its library database, settings, cover cache, and log (lune.log) in this folder."
    "To update, delete everything in the Lune folder except this one, then unzip the new version where the old one was."
    "Delete it to start over."
)

$Zip = "$Dist\Lune-$Version-windows-x64.zip"
if (Test-Path $Zip) { Remove-Item -Force $Zip }
Compress-Archive -Path $App -DestinationPath $Zip

Write-Host 'Built:'
Write-Host "  $App"
Write-Host "  $Zip"
