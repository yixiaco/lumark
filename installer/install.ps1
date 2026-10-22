param(
    [string]$Binary = (Join-Path $PSScriptRoot "..\target\release\lumark-app.exe"),
    [string]$SfxStub = (Join-Path $PSScriptRoot "..\target\release\lumark-sfx.exe")
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $Binary)) {
    throw "找不到 Lumark 可执行文件：$Binary。请先运行 cargo build --release -p lumark-app"
}
if (-not (Test-Path -LiteralPath $SfxStub)) {
    throw "找不到自解压运行时模块：$SfxStub。请先运行 cargo build --release -p lumark-sfx"
}

$programs = Join-Path $env:LOCALAPPDATA "Programs\Lumark"
New-Item -ItemType Directory -Force -Path $programs | Out-Null
Copy-Item -LiteralPath $Binary -Destination (Join-Path $programs "Lumark.exe") -Force
Copy-Item -LiteralPath $SfxStub -Destination (Join-Path $programs "lumark-sfx.exe") -Force

& (Join-Path $programs "Lumark.exe") --register-shell

$sendTo = Join-Path $env:APPDATA "Microsoft\Windows\SendTo"
New-Item -ItemType Directory -Force -Path $sendTo | Out-Null
$shortcutPath = Join-Path $sendTo "Lumark 压缩.lnk"
$shell = New-Object -ComObject WScript.Shell
try {
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = Join-Path $programs "Lumark.exe"
    $shortcut.Arguments = '--compress "%1"'
    $shortcut.WorkingDirectory = $programs
    $shortcut.Description = "使用 Lumark 压缩"
    $shortcut.Save()
} finally {
    if ($null -ne $shell) {
        [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null
    }
}

Write-Host "Lumark 已安装到 $programs（当前用户，无需管理员权限）。"
Write-Host "请在 Windows 设置 > 应用 > 默认应用中手动选择 Lumark。"
