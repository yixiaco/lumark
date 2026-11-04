#requires -Version 5.1
<#
.SYNOPSIS
    清理 target 下失效的增量编译缓存和测试遗留的临时目录。

.DESCRIPTION
    Cargo.toml 只给少数高频 crate 打开 incremental，rustc 按
    target\<profile>\incremental\<crate>-<哈希> 建目录，编译参数一变化就另建一份，
    旧目录不会回收。脚本对每个 crate 只保留最后写入的那份会话缓存，删除其余目录，
    并清理 target 根目录下测试或工具异常退出遗留的 lumark-disguise-*、rustdoctest*
    临时目录（默认只删一天以前的，避免误删正在运行的测试目录）。

    以 target 下 .cargo-lock 是否被占用作为「正在构建」的判据：cargo 只在编译阶段
    持有该锁，跑测试二进制时已释放，因此不会因为有人开着 cargo test 就拒绝清理。
    锁被占用时直接跳过，不删除任何文件；确实没有构建时可用 -Force 跳过该检查。

.PARAMETER OlderThanDays
    临时目录的最后写入时间早于该天数才会删除，默认 1 天。

.PARAMETER DryRun
    只列出待删除项和可回收空间，不实际删除。

.PARAMETER Force
    跳过「构建锁被占用」检查。只有确认没有任何 cargo/rustc 在编译时才使用。

.PARAMETER PurgeIncremental
    删除 incremental 下的全部会话目录，而不只是每个 crate 的陈旧副本。用于编译参数
    刚调整、旧缓存整体失效的场景，代价是下一次构建要重新生成缓存。

.EXAMPLE
    powershell -NoProfile -File tools\prune_target.ps1 -DryRun

.EXAMPLE
    powershell -NoProfile -File tools\prune_target.ps1 -OlderThanDays 7
#>
[CmdletBinding()]
param(
    [int]$OlderThanDays = 1,
    [switch]$DryRun,
    [switch]$Force,
    [switch]$PurgeIncremental
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "target"
if (-not (Test-Path -LiteralPath $target)) {
    throw "找不到 target 目录：$target（先执行一次构建再清理）"
}

# 删除前统一换算成绝对路径，后续只允许删除 target 之下的条目。
$targetFull = [System.IO.Path]::GetFullPath($target).TrimEnd([char]'\')
$separator = [System.IO.Path]::DirectorySeparatorChar

if (-not $Force) {
    $lockedBy = $null
    foreach ($lock in Get-ChildItem -LiteralPath $target -Recurse -Depth 2 -Filter ".cargo-lock" -File -Force -ErrorAction SilentlyContinue) {
        try {
            $handle = [System.IO.File]::Open($lock.FullName, 'Open', 'ReadWrite', 'None')
            $handle.Close()
        } catch {
            $lockedBy = $lock.FullName
            break
        }
    }
    if ($lockedBy) {
        Write-Host ("构建锁被占用：{0}；跳过清理，确认没有构建时加 -Force。" -f $lockedBy)
        exit 0
    }
    $running = Get-Process cargo, rustc -ErrorAction SilentlyContinue
    if ($running) {
        Write-Host ("构建锁空闲，仅检测到 {0} 个 cargo/rustc 进程（可能在运行测试产物），继续清理。" -f $running.Count)
    }
}

function Measure-DirectoryBytes {
    param([string]$Path)
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) {
        return [int64]0
    }
    return [int64]$sum
}

function Format-Size {
    param([int64]$Bytes)
    if ($Bytes -ge 1GB) {
        return ("{0:N2} GB" -f ($Bytes / 1GB))
    }
    if ($Bytes -ge 1MB) {
        return ("{0:N1} MB" -f ($Bytes / 1MB))
    }
    return ("{0:N0} KB" -f ($Bytes / 1KB))
}

# 删除前再次确认目标确实位于 target 之下，避免路径计算出错时误删仓库其它内容。
function Remove-TargetEntry {
    param([string]$Path)
    $full = [System.IO.Path]::GetFullPath($Path).TrimEnd([char]'\')
    if (-not $full.StartsWith($targetFull + $separator, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝删除 target 之外的路径：$full"
    }
    Remove-Item -LiteralPath $full -Recurse -Force -ErrorAction SilentlyContinue
}

$staleEntries = New-Object System.Collections.ArrayList
$freedBytes = [int64]0

foreach ($incrementalDir in Get-ChildItem -LiteralPath $target -Recurse -Depth 3 -Directory -Filter "incremental" -ErrorAction SilentlyContinue) {
    $sessions = Get-ChildItem -LiteralPath $incrementalDir.FullName -Directory -Force -ErrorAction SilentlyContinue
    if (-not $sessions) {
        continue
    }
    if ($PurgeIncremental) {
        foreach ($session in $sessions) {
            [void]$staleEntries.Add($session)
            $freedBytes += Measure-DirectoryBytes $session.FullName
        }
        continue
    }
    # 会话目录名是 <crate>-<哈希>，哈希段不含连字符，按最后一段切出 crate 名分组。
    $groups = $sessions | Group-Object { ($_.Name -replace '-[^-]+$', '') }
    foreach ($group in $groups) {
        $stale = $group.Group | Sort-Object LastWriteTime -Descending | Select-Object -Skip 1
        foreach ($session in $stale) {
            [void]$staleEntries.Add($session)
            $freedBytes += Measure-DirectoryBytes $session.FullName
        }
    }
}

$cutoff = (Get-Date).AddDays(-$OlderThanDays)
$junkEntries = New-Object System.Collections.ArrayList
$junkBytes = [int64]0
foreach ($item in Get-ChildItem -LiteralPath $target -Directory -Force -ErrorAction SilentlyContinue) {
    $isJunk = $item.Name -like "lumark-disguise-*" -or $item.Name -like "rustdoctest*"
    if (-not $isJunk -or $item.LastWriteTime -ge $cutoff) {
        continue
    }
    [void]$junkEntries.Add($item)
    $junkBytes += Measure-DirectoryBytes $item.FullName
}

if ($staleEntries.Count -eq 0 -and $junkEntries.Count -eq 0) {
    Write-Host "没有需要清理的增量缓存或临时目录。"
    exit 0
}

if ($DryRun) {
    foreach ($entry in $staleEntries) {
        Write-Host ("将删除增量缓存：{0}" -f $entry.FullName)
    }
    foreach ($entry in $junkEntries) {
        Write-Host ("将删除临时目录：{0}" -f $entry.FullName)
    }
    Write-Host ("试运行：可回收约 {0}，其中增量缓存 {1} 个目录、临时目录 {2} 个；去掉 -DryRun 即可执行删除。" -f `
        (Format-Size ($freedBytes + $junkBytes)), $staleEntries.Count, $junkEntries.Count)
    exit 0
}

foreach ($entry in $staleEntries) {
    Remove-TargetEntry -Path $entry.FullName
}
foreach ($entry in $junkEntries) {
    Remove-TargetEntry -Path $entry.FullName
}

Write-Host ("已清理 {0} 个增量缓存目录、{1} 个临时目录，回收约 {2}。" -f `
    $staleEntries.Count, $junkEntries.Count, (Format-Size ($freedBytes + $junkBytes)))
