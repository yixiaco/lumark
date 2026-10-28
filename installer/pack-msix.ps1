<#
.SYNOPSIS
    把 release 目录中的 Lumark 打包为全信任 MSIX。

.DESCRIPTION
    自动从 crates/lumark-app/assets/lumark.ico 生成 Store/开始菜单所需的 PNG，
    生成 AppxManifest.xml，并调用 Windows SDK 的 makeappx.exe。打包产物默认
    未签名，安装前使用 signtool 或企业证书签名。
#>
[CmdletBinding()]
param(
    [string]$InputDir = (Join-Path $PSScriptRoot "..\target\release"),
    [string]$OutputPath = (Join-Path $PSScriptRoot "..\target\release\Lumark.msix"),
    [string]$Publisher = "CN=Lumark",
    [string]$Version = "1.0.0.0",
    [switch]$KeepStaging
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

function Find-MakeAppx {
    $command = Get-Command "makeappx.exe" -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path -LiteralPath $kits) {
        $candidate = Get-ChildItem -Path $kits -Directory |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "x64\makeappx.exe" } |
            Where-Object { Test-Path -LiteralPath $_ } |
            Select-Object -First 1
        if ($candidate) {
            return $candidate
        }
    }
    throw "makeappx.exe was not found. Install the Windows SDK."
}

$input = (Resolve-Path -LiteralPath $InputDir).ProviderPath
$appBinary = Join-Path $input "lumark-app.exe"
$sfxBinary = Join-Path $input "lumark-sfx.exe"
if (-not (Test-Path -LiteralPath $appBinary)) {
    throw "Missing $appBinary. Run cargo build --release -p lumark-app first."
}
if (-not (Test-Path -LiteralPath $sfxBinary)) {
    throw "Missing $sfxBinary. Run cargo build --release -p lumark-sfx first."
}
$ico = Join-Path $PSScriptRoot "..\crates\lumark-app\assets\lumark.ico"
if (-not (Test-Path -LiteralPath $ico)) {
    throw "Missing application icon: $ico"
}

$output = [System.IO.Path]::GetFullPath($OutputPath)
$outputDir = Split-Path -Parent $output
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
$staging = Join-Path ([System.IO.Path]::GetTempPath()) ("lumark-msix-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path (Join-Path $staging "Assets") | Out-Null

try {
    Copy-Item -LiteralPath $appBinary -Destination (Join-Path $staging "Lumark.exe")
    Copy-Item -LiteralPath $sfxBinary -Destination (Join-Path $staging "lumark-sfx.exe")

    $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot "msix\AppxManifest.xml") -Raw
    $manifest = $manifest.Replace("__PUBLISHER__", $Publisher).Replace("__VERSION__", $Version)
    Set-Content -LiteralPath (Join-Path $staging "AppxManifest.xml") -Value $manifest -Encoding UTF8

    $icon = [System.Drawing.Icon]::new($ico)
    $source = $icon.ToBitmap()
    function Save-Square([string]$Name, [int]$Size) {
        $bitmap = New-Object System.Drawing.Bitmap($Size, $Size)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([System.Drawing.Color]::Transparent)
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.DrawImage($source, 0, 0, $Size, $Size)
            $bitmap.Save((Join-Path $staging "Assets\$Name"), [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
    Save-Square "StoreLogo.png" 50
    Save-Square "Square44x44Logo.png" 44
    Save-Square "Square150x150Logo.png" 150
    $wide = New-Object System.Drawing.Bitmap(310, 150)
    $wideGraphics = [System.Drawing.Graphics]::FromImage($wide)
    try {
        $wideGraphics.Clear([System.Drawing.Color]::Transparent)
        $wideGraphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $wideGraphics.DrawImage($source, 62, 12, 186, 126)
        $wide.Save((Join-Path $staging "Assets\Wide310x150Logo.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $wideGraphics.Dispose()
        $wide.Dispose()
        $source.Dispose()
        $icon.Dispose()
    }

    $makeappx = Find-MakeAppx
    & $makeappx pack /d $staging /p $output /o
    if ($LASTEXITCODE -ne 0) {
        throw "makeappx failed with exit code $LASTEXITCODE"
    }
    Write-Host "MSIX created: $output"
} finally {
    if (-not $KeepStaging) {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "Staging kept: $staging"
    }
}
