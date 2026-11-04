#requires -Version 5.1
<#
.SYNOPSIS
    使用 Inno Setup 生成指定架构的 Lumark Windows 安装程序。

.DESCRIPTION
    按 GPUI Kit 官方打包文档的 Windows 流程：先校验 release 产物架构，
    暂存到 target\dist-stage\<架构>，再调用 Inno Setup 的 ISCC.exe 编译
    installer\lumark.iss。安装器携带 app、SFX 运行时和许可文本；安装器需要
    管理员权限，默认安装到 Program Files（x86 安装包在 64 位系统上安装到
    Program Files (x86)），安装后由 Lumark 以启动安装器的原始用户身份注册
    当前用户的经典右键菜单。

    x64 和 x86 使用同一份 .iss，通过 /DArch 与 /DX64、/DX86 区分安装器架构；
    输出文件名固定为 Lumark-Setup-<版本>-<架构>.exe。

    日常发布建议直接运行 tools/build_release.ps1：它会先编译、检查并修复
    MOTW 和 Low 完整性标签，再调用本脚本，最后生成便携包和 SHA256SUMS.txt。

.PARAMETER Version
    安装器版本号，同时传给 ISCC 的 AppVersion。
.PARAMETER Arch
    目标架构：x64（默认）或 x86。
.PARAMETER InputDir
    release 产物目录；x64 默认 target\release，x86 默认
    target\i686-pc-windows-msvc\release（相对仓库根目录）。
.PARAMETER OutputDir
    安装器输出目录，默认 dist。
.PARAMETER StageDir
    暂存目录，默认 target\dist-stage\<架构>。刻意不放在 dist 下，避免内部
    二进制混入发布目录和 CI artifact。
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
    [ValidateSet("x64", "x86")]
    [string]$Arch = "x64",
    [string]$InputDir,
    [string]$OutputDir,
    [string]$StageDir,
    [string]$IsccPath,
    [string]$SignThumbprint,
    [string]$SignPfxPath,
    [string]$SignPfxPassword,
    [switch]$NoTimestamp
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot

# 相对路径统一按仓库根目录解析，避免依赖调用者的当前工作目录。
function Resolve-RepoPath {
    param([Parameter(Mandatory)][string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $repo $Path))
}

# 读取 PE 头的 Machine 字段，用于确认待打包二进制与 -Arch 一致。
function Get-PeMachine {
    param([Parameter(Mandatory)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $reader = New-Object System.IO.BinaryReader($stream)
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -le 0 -or $peOffset + 6 -gt $stream.Length) {
            throw "PE 头偏移非法：$Path"
        }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "不是有效的 PE 文件：$Path"
        }
        return $reader.ReadUInt16()
    } finally {
        $stream.Dispose()
    }
}

$targetTriple = if ($Arch -eq "x86") { "i686-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
if (-not $InputDir) {
    $InputDir = "target\release"
    if ($Arch -eq "x86") {
        $InputDir = "target\$targetTriple\release"
    }
}
if (-not $OutputDir) { $OutputDir = "dist" }
if (-not $StageDir) { $StageDir = "target\dist-stage\$Arch" }
$InputDir = Resolve-RepoPath $InputDir
$OutputDir = Resolve-RepoPath $OutputDir
$StageDir = Resolve-RepoPath $StageDir
if (-not (Test-Path -LiteralPath $InputDir)) {
    throw "缺少 release 产物目录：$InputDir。请先运行 tools/build_release.ps1 -Arch $Arch 或对应的 cargo build --release --target $targetTriple。"
}
$InputDir = (Resolve-Path -LiteralPath $InputDir).ProviderPath
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$app = Join-Path $InputDir "lumark-app.exe"
$sfx = Join-Path $InputDir "lumark-sfx.exe"
foreach ($file in @($app, $sfx)) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "缺少 release 产物：$file。请先运行 tools/build_release.ps1 -Arch $Arch。"
    }
}

# 防止把 x64 二进制打进 x86 安装包（或反之）。
$expectedMachine = if ($Arch -eq "x86") { 0x014C } else { 0x8664 }
$actualMachine = Get-PeMachine $app
if ($actualMachine -ne $expectedMachine) {
    throw ("release 产物架构与 -Arch {0} 不一致：{1}（PE Machine=0x{2:X4}）" -f $Arch, $app, $actualMachine)
}

$stage = $StageDir
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
$archDefine = if ($Arch -eq "x86") { "/DX86=1" } else { "/DX64=1" }
# Windows PowerShell 5.1 会把 ISCC 的 stderr 当作错误流并中断脚本；
# 这里在调用期间临时降级错误策略，只按退出码判断编译结果。
$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    & $iscc "/DAppVersion=$Version" "/DArch=$Arch" "/DStageDir=$stage" "/DOutputDir=$OutputDir" $archDefine $script
    $isccExitCode = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previousErrorAction
}
if ($isccExitCode -ne 0) {
    throw "ISCC 编译失败，退出码 $isccExitCode"
}

$installer = Join-Path $OutputDir "Lumark-Setup-$Version-$Arch.exe"
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
Write-Host "安装器：$installer"
Write-Host "SHA-256：$hash"
