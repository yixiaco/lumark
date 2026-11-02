#requires -Version 5.1
<#
.SYNOPSIS
    构建 Lumark Windows 发布产物，并修复 MOTW / Low 完整性标签。

.DESCRIPTION
    统一发布入口：编译 lumark-app、lumark-sfx，检查并修复 release 产物的
    Mark-of-the-Web 和 Low Mandatory Level，按需做 Authenticode 签名，
    调用 installer/pack-installer.ps1 生成安装器，最后生成便携包和
    dist\SHA256SUMS.txt。

    “打开文件 - 安全警告”有两个独立来源：
    - Zone.Identifier（MOTW）：文件被浏览器、聊天工具或压缩包标记；
    - Low Mandatory Level：文件由低完整性沙箱进程创建，target 下的新 exe
      会继承工作区的标签。两者都会触发 Windows 附件管理器提示，签名并不能
      消除它们。本脚本会把最终发布产物修复为 Medium，避免用户双击时弹窗。

.PARAMETER Version
    安装器和便携包版本号；默认读取 cargo metadata 中 lumark-app 的版本。

.PARAMETER UseDevCert
    使用当前用户证书存储中受信任的 Lumark 开发证书签名，并强制 -NoTimestamp。
    与显式证书参数互斥。

.PARAMETER SignThumbprint
    代码签名证书 SHA-1 指纹。

.PARAMETER SignPfxPath
    PFX 证书路径；与 -SignThumbprint 二选一。

.PARAMETER SignPfxPassword
    PFX 密码。

.PARAMETER NoTimestamp
    不请求 RFC 3161 时间戳。自签名证书应启用此开关。

.PARAMETER SkipPackage
    只构建、清理并签名 target\release，不生成安装器和便携包。
#>
[CmdletBinding()]
param(
    [string]$Version,
    [switch]$UseDevCert,
    [string]$SignThumbprint,
    [string]$SignPfxPath,
    [string]$SignPfxPassword,
    [switch]$NoTimestamp,
    [switch]$SkipPackage
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$manifest = Join-Path $repo "Cargo.toml"
$releaseDir = Join-Path $repo "target\release"
$app = Join-Path $releaseDir "lumark-app.exe"
$sfx = Join-Path $releaseDir "lumark-sfx.exe"
$dist = Join-Path $repo "dist"
$stage = Join-Path $dist "windows"
$signScript = Join-Path $repo "installer\sign.ps1"
$packScript = Join-Path $repo "installer\pack-installer.ps1"

if (-not (Test-Path -LiteralPath $manifest)) {
    throw "找不到工作区清单：$manifest"
}
if (-not (Test-Path -LiteralPath $signScript) -or -not (Test-Path -LiteralPath $packScript)) {
    throw "找不到 installer\sign.ps1 或 installer\pack-installer.ps1"
}

# 读取一个文件或目录的完整性级别；没有标签的对象按 Windows 默认的 Medium 处理。
function Get-IntegrityLevel {
    param([Parameter(Mandatory)][string]$Path)

    $output = & icacls.exe $Path 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "读取完整性级别失败：$Path（icacls 退出码 $LASTEXITCODE）"
    }
    $label = $output | Select-String -Pattern 'Mandatory Label' | Select-Object -First 1
    if (-not $label) {
        return "Medium"
    }
    $text = $label.Line
    if ($text -match 'Low Mandatory Level') { return "Low" }
    if ($text -match 'Medium Mandatory Level') { return "Medium" }
    if ($text -match 'High Mandatory Level') { return "High" }
    return "Unknown"
}

