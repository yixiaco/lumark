<#
.SYNOPSIS
    使用 Windows SDK 的 signtool 为 Lumark 可执行文件签名或校验签名。

.DESCRIPTION
    默认签名应用和自解压运行时模块。证书可以通过当前用户证书存储的 SHA-1
    指纹提供，也可以直接指定 PFX 文件。脚本在签名后自动执行 Authenticode
    校验；没有证书时可用 -VerifyOnly 只检查已有签名。

.PARAMETER Path
    要签名的文件；默认 target\release\lumark-app.exe 和 lumark-sfx.exe。

.PARAMETER Thumbprint
    当前用户证书存储中的证书 SHA-1 指纹。

.PARAMETER PfxPath
    PFX 证书文件路径。

.PARAMETER PfxPassword
    PFX 密码；建议使用 SecureString 或从环境读取。

.PARAMETER TimestampUrl
    RFC 3161 时间戳服务；默认 DigiCert。
.PARAMETER NoTimestamp
    不请求时间戳。自签名开发证书或离线构建时使用。

.PARAMETER VerifyOnly
    只校验，不签名。

.PARAMETER MachineStore
    使用 LocalMachine 证书存储而不是 CurrentUser。
#>
[CmdletBinding()]
param(
    [string[]]$Path,
    [string]$Thumbprint,
    [string]$PfxPath,
    [string]$PfxPassword,
    [string]$TimestampUrl = "http://timestamp.digicert.com",
    [switch]$NoTimestamp,
    [switch]$VerifyOnly,
    [switch]$MachineStore
)

$ErrorActionPreference = "Stop"

if (-not $Path -or $Path.Count -eq 0) {
    $Path = @(
        (Join-Path $PSScriptRoot "..\target\release\lumark-app.exe"),
        (Join-Path $PSScriptRoot "..\target\release\lumark-sfx.exe")
    )
}

function Find-SignTool {
    $command = Get-Command "signtool.exe" -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path -LiteralPath $kits) {
        $candidate = Get-ChildItem -Path $kits -Directory |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "x64\signtool.exe" } |
            Where-Object { Test-Path -LiteralPath $_ } |
            Select-Object -First 1
        if ($candidate) {
            return $candidate
        }
    }
    throw "signtool.exe was not found. Install the Windows SDK or add signtool to PATH."
}

$signTool = Find-SignTool
$files = @()
foreach ($item in $Path) {
    $resolved = Resolve-Path -LiteralPath $item -ErrorAction SilentlyContinue
    if (-not $resolved) {
        throw "File not found: $item"
    }
    $files += $resolved.ProviderPath
}
if ($files.Count -eq 0) {
    throw "No files to sign."
}

foreach ($file in $files) {
    if ($VerifyOnly) {
        & $signTool verify /pa /all $file
        if ($LASTEXITCODE -ne 0) {
            throw "Signature verification failed: $file"
        }
        continue
    }

    $certificateArgs = @()
    if ($PfxPath) {
        $certificateArgs += @("/f", (Resolve-Path -LiteralPath $PfxPath).ProviderPath)
        if ($PfxPassword) {
            $certificateArgs += @("/p", $PfxPassword)
        }
    } elseif ($Thumbprint) {
        $certificateArgs += @("/sha1", $Thumbprint)
        if ($MachineStore) {
            $certificateArgs += "/sm"
        }
    } else {
        throw "Provide -Thumbprint or -PfxPath, or use -VerifyOnly."
    }

    $signArgs = @("sign", "/fd", "SHA256")
    if (-not $NoTimestamp) {
        $signArgs += @("/tr", $TimestampUrl, "/td", "SHA256")
    }
    $signArgs += $certificateArgs
    $signArgs += $file
    & $signTool @signArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Signing failed: $file"
    }
    & $signTool verify /pa /all $file
    if ($LASTEXITCODE -ne 0) {
        throw "Signature verification failed: $file"
    }
    Write-Host "Signed and verified: $file"
}
