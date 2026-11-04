# Lumark 验收与发布说明（A 阶段）

> 更新：2026-11-02。目标：对照原始 8 项需求与《分步执行计划 v4》给出可复核证据。

## 一、原始 8 项需求验收

| # | 需求 | 状态 | 证据 |
|---|---|---|---|
| 1 | 3D 资产查看器（GPU/材质/法线/分辨率/独显/引擎无关场景/UI） | ✅（功能级） | `lumark-render` wgpu 30 + CPU 回退；PBR/ORM/法线/自发光、蒙皮、着色模式、网格/骨骼、检查器；资产查看器窗口 GPU 优先并显示后端；BNB `SK_Chicken`/`SM_Box_02` 实机；`docs/viewer-architecture.md`。缺口：V-7 实例化、V-14 世界 Provider、V-16 异步帧泵、V-18 骨骼搜索、V-19 快捷键/Slider |
| 2 | 模型导出 PSK/PSKX、glTF binary、UEFormat uemodel、USDA + 自动材质 | ✅ | `lumark-export` 五种写出器；PSK/PSA/GLB/UEMODEL/USDA 结构校验全过；BNB 9 文件 PASS；`docs/export-formats.md`、`docs/export-validation.md` |
| 3 | 贴图导出 PNG/JPEG/TGA/WebP | ✅ | `TextureExportFormat` + `encode_texture`；BNB `MI_Props` 四格式侧车；E-3 |
| 4 | HDR `.hdr` 导出 | ✅ | Radiance RGBE 编码；512×512、1,048,625 字节实机图；E-3 HDR |
| 5 | 整包批量导出 + 每游戏导出配置 | ✅ | `export_archive_cancellable`、任务中心（取消/进度/报告 JSON）、批量/单资产导出窗口；`GameExportConfig` + 游戏编辑导出区；E-12/13/14/15/E-1/E-2 |
| 6 | 图片查看器滚轮缩放/平移/图标修复 | ✅ | 光标锚点、拖拽、EXIF/SVG、IconPark `zoom-in/zoom-out`；模块拆分；`docs/image-viewer.md`；I 阶段 5/5 |
| 7 | 媒体播放器（ffmpeg） | ✅ | `lumark-media` ffmpeg/ffprobe 探测/抽帧、ffplay 音频；播放窗口 + 归档内媒体；`docs/media-player.md`；M 阶段 8/8 |
| 8 | GitHub CI 修复 | ✅ | 已推送 `master`；GitHub Actions run [41588158621](https://github.com/yixiaco/lumark/actions/runs/41588158621) success：Format/File sizes/Clippy/Test 与 Build/package/artifact 全通过 |

## 二、构建与运行

```powershell
cargo run -p lumark-app
cargo build --release --locked -p lumark-app -p lumark-sfx
./tools/build_release.ps1 -Version 0.1.0
```

要求 Rust stable（工作区 MSRV 1.96），Windows x64 + MSVC。媒体播放需要 PATH/`LUMARK_FFMPEG` 中有 `ffmpeg`/`ffprobe`（音频另需 `ffplay`）；游戏资产需要 Oodle（内置 Rust 后端或自备 DLL）与 `.usmap`。

## 三、测试与性能证据

### 测试命令

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets --locked
powershell -NoProfile -ExecutionPolicy Bypass -File tools/test_workspace.ps1
```

关键测试（`powershell -NoProfile -ExecutionPolicy Bypass -File tools/test_workspace.ps1`，2026-11-04）：

- **74 个测试目标、680 个测试通过、0 失败**；
- 其中 `lumark-ui` 187、`lumark-unreal` 93、`lumark-render` 43（CPU + GPU，无 GPU 时自动跳过）、`lumark-archive` 192（含集成测试）；
- 另有示例诊断脚本不参与计数；默认构建并发已限制为 4，低内存验证使用 `-j 2 --test-threads=1`；开发/测试配置只保留工作区代码的行号表，第三方依赖不生成完整 debug 信息，避免 MSVC 链接阶段占用十余 GiB；
- 构建内存实测：`link.exe` 峰值从 5.48 GiB 降到 2.18 GiB，构建总工作集从 8.32 GiB 降到 2.72 GiB；`lumark-ui` 测试 PDB 从 1.0 GiB 降到 210 MiB，分页文件不再持续增长。

### 实机样本

- BNB（IoStore，92,368 条目，Oodle）：`SK_Chicken`、`ANIM_Chicken_Idle2`、`SM_Box_02`、`MI_Props`；
- FF7R（PAK，18,896 条目，Oodle/Zlib/Gzip）：Oodle 预检 + 资产读取；
- 导出：GLB/PSK/PSA/UEMODEL/USDA 结构校验 9/9 PASS；HDR 侧车；
- 渲染：GPU 900×600 2×SS 约 70–95 ms（V-4/V-5），CPU 回退可用。

## 四、支持范围与验证边界

1. **世界场景**：Provider 支持 `.umap`（`load_scene` → `load_world_scene`），StaticMeshActor（含蓝图子类）按跨包 StaticMesh + 槽位材质解析加载；实测 BNB `GinkgoTree.umap` 178,600 顶点 / 105,348 三角形 / 16 材质。SkeletalMesh/Landscape Actor 不属于静态世界网格，由各自查看器呈现。
2. **媒体播放**：按帧抽图 + `ffplay` 音频（设计选择），归档内媒体先解压到临时目录再播放。
3. **模型导出验证**：GLB 用官方 `gltf` crate 完整解析；PSK/PSA/UEMODEL/USDA 用独立布局解析器交叉校验；UE/Blender/USD Composer 导入作为可选的额外复验步骤。
4. **Oodle 分发**：随包提供纯 Rust 后端；Oodle DLL 由用户自备授权文件（`LUMARK_OODLE_DLL`/`-OodleDll`），官方 zip 下载需提供 URL 与 SHA-256。
5. **签名**：发布流程支持 `-UseDevCert` 开发签名；未配置正式证书时产物未签名并以 warning 提示。

## 五、发布流程（A-5）

1. `master` 已推送（P-5）；
2. GitHub Actions run [41588158621](https://github.com/yixiaco/lumark/actions/runs/41588158621) 已通过（Format/File sizes/Clippy/Test/Build/package/artifact）；
3. `tools/build_release.ps1 -Version <v> -UseDevCert` 生成安装器/便携包/SHA256；
4. 复核 `docs/lgpl-compliance.md`、`licenses/` 与 IconPark 许可；
5. 打 tag、上传 Release、更新变更说明。

## 六、文档索引

- [计划进度](plan-progress.md)
- [导出格式](export-formats.md) / [导出校验](export-validation.md)
- [查看器架构](viewer-architecture.md) / [图片查看器](image-viewer.md)
- [游戏资产](game-assets.md) / [Oodle 后端](oodle-backends.md)
- [媒体播放器](media-player.md) / [LGPL 合规](lgpl-compliance.md)
- `IMPLEMENTATION_STATUS.md`（逐项实现记录）
