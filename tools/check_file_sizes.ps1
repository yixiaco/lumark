param(
    [int]$WarnLimit = 1000,
    [int]$Limit = 3000
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$roots = @("crates", "vendor", "fuzz") | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path -LiteralPath $_ }
if ($roots.Count -eq 0) {
    throw "No Rust source directories found under $root"
}

if ($WarnLimit -gt $Limit) {
    throw "WarnLimit 不能大于 Limit：$WarnLimit > $Limit"
}

$warnings = @()
$violations = @()
foreach ($file in $roots | ForEach-Object { Get-ChildItem -LiteralPath $_ -Recurse -Filter *.rs -File }) {
    $lines = [System.IO.File]::ReadAllLines($file.FullName).Count
    if ($lines -gt $WarnLimit) {
        $relative = $file.FullName.Substring($root.Length).TrimStart([char]'\')
        $entry = [pscustomobject]@{
            Lines = $lines
            Path = $relative
        }
        $warnings += $entry
        if ($lines -gt $Limit) {
            $violations += $entry
        }
    }
}

if ($warnings.Count -gt 0) {
    Write-Host ("以下 {0} 个 Rust 文件超过提醒阈值 {1} 行（超过 {2} 行才会失败）：" -f $warnings.Count, $WarnLimit, $Limit)
    $warnings | Sort-Object Lines -Descending | Format-Table -AutoSize | Out-String | Write-Host
}

if ($violations.Count -gt 0) {
    Write-Error ("{0} Rust files exceed {1} lines; split them by responsibility first." -f $violations.Count, $Limit)
    exit 1
}

if ($warnings.Count -gt 0) {
    Write-Host ("行数检查通过：{0} 个文件仅触发提醒，未超过失败阈值 {1} 行。" -f $warnings.Count, $Limit)
} else {
    Write-Host ("All Rust files are within {0} lines." -f $WarnLimit)
}
