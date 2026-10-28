param(
    [int]$Limit = 1000
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$roots = @("crates", "vendor", "fuzz") | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path -LiteralPath $_ }
if ($roots.Count -eq 0) {
    throw "No Rust source directories found under $root"
}

$violations = @()
foreach ($file in $roots | ForEach-Object { Get-ChildItem -LiteralPath $_ -Recurse -Filter *.rs -File }) {
    $lines = [System.IO.File]::ReadAllLines($file.FullName).Count
    if ($lines -gt $Limit) {
        $relative = $file.FullName.Substring($root.Length).TrimStart([char]'\')
        $violations += [pscustomobject]@{
            Lines = $lines
            Path = $relative
        }
    }
}

if ($violations.Count -gt 0) {
    $violations | Sort-Object Lines -Descending | Format-Table -AutoSize | Out-String | Write-Host
    Write-Error ("{0} Rust files exceed {1} lines; split them by responsibility first." -f $violations.Count, $Limit)
    exit 1
}

Write-Host ("All Rust files are within {0} lines." -f $Limit)
