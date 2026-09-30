$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$sourceExe = Join-Path $repo "target\release\remotehand-toolbox.exe"
$stageDir = Join-Path $repo "bin\windows-x86_64"
$stageExe = Join-Path $stageDir "remotehand-toolbox.exe"
$packageDir = Join-Path $repo "dist\remotehand-toolbox-windows-x86_64"
$packageBinDir = Join-Path $packageDir "bin\windows-x86_64"
$packageUiDir = Join-Path $packageDir "ui"
$packageAssetsDir = Join-Path $packageDir "assets"
$pluginVersion = (Get-Content (Join-Path $repo "plugin.json") -Raw | ConvertFrom-Json).version

Write-Host "Building RemoteHand Toolbox (release)..."
Push-Location $repo
try {
    cargo build --release
} finally {
    Pop-Location
}

if (-not (Test-Path $sourceExe)) {
    throw "Build completed but executable was not found: $sourceExe"
}

New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
Copy-Item -Force $sourceExe $stageExe

if (Test-Path $packageDir) {
    Remove-Item -Recurse -Force $packageDir
}
New-Item -ItemType Directory -Force -Path $packageBinDir | Out-Null
New-Item -ItemType Directory -Force -Path $packageUiDir | Out-Null
New-Item -ItemType Directory -Force -Path $packageAssetsDir | Out-Null

Copy-Item -Force (Join-Path $repo "plugin.json") (Join-Path $packageDir "plugin.json")
Copy-Item -Force (Join-Path $repo "ui\index.html") (Join-Path $packageUiDir "index.html")
Copy-Item -Force (Join-Path $repo "assets\*") $packageAssetsDir
Copy-Item -Force $sourceExe (Join-Path $packageBinDir "remotehand-toolbox.exe")

$packageBytes = (Get-ChildItem -File -Recurse $packageDir | Measure-Object -Property Length -Sum).Sum
$packageMiB = [Math]::Round($packageBytes / 1MB, 2)

Write-Host ""
Write-Host "Plugin runtime staged successfully:"
Write-Host "  $stageExe"
Write-Host ""
Write-Host "Clean installable plugin package created:"
Write-Host "  $packageDir"
Write-Host "  Size: $packageMiB MiB"

$zipPath = Join-Path $repo ("dist\remotehand-toolbox-v{0}-windows-x86_64.zip" -f $pluginVersion)
if (Test-Path $zipPath) {
    Remove-Item -Force $zipPath
}
Compress-Archive -Path (Join-Path $packageDir "*") -DestinationPath $zipPath -CompressionLevel Optimal

$zipHash = (Get-FileHash -Algorithm SHA256 $zipPath).Hash
$checksumPath = Join-Path $repo "dist\SHA256SUMS.txt"
"$zipHash  $(Split-Path -Leaf $zipPath)" | Set-Content -Encoding ascii $checksumPath

Write-Host ""
Write-Host "Validation ZIP created:"
Write-Host "  $zipPath"
Write-Host "  SHA256: $zipHash"
Write-Host ""
Write-Host "Checksum file:"
Write-Host "  $checksumPath"
