param(
    [string]$Version = "1.28.6"
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$cacheDirectory = Join-Path $projectRoot ".gstreamer\cache"
$installDirectory = Join-Path $projectRoot ".gstreamer\1.0\msvc_x86_64"
$installerName = "gstreamer-1.0-msvc-x86_64-$Version.exe"
$installerPath = Join-Path $cacheDirectory $installerName
$baseUrl = "https://gstreamer.freedesktop.org/data/pkg/windows/$Version/msvc"

New-Item -ItemType Directory -Path $cacheDirectory -Force | Out-Null

if (-not (Test-Path -LiteralPath $installerPath) -or (Get-Item -LiteralPath $installerPath).Length -eq 0) {
    $partialPath = "$installerPath.partial"
    Remove-Item -LiteralPath $partialPath -Force -ErrorAction SilentlyContinue
    Invoke-WebRequest -Uri "$baseUrl/$installerName" -OutFile $partialPath
    Move-Item -LiteralPath $partialPath -Destination $installerPath -Force
}

$checksumPath = "$installerPath.sha256sum"
Invoke-WebRequest -Uri "$baseUrl/$installerName.sha256sum" -OutFile $checksumPath
$expectedHash = ((Get-Content -LiteralPath $checksumPath -Raw).Trim() -split "\s+")[0].ToUpperInvariant()
$actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installerPath).Hash.ToUpperInvariant()
if ($actualHash -ne $expectedHash) {
    throw "GStreamer installer checksum mismatch. Expected $expectedHash but received $actualHash."
}

$arguments = @(
    "/CURRENTUSER",
    "/TYPE=devel",
    "/DIR=$installDirectory",
    "/VERYSILENT",
    "/NORESTART"
)
$process = Start-Process -FilePath $installerPath -ArgumentList $arguments -Wait -PassThru
if ($process.ExitCode -ne 0) {
    throw "The GStreamer installer exited with code $($process.ExitCode)."
}

$gstLaunch = Join-Path $installDirectory "bin\gst-launch-1.0.exe"
if (-not (Test-Path -LiteralPath $gstLaunch)) {
    throw "GStreamer installation completed without the expected gst-launch-1.0 executable."
}

Write-Host "GStreamer $Version is ready at $installDirectory"
Write-Host "Use 'npm run tauri:gstreamer -- dev' so Cargo can locate the SDK."
