#requires -Version 5.1
<#
.SYNOPSIS
    使用 Inno Setup 生成 Lumark Windows 安装程序。
.DESCRIPTION
    按 GPUI Kit 官方打包文档的 Windows 流程：先校验 release 产物，暂存到
    dist\windows，再调用 Inno Setup 的 ISCC.exe。安装器携带 app、SFX 运行时
    和许可文本；安装后由 Lumark 自己注册当前用户的经典右键菜单。
.PARAMETER Version
    安装器版本号，同时传给 ISCC 的 AppVersion。
.PARAMETER InputDir
    release 产物目录，默认 target\release（相对仓库根目录）。
.PARAMETER OutputDir
    暂存和安装器输出目录，默认 dist。
.PARAMETER IsccPath
    ISCC.exe 路径；未提供时在 PATH 和 Inno Setup 默认安装目录中查找。
.PARAMETER SignThumbprint
    可选，当前用户证书存储中的代码签名证书 SHA-1 指纹。
.PARAMETER SignPfxPath
    可选，PFX 证书路径；与 -SignThumbprint 二选一。
.PARAMETER SignPfxPassword
    PFX 密码。
#>
[CmdletBinding()]
param(
    [string]$Version = "0.1.0",
    [string]$InputDir,
    [string]$OutputDir,
    [string]$IsccPath,
    [string]$SignThumbprint,
    [string]$SignPfxPath,
    [string]$SignPfxPassword
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
if (-not $InputDir) {
    $InputDir = Join-Path $repo "target\release"
}
if (-not $OutputDir) {
    $OutputDir = Join-Path $repo "dist"
}
$InputDir = (Resolve-Path -LiteralPath $InputDir).ProviderPath
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$app = Join-Path $InputDir "lumark-app.exe"
$sfx = Join-Path $InputDir "lumark-sfx.exe"
foreach ($file in @($app, $sfx)) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "缺少 release 产物：$file。请先运行 cargo build --release --locked -p lumark-app -p lumark-sfx。"
    }
}

$stage = Join-Path $OutputDir "windows"
if (Test-Path -LiteralPath $stage) {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $app -Destination (Join-Path $stage "Lumark.exe") -Force
Copy-Item -LiteralPath $sfx -Destination (Join-Path $stage "lumark-sfx.exe") -Force
foreach ($notice in @("README.md", "THIRD_PARTY_NOTICES.md")) {
    $source = Join-Path $repo $notice
    if (Test-Path -LiteralPath $source) {
        Copy-Item -LiteralPath $source -Destination (Join-Path $stage $notice) -Force
    }
}

function Find-Iscc([string]$Explicit) {
    if ($Explicit) {
        if (-not (Test-Path -LiteralPath $Explicit)) {
            throw "ISCC.exe 不存在：$Explicit"
        }
        return (Resolve-Path -LiteralPath $Explicit).ProviderPath
    }
    $command = Get-Command "ISCC.exe" -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $localPrograms = if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA "Programs" }
    foreach ($root in @(${env:ProgramFiles}, ${env:ProgramFiles(x86)}, $localPrograms)) {
        if (-not $root) { continue }
        foreach ($edition in @("Inno Setup 6", "Inno Setup 5")) {
            $candidate = Join-Path (Join-Path $root $edition) "ISCC.exe"
            if (Test-Path -LiteralPath $candidate) {
                return $candidate
            }
        }
    }
    throw "未找到 ISCC.exe。请安装 Inno Setup 6：https://jrsoftware.org/isdl.php"
}

$signed = $SignThumbprint -or $SignPfxPath
if ($signed) {
    $signArgs = @{
        Path = @(
            (Join-Path $stage "Lumark.exe"),
            (Join-Path $stage "lumark-sfx.exe")
        )
    }
    if ($SignThumbprint) { $signArgs["Thumbprint"] = $SignThumbprint }
    if ($SignPfxPath) { $signArgs["PfxPath"] = $SignPfxPath }
    if ($SignPfxPassword) { $signArgs["PfxPassword"] = $SignPfxPassword }
    & (Join-Path $PSScriptRoot "sign.ps1") @signArgs
}

$iscc = Find-Iscc $IsccPath
$script = Join-Path $PSScriptRoot "lumark.iss"
& $iscc "/DAppVersion=$Version" $script
if ($LASTEXITCODE -ne 0) {
    throw "ISCC 编译失败，退出码 $LASTEXITCODE"
}

$installer = Join-Path $OutputDir "Lumark-Setup-$Version.exe"
if (-not (Test-Path -LiteralPath $installer)) {
    throw "ISCC 没有生成预期安装器：$installer"
}
if ($signed) {
    $signArgs = @{ Path = @($installer) }
    if ($SignThumbprint) { $signArgs["Thumbprint"] = $SignThumbprint }
    if ($SignPfxPath) { $signArgs["PfxPath"] = $SignPfxPath }
    if ($SignPfxPassword) { $signArgs["PfxPassword"] = $SignPfxPassword }
    & (Join-Path $PSScriptRoot "sign.ps1") @signArgs
}
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$installer.sha256" -Value $hash -Encoding ASCII
Write-Host "安装器：$installer"
Write-Host "SHA-256：$hash"