# 恢复目录的 Low 完整性；目录标签需要 WRITE_OWNER，权限不足时先为当前用户
# 补上工作区根的继承完全控制，再重试。
function Repair-DirectoryIntegrity {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }
    $level = Get-IntegrityLevel $Path
    if ($level -ne "Low") {
        return
    }

    Write-Warning "目录带 Low 完整性标签：$Path；正在恢复为 Medium。"
    & icacls.exe $Path /setintegritylevel "(OI)(CI)Medium" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "目录标签修改被拒绝，正在为当前用户添加工作区根的继承完全控制。"
        & icacls.exe $repo /grant:r "${currentUser}:(OI)(CI)F" | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "无法为 ${currentUser} 授予工作区控制；请用管理员会话执行：icacls `"$repo`" /grant:r `"${currentUser}:(OI)(CI)F`""
        }
        & icacls.exe $Path /setintegritylevel "(OI)(CI)Medium" | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "无法恢复目录完整性：$Path；请用管理员会话执行：icacls `"$Path`" /setintegritylevel `"(OI)(CI)Medium`""
        }
    }

    $level = Get-IntegrityLevel $Path
    if ($level -ne "Medium") {
        throw "目录完整性未恢复为 Medium：$Path（当前 $level）"
    }
    Write-Host "已恢复目录完整性为 Medium：$Path"
}

# 读取 Zone.Identifier 的内容；没有 MOTW 时返回 $null。
function Get-ZoneIdentifier {
    param([Parameter(Mandatory)][string]$Path)

    $stream = Get-Item -LiteralPath $Path -Stream 'Zone.Identifier' -ErrorAction SilentlyContinue
    if (-not $stream) {
        return $null
    }
    $content = Get-Content -LiteralPath $Path -Stream 'Zone.Identifier' -ErrorAction SilentlyContinue
    return ($content -join "`n")
}

# 清除 MOTW 并把 Low 完整性提升为 Medium；修复后重新读取并断言。
function Repair-ReleaseTrust {
    param(
        [Parameter(Mandatory)][string]$Path,
        [string]$Kind = "产物"
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "缺少${Kind}：$Path"
    }

    $zone = Get-ZoneIdentifier $Path
    if ($zone) {
        Write-Host "发现 MOTW（$Kind）：$Path"
        Write-Host $zone
        Unblock-File -LiteralPath $Path -ErrorAction Stop
        if (Get-ZoneIdentifier $Path) {
            throw "MOTW 清除失败：$Path"
        }
        Write-Host "已清除 MOTW：$Path"
    }

    $level = Get-IntegrityLevel $Path
    if ($level -eq "Low") {
        $workspaceLevel = Get-IntegrityLevel $repo
        Write-Warning "发现 Low 完整性标签（$Kind）：$Path"
        if ($workspaceLevel -eq "Low") {
            Write-Warning "工作区根目录仍为 Low；永久修复：icacls `"$repo`" /setintegritylevel `"(OI)(CI)Medium`""
        }
        & icacls.exe $Path /setintegritylevel Medium | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "提升完整性级别失败：$Path（icacls 退出码 $LASTEXITCODE）。如果文件继承了受限工作区的标签且当前账户没有 WRITE_OWNER，请修复仓库根目录：icacls `"$repo`" /setintegritylevel `"(OI)(CI)Medium`""
        }
        $level = Get-IntegrityLevel $Path
        if ($level -ne "Medium") {
            throw "完整性级别未恢复为 Medium：$Path（当前 $level）"
        }
        Write-Host "已把完整性级别提升为 Medium：$Path"
    }

    if ($level -ne "Medium") {
        throw "完整性级别异常：$Path（当前 $level）"
    }
    return [pscustomobject]@{
        Path = $Path
        Motw = $false
        Integrity = $level
    }
}

# 按需调用 installer/sign.ps1；不传证书时直接返回。
function Invoke-Signature {
    param([string[]]$Files)

    if (-not $signed) {
        return
    }
    $arguments = @{ Path = $Files }
    if ($SignThumbprint) { $arguments.Thumbprint = $SignThumbprint }
    if ($SignPfxPath) {
        $arguments.PfxPath = $SignPfxPath
        if ($SignPfxPassword) { $arguments.PfxPassword = $SignPfxPassword }
    }
    if ($NoTimestamp) { $arguments.NoTimestamp = $true }
    & $signScript @arguments
    if (-not $?) {
        throw "签名失败：$($Files -join ', ')"
    }
}

# 校验签名；没有证书时只提示未签名，不阻断本地构建。
function Assert-Signatures {
    param([string[]]$Files)

    foreach ($file in $Files) {
        if (-not (Test-Path -LiteralPath $file)) {
            throw "签名校验时缺少文件：$file"
        }
        $signature = Get-AuthenticodeSignature -LiteralPath $file
        if ($signed) {
            if ($signature.Status -ne "Valid") {
                throw "签名校验失败：$file（$($signature.Status)）"
            }
            Write-Host "签名有效：$file（$($signature.SignerCertificate.Subject)）"
        } else {
            Write-Host "未签名（$($signature.Status)）：$file"
        }
    }
}

