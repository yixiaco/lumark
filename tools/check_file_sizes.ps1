param(
    [int]$Limit = 1000
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$crates = Join-Path $root "crates"
if (-not (Test-Path -LiteralPath $crates)) {
    throw "crates directory not found: $crates"
}

$violations = @()
foreach ($file in Get-ChildItem -LiteralPath $crates -Recurse -Filter *.rs -File) {
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
