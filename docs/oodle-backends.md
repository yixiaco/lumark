# Oodle 后端与预检（O-1 / O-2 / O-6）

> 更新：2026-11-02。实现位置：`crates/lumark-archive/src/pak/oodle.rs`、
> `crates/lumark-unreal/src/source.rs`。

## 后端

| 后端 | 说明 |
|---|---|
| `rust` | 内置 `oozextract` 纯 Rust 解码器，Windows/Linux/macOS 可用 |
| `dll` | Windows 动态库 `OodleLZ_Decompress`；按显式配置 → `LUMARK_OODLE_DLL` → 可执行文件同目录查找 |
| `auto` | 默认：先 Rust，失败回退 DLL |

环境变量：

```powershell
$env:LUMARK_OODLE_BACKEND = "auto"   # auto | rust | dll
$env:LUMARK_OODLE_DLL = "D:\...\oo2core_9_win64.dll"
```

也可通过 `configure_oodle_library(Some(path))` 显式指定 DLL；配置失败只记录告警，不影响 Rust 后端。

## 状态 API（O-1）

```rust
use lumark_archive::{OodleStatus, oodle_status};

let status: OodleStatus = oodle_status();
status.setting;        // auto/rust/dll
status.rust_available; // 当前始终 true
status.dll_supported;  // Windows 为 true
status.dll_configured; // 显式/环境配置路径
status.dll_loaded;     // DLL 是否已成功加载
status.usable();       // 当前配置下是否有可用后端
status.summary();      // 界面展示文本
```

## 容器预检（O-2）

```rust
let preflight = source.oodle_preflight();
preflight.needed;   // 容器压缩方法是否包含 Oodle
preflight.ready;    // 当前后端能否解压
preflight.methods;  // 容器声明的压缩方法
preflight.summary();// 界面展示文本
```

`GameAssetSource::needs_oodle()` / `compression_methods()` 也可单独使用；IoStore 与 PAK 都支持。

## 实机结果

BNB（`pakchunk0-Windows.utoc`，IoStore）：

```text
kind=IoStore entries=92368 methods=["Oodle"] needed=true ready=true
summary=需要 Oodle（可解压）；后端：Rust（配置 auto；Rust 可用；DLL 未加载）
```

诊断命令：

```powershell
cargo run -q -p lumark-unreal --example oodle_preflight -- "D:\Games\BNB" "pakchunk0-Windows.utoc"
```

## 限制

- O-3 运行时下载/缓存、O-4 随包内置尚未实现；目前依赖内置 Rust 解码器或用户自备 DLL；
- O-5 设置页已显示当前后端摘要，但尚无“下载/安装 Oodle”向导；
- O-7 需在 FF7R 等更多容器上验收。