# 输出最终产物的 MOTW、完整性级别和签名状态。
function Write-Summary {
    param([string[]]$Files)

    $rows = foreach ($file in $Files) {
        if (-not (Test-Path -LiteralPath $file)) { continue }
        $item = Get-Item -LiteralPath $file
        $zone = Get-ZoneIdentifier $file
        $signatureStatus = if ($item.Extension -in '.exe', '.dll') {
            (Get-AuthenticodeSignature -LiteralPath $file).Status
        } else {
            'N/A'
        }
        [pscustomobject]@{
            Path = $item.FullName.Substring($repo.Length).TrimStart('\')
            SizeMB = [math]::Round($item.Length / 1MB, 2)
            MOTW = if ($zone) { "有" } else { "无" }
            Integrity = Get-IntegrityLevel $file
            Signature = $signatureStatus
        }
    }
    $rows | Format-Table -AutoSize | Out-String | Write-Host
}

if ($UseDevCert -and ($SignThumbprint -or $SignPfxPath)) {
    throw "-UseDevCert 不能与 -SignThumbprint/-SignPfxPath 同时使用"
}
if ($SignThumbprint -and $SignPfxPath) {
    throw "-SignThumbprint 与 -SignPfxPath 只能二选一"
}
if ($SignPfxPassword -and -not $SignPfxPath) {
    throw "-SignPfxPassword 必须与 -SignPfxPath 一起使用"
}

if ($UseDevCert) {
    $certificate = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
        Where-Object { $_.Subject -match 'CN=Lumark Development' -and $_.HasPrivateKey } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if (-not $certificate) {
        throw "没有找到 CN=Lumark Development 的代码签名证书；请先运行 ./installer/self-sign.ps1"
    }
    $SignThumbprint = $certificate.Thumbprint
    $NoTimestamp = $true
    Write-Host "使用开发证书：$($certificate.Subject)（$($certificate.Thumbprint)），跳过时间戳。"
}

$signed = [bool]($SignThumbprint -or $SignPfxPath)
if ($SignPfxPath) {
    $SignPfxPath = (Resolve-Path -LiteralPath $SignPfxPath).ProviderPath
}

if (-not $Version) {
    $metadataJson = & cargo metadata --manifest-path $manifest --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0 -or -not $metadataJson) {
        throw "读取 cargo metadata 失败，退出码 $LASTEXITCODE"
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq "lumark-app" } | Select-Object -First 1
    if (-not $package -or -not $package.version) {
        throw "cargo metadata 中找不到 lumark-app 版本"
    }
    $Version = $package.version
}
$Version = $Version.Trim().TrimStart('v')
if (-not $Version) {
    throw "版本号为空"
}

Write-Host "Lumark 发布构建 $Version"
Write-Host "仓库：$repo"

$workspaceLevel = Get-IntegrityLevel $repo
if ($workspaceLevel -eq "Low") {
    Write-Warning "工作区根目录带 Low 完整性标签；正在恢复为 Medium（需要仓库根目录的写权限）。"
    & icacls.exe $repo /setintegritylevel "(OI)(CI)Medium" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "无法恢复工作区完整性（icacls 退出码 $LASTEXITCODE）。请用管理员会话执行：icacls `"$repo`" /setintegritylevel `"(OI)(CI)Medium`""
    }
    $workspaceLevel = Get-IntegrityLevel $repo
    if ($workspaceLevel -ne "Medium") {
        throw "工作区完整性未恢复为 Medium（当前 $workspaceLevel）"
    }
    Write-Host "工作区已恢复为 Medium。回退：icacls `"$repo`" /setintegritylevel `"(OI)(CI)Low`""
}

foreach ($directory in @(
    (Join-Path $repo "target"),
    (Join-Path $repo "target\release"),
    (Join-Path $repo "target\debug"),
    (Join-Path $repo "dist"),
    (Join-Path $repo "dist\windows")
)) {
    Repair-DirectoryIntegrity $directory
}

