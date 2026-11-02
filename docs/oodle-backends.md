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

## 运行时下载与缓存（O-3）

```rust
use lumark_archive::{OodleDownloadOptions, download_and_cache_oodle};

let result = download_and_cache_oodle(
    &OodleDownloadOptions {
        url: "https://.../oodle.zip".to_string(),
        cache_dir: cache_dir.clone(),
        expected_sha256: Some("...".to_string()),
        preferred_library: Some("oo2core_9_win64.dll".to_string()),
    },
    |downloaded, total| { /* 进度 */ },
)?;
lumark_archive::configure_oodle_library(Some(result.library.clone()))?;
```

- 下载到 `cache_dir/oodle.zip`（临时 `.part`，完成后原子改名）；
- 可选 SHA-256 校验，不匹配不会留下缓存/库；
- zip 解压只取文件名，拒绝 `../` 目录穿越；
- 只接受 `oo2core*.dll` / `oodle*.dll`；
- 已缓存且校验通过时不再联网；
- `install_oodle_zip(zip_bytes, cache_dir, sha, preferred)` 用于离线导入。

离线测试（4 个）：安装/复用、SHA 不匹配、路径穿越、非 Oodle DLL 拒绝。

## 随包内置（O-4）

DLL 查找顺序：

1. `configure_oodle_library(Some(path))` 显式路径；
2. `LUMARK_OODLE_DLL`；
3. 可执行文件同目录；
4. `oodle/`；
5. `resources/oodle/`。

发布时把已获授权的 `oo2core*.dll` 放到上述目录即可被自动发现；Lumark 仓库不内置二进制。

## 设置页（O-5）

游戏设置 → Oodle 区显示：

- 当前选择或自动发现的库路径；
- 「当前后端」摘要（配置模式、Rust/DLL 可用性、是否已加载）；
- 「选择 Oodle 库…」按钮。

下载/安装向导尚未接入设置页。

## 诊断与文档

```powershell
cargo run -q -p lumark-unreal --example oodle_preflight -- "D:\Games\BNB" "pakchunk0-Windows.utoc"
```

输出容器条目数、压缩方法、是否需要 Oodle、当前后端是否可解压。

## 实机结果（2026-11-02）

### BNB（IoStore）

```text
kind=IoStore entries=92368 methods=["Oodle"] needed=true ready=true
summary=需要 Oodle（可解压）；后端：Rust（配置 auto；Rust 可用；DLL 未加载）
read=Engine/Content/Animation/DefaultAnimBoneCompressionSettings.uasset bytes=2297
```

### FINAL FANTASY VII REMAKE（UE4 PAK）

```text
kind=Pak entries=18896 methods=["None", "Zlib", "Gzip", "Oodle"] needed=true ready=true
summary=需要 Oodle（可解压）；后端：Rust（配置 auto；Rust 可用；DLL 未加载）
read=End/Content/GameContents/Animation/BonamikWind/LV0_BonamikWind.uasset bytes=700
```

两个容器都能通过内置 Rust 后端完成真实资产读取。

诊断命令：

```powershell
cargo run -q -p lumark-unreal --example oodle_preflight -- "D:\Games\BNB" "pakchunk0-Windows.utoc"
cargo run -q -p lumark-unreal --example oodle_preflight -- "D:\Games\FINAL FANTASY VII REMAKE" "pakchunk0-WindowsNoEditor.pak"
```

## 限制

- 默认下载源未内置；调用方必须提供授权 zip 的 URL 与 SHA-256；
- O-4 仅提供查找路径，仓库不随包分发 Oodle 二进制；
- O-7 需在 FF7R 等更多容器上验收。
