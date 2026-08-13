param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArguments
)

$ErrorActionPreference = "Stop"

Write-Warning "scripts/with-gstreamer.ps1 is retained for compatibility; scripts/tauri.mjs now configures GStreamer."
& node (Join-Path $PSScriptRoot "tauri.mjs") @TauriArguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