Write-Host "编译 release 产物..."
& cargo build --manifest-path $manifest --release --locked -p lumark-app -p lumark-sfx
if ($LASTEXITCODE -ne 0) {
    throw "cargo build 失败，退出码 $LASTEXITCODE"
}
foreach ($file in @($app, $sfx)) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "缺少 release 产物：$file"
    }
}

$targetArtifacts = @($app, $sfx)
foreach ($file in $targetArtifacts) {
    Repair-ReleaseTrust $file "release 产物" | Out-Null
}
foreach ($file in @(
    (Join-Path $repo "target\debug\lumark-app.exe"),
    (Join-Path $repo "target\debug\lumark-sfx.exe")
)) {
    if (Test-Path -LiteralPath $file) {
        Repair-ReleaseTrust $file "debug 产物" | Out-Null
    }
}

$packagedFiles = @()
if (-not $SkipPackage) {
    Write-Host "生成安装器和暂存目录..."
    $packArguments = @{ Version = $Version }
    if ($SignThumbprint) { $packArguments.SignThumbprint = $SignThumbprint }
    if ($SignPfxPath) {
        $packArguments.SignPfxPath = $SignPfxPath
        if ($SignPfxPassword) { $packArguments.SignPfxPassword = $SignPfxPassword }
    }
    if ($NoTimestamp) { $packArguments.NoTimestamp = $true }
    & $packScript @packArguments
    if (-not $?) {
        throw "安装器打包失败"
    }

    $installer = Join-Path $dist "Lumark-Setup-$Version.exe"
    $stageApp = Join-Path $stage "Lumark.exe"
    $stageSfx = Join-Path $stage "lumark-sfx.exe"
    foreach ($file in @($installer, $stageApp, $stageSfx)) {
        if (-not (Test-Path -LiteralPath $file)) {
            throw "安装器打包后缺少产物：$file"
        }
    }
    $packagedFiles = @($installer, $stageApp, $stageSfx)
}

if ($signed) {
    Write-Host "签名 target\release 产物..."
    Invoke-Signature $targetArtifacts
}

foreach ($file in $targetArtifacts) {
    Repair-ReleaseTrust $file "签名后 release 产物" | Out-Null
}
foreach ($file in $packagedFiles) {
    Repair-ReleaseTrust $file "安装器产物" | Out-Null
}

Assert-Signatures ($targetArtifacts + $packagedFiles)

$allArtifacts = @($targetArtifacts)
if (-not $SkipPackage) {
    $installer = Join-Path $dist "Lumark-Setup-$Version.exe"
    Get-ChildItem -LiteralPath $dist -Filter "Lumark-*-portable-x64.zip" -File -ErrorAction SilentlyContinue |
        Remove-Item -Force
    $zip = Join-Path $dist "Lumark-$Version-portable-x64.zip"
    if (Test-Path -LiteralPath $zip) {
        Remove-Item -LiteralPath $zip -Force
    }
    Write-Host "生成便携包..."
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -CompressionLevel Optimal
    Repair-ReleaseTrust $zip "便携包" | Out-Null

    $hashTargets = @()
    $hashTargets += Get-ChildItem -LiteralPath $stage -Recurse -File
    $hashTargets += Get-Item -LiteralPath $installer
    $hashTargets += Get-Item -LiteralPath $zip
    $hashLines = $hashTargets |
        Sort-Object FullName |
        ForEach-Object {
            $relative = $_.FullName.Substring($dist.Length).TrimStart('\') -replace '\\', '/'
            $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash  $relative"
        }
    $sumFile = Join-Path $dist "SHA256SUMS.txt"
    Set-Content -LiteralPath $sumFile -Value ($hashLines -join "`n") -Encoding ASCII
    Write-Host "校验文件：$sumFile"

    $allArtifacts += @($installer, (Join-Path $stage "Lumark.exe"), (Join-Path $stage "lumark-sfx.exe"), $zip)
}

Write-Host ""
Write-Host "发布产物："
Write-Summary $allArtifacts
if ($SkipPackage) {
    Write-Host "已跳过安装器和便携包（-SkipPackage）。"
}
