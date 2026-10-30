# 游戏资产验收脚本
#
# 基础测试总会执行；提供完整环境变量后额外用真实游戏做端到端验收：
#   $env:LUMARK_GAME_ROOT     游戏根目录，例如 D:\Games\BNB
#   $env:LUMARK_GAME_MAPPING  .usmap 路径
#   $env:LUMARK_GAME_OODLE    Oodle DLL 路径
#   $env:LUMARK_GAME_CONTAINER 容器文件名，例如 pakchunk0-Windows.utoc
#   $env:LUMARK_GAME_ENTRY    资产路径
#   $env:LUMARK_GAME_OUTPUT   输出 PNG 路径
#
# 示例：
#   .\tools\verify_game_assets.ps1
#   .\tools\verify_game_assets.ps1 -GameRoot "D:\Games\BNB" -Mapping "D:\bnb.usmap" ...

param(
    [string]$GameRoot = $env:LUMARK_GAME_ROOT,
    [string]$Mapping = $env:LUMARK_GAME_MAPPING,
    [string]$Oodle = $env:LUMARK_GAME_OODLE,
    [string]$Container = $env:LUMARK_GAME_CONTAINER,
    [string]$Entry = $env:LUMARK_GAME_ENTRY,
    [string]$Output = $env:LUMARK_GAME_OUTPUT
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host "== IoStore 测试 =="
    cargo test -p lumark-archive --test iostore
    if ($LASTEXITCODE -ne 0) { throw "IoStore 测试失败" }

    Write-Host "== UE 解析测试 =="
    cargo test -p lumark-unreal
    if ($LASTEXITCODE -ne 0) { throw "UE 解析测试失败" }

    Write-Host "== 游戏资产 UI 测试 =="
    cargo test -p lumark-ui game_assets
    if ($LASTEXITCODE -ne 0) { throw "游戏资产 UI 测试失败" }

    $realInputs = @($GameRoot, $Mapping, $Oodle, $Container, $Entry, $Output) |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    if ($realInputs.Count -lt 6) {
        Write-Host "未提供完整真实游戏参数，跳过端到端验收。"
        Write-Host "需要的环境变量：LUMARK_GAME_ROOT / MAPPING / OODLE / CONTAINER / ENTRY / OUTPUT"
        exit 0
    }

    foreach ($path in @($GameRoot, $Mapping, $Oodle)) {
        if (-not (Test-Path -LiteralPath $path)) { throw "路径不存在：$path" }
    }
    Write-Host "== 真实游戏端到端验收（不打印密钥） =="
    cargo run -p lumark-unreal --example accept_game -- `
        $GameRoot $Mapping $Oodle $Container $Entry $Output
    if ($LASTEXITCODE -ne 0) { throw "端到端验收失败" }
    Write-Host "验收输出：$Output"
}
finally {
    Pop-Location
}
