<#
.SYNOPSIS
    安装或卸载已签名的 Lumark MSIX 包。

.DESCRIPTION
    MSIX 必须使用受信任证书签名。脚本可先把 .cer/.pfx 证书导入当前用户
    TrustedPeople，再调用 Add-AppxPackage；卸载时按包名移除 Lumark。
#>
[CmdletBinding()]
param(
    [string]$PackagePath,
    [string]$CertificatePath,
    [string]$CertificatePassword,
    [switch]$TrustCertificate,
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

if (-not $PackagePath) {
    $PackagePath = Join-Path $PSScriptRoot "..\target\release\Lumark.msix"
}

if ($Uninstall) {
    $package = Get-AppxPackage -Name "Lumark" -ErrorAction SilentlyContinue
    if (-not $package) {
        Write-Host "Lumark MSIX is not installed."
        return
    }
    Remove-AppxPackage -Package $package.PackageFullName
    Write-Host "Lumark MSIX removed."
    return
}

if (-not (Test-Path -LiteralPath $PackagePath)) {
    throw "MSIX package not found: $PackagePath"
}

if ($TrustCertificate) {
    if (-not $CertificatePath -or -not (Test-Path -LiteralPath $CertificatePath)) {
        throw "-TrustCertificate requires an existing -CertificatePath."
    }
    if ($CertificatePath.EndsWith(".pfx", [System.StringComparison]::OrdinalIgnoreCase)) {
        if (-not $CertificatePassword) {
            throw "A PFX certificate requires -CertificatePassword."
        }
        $secure = ConvertTo-SecureString $CertificatePassword -AsPlainText -Force
        $certificate = Import-PfxCertificate -FilePath $CertificatePath -CertStoreLocation "Cert:\CurrentUser\My" -Password $secure
    } else {
        $certificate = Import-Certificate -FilePath $CertificatePath -CertStoreLocation "Cert:\CurrentUser\TrustedPeople"
    }
    Write-Host "Certificate imported: $($certificate.Thumbprint)"
}

Add-AppxPackage -Path $PackagePath
Write-Host "Lumark MSIX installed: $PackagePath"
