#requires -Version 5.1
<#
.SYNOPSIS
    使用 Inno Setup 生成 Lumark Windows 安装程序。
.DESCRIPTION
    按 GPUI Kit 官方打包文档的 Windows 流程：先校验 release 产物，暂存到
    dist\windows，再调用 Inno Setup 的 ISCC.exe。安装器携带 app、SFX 运行时
    和许可文本；安装器需要管理员权限，默认安装到 Program Files，安装后由
    Lumark 以启动安装器的原始用户身份注册当前用户的经典右键菜单。

    日常发布建议直接运行 tools/build_release.ps1：它会先编译、检查并修复
    MOTW 和 Low 完整性标签，再调用本脚本，最后生成便携包和 SHA256SUMS.txt。
.PARAMETER Version
    安装器版本号，同时传给 ISCC 的 AppVersion。
.PARAMETER InputDir
    release 产物目录，默认 target\release（相对仓库根目录）。
.PARAMETER OutputDir
    暂存和安装器输出目录，默认 dist。
.PARAMETER IsccPath
    ISCC.exe 路径；未提供时使用仓库内置的 installer\tools\innosetup\ISCC.exe。
.PARAMETER SignThumbprint
    可选，当前用户证书存储中的代码签名证书 SHA-1 指纹。
.PARAMETER SignPfxPath
    可选，PFX 证书路径；与 -SignThumbprint 二选一。
.PARAMETER SignPfxPassword
    PFX 密码。
.PARAMETER NoTimestamp
    不对签名做 RFC 3161 时间戳。开发用自签名证书应启用此开关。
#>
[CmdletBinding()]
param(
    [string]$Version = "0.1.0",
    [string]$InputDir,
    [string]$OutputDir,
    [string]$IsccPath,
    [string]$SignThumbprint,
    [string]$SignPfxPath,
    [string]$SignPfxPassword,
    [switch]$NoTimestamp
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
        throw "缺少 release 产物：$file。请先运行 cargo build --release --locked -p lumark-app，再运行 cargo build --profile sfx --locked -p lumark-sfx 并把 target\sfx\lumark-sfx.exe 复制到 target\release\。"
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
# 可选随包 Oodle（build_release.ps1 -OodleDll 会放到 release\oodle）。
$oodleDir = Join-Path $InputDir "oodle"
if (Test-Path -LiteralPath $oodleDir) {
    Copy-Item -LiteralPath $oodleDir -Destination (Join-Path $stage "oodle") -Recurse -Force
}

function Find-Iscc([string]$Explicit) {
    if ($Explicit) {
        if (-not (Test-Path -LiteralPath $Explicit)) {
            throw "ISCC.exe 不存在：$Explicit"
        }
        return (Resolve-Path -LiteralPath $Explicit).ProviderPath
    }
    $bundled = Join-Path $PSScriptRoot "tools\innosetup\ISCC.exe"
    if (Test-Path -LiteralPath $bundled) {
        return (Resolve-Path -LiteralPath $bundled).ProviderPath
    }
    throw "内置 ISCC.exe 不存在：$bundled。请恢复 installer\tools\innosetup 目录；该目录随仓库提交，构建机不需要安装 Inno Setup。"
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
    if ($NoTimestamp) { $signArgs["NoTimestamp"] = $true }
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
    if ($NoTimestamp) { $signArgs["NoTimestamp"] = $true }
    & (Join-Path $PSScriptRoot "sign.ps1") @signArgs
}

# 构建产物可能从下载的源目录继承 Zone.Identifier；清除它，避免本机运行时
# 触发“来自 Internet 的文件”安全警告。它不影响文件内容或 Authenticode。
Get-ChildItem -LiteralPath $stage -Recurse -File | Unblock-File -ErrorAction SilentlyContinue
Unblock-File -LiteralPath $installer -ErrorAction SilentlyContinue
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$installer.sha256" -Value $hash -Encoding ASCII
Write-Host "安装器：$installer"
Write-Host "SHA-256：$hash"
