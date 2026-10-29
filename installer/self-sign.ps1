#requires -Version 5.1
<#
.SYNOPSIS
    创建并信任 Lumark 开发用自签名代码签名证书。
.DESCRIPTION
    证书只写入当前用户的证书存储，不需要管理员权限。脚本会：
    1. 在 CurrentUser\My 创建 Code Signing 证书；
    2. 导出 CER 和 PFX 到 dist\dev-cert；
    3. 把 CER 加入 CurrentUser\Root 和 CurrentUser\TrustedPublisher；
    4. 输出证书指纹，供 pack-installer.ps1 -SignThumbprint 使用。

    自签名只适合本机开发和测试。其它机器必须手动信任导出的 CER，否则仍然
    会显示未知发布者；它也不能替代受信任 CA 的 Authenticode 证书或
    SmartScreen 信誉。
.PARAMETER Subject
    证书主题，默认 CN=Lumark Development, O=Lumark。
.PARAMETER OutputDir
    CER/PFX 输出目录，默认 dist\dev-cert（已被 .gitignore 忽略）。
.PARAMETER PfxPassword
    PFX 密码，默认仅用于本地开发。
.PARAMETER ValidYears
    有效年数，默认 3 年。
#>
[CmdletBinding()]
param(
    [string]$Subject = "CN=Lumark Development, O=Lumark",
    [string]$OutputDir,
    [string]$PfxPassword = "lumark-dev",
    [ValidateRange(1, 10)]
    [int]$ValidYears = 3
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
if (-not $OutputDir) {
    $OutputDir = Join-Path $repo "dist\dev-cert"
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$cert = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -KeyExportPolicy Exportable `
    -KeyLength 3072 `
    -HashAlgorithm SHA256 `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears($ValidYears)

$cerPath = Join-Path $OutputDir "Lumark-Dev.cer"
$pfxPath = Join-Path $OutputDir "Lumark-Dev.pfx"
Export-Certificate -Cert $cert -FilePath $cerPath -Type CERT -Force | Out-Null

foreach ($store in @("Root", "TrustedPublisher")) {
    & certutil.exe -user -addstore -f $store $cerPath | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "无法把开发证书加入当前用户 $store 存储，certutil 退出码 $LASTEXITCODE"
    }
}

$securePassword = ConvertTo-SecureString -String $PfxPassword -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $securePassword -Force | Out-Null

Write-Host "已创建并信任开发证书：$($cert.Thumbprint)"
Write-Host "CER：$cerPath"
Write-Host "PFX：$pfxPath（密码：$PfxPassword）"
Write-Host "签名安装器："
Write-Host "  ./installer/pack-installer.ps1 -Version 0.1.0 -SignThumbprint $($cert.Thumbprint) -NoTimestamp"
Write-Host "其它机器信任：certutil -user -addstore Root `"$cerPath`"; certutil -user -addstore TrustedPublisher `"$cerPath`""

[pscustomobject]@{
    Subject    = $cert.Subject
    Thumbprint = $cert.Thumbprint
    NotAfter   = $cert.NotAfter
    CerPath    = $cerPath
    PfxPath    = $pfxPath
}
