param(
    [int]$Jobs = 2,
    [int]$TestThreads = 1,
    [switch]$NoFailFast
)

# 低内存全量测试入口：限制 cargo 构建并发和测试线程，并把输出流式写入日志。
# 不要用 `Select-Object -Last 60` 包裹长构建输出，它会缓存全部输出行。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$log = Join-Path $root "target\test-workspace.log"
$arguments = @("test", "--workspace", "-j", $Jobs)
if ($NoFailFast) {
    $arguments += "--no-fail-fast"
}
$arguments += @("--", "--test-threads=$TestThreads")

Write-Host ("运行 cargo {0}；日志：{1}" -f ($arguments -join " "), $log)
Push-Location $root
try {
    & cargo @arguments 2>&1 | Tee-Object -FilePath $log
    $exitCode = $LASTEXITCODE
} finally {
    Pop-Location
}

Write-Host ("测试结束，退出码 {0}；最近输出：" -f $exitCode)
Get-Content -LiteralPath $log -Tail 80
exit $exitCode
