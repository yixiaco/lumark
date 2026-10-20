param(
    [switch]$KeepUserData = $true
)

$ErrorActionPreference = "Stop"
$programs = Join-Path $env:LOCALAPPDATA "Programs\Lumark"
$exe = Join-Path $programs "Lumark.exe"
if (Test-Path -LiteralPath $exe) {
    Start-Process -FilePath $exe -ArgumentList "--unregister-shell" -Wait -WindowStyle Hidden | Out-Null
    Start-Sleep -Milliseconds 500
}
$sendToShortcut = Join-Path $env:APPDATA "Microsoft\Windows\SendTo\Lumark 压缩.lnk"
if (Test-Path -LiteralPath $sendToShortcut) {
    Remove-Item -LiteralPath $sendToShortcut -Force
}
if (Test-Path -LiteralPath $programs) {
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        try {
            Remove-Item -LiteralPath $programs -Recurse -Force -ErrorAction Stop
            break
        } catch {
            if ($attempt -eq 4) { throw }
            Start-Sleep -Milliseconds 500
        }
    }
}
if (-not $KeepUserData) {
    $appData = Join-Path $env:APPDATA "Lumark"
    $localData = Join-Path $env:LOCALAPPDATA "Lumark"
    if (Test-Path -LiteralPath $appData) {
        Remove-Item -LiteralPath $appData -Recurse -Force
    }
    if (Test-Path -LiteralPath $localData) {
        Remove-Item -LiteralPath $localData -Recurse -Force
    }
}
Write-Host "Lumark 已卸载。"
