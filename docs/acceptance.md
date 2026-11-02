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
| 8 | GitHub CI 修复 | 🟡 | 本地 `fmt/clippy/check/test` 全绿；`master` 领先 origin 待推送，GitHub Actions 尚未验证（P-5 等待用户确认） |

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
cargo test --workspace --lib
```

关键测试数（`cargo test --workspace --lib`，2026-11-02）：

| crate | 通过数 |
|---|---|
| lumark-archive | 61 |
| lumark-browser | 14 |
| lumark-export | 25 |
| lumark-legacy | 54 |
| lumark-mail | 4 |
| lumark-media | 4 |
| lumark-platform-windows | 21 |
| lumark-render | 31 |
| lumark-scene | 11 |
| lumark-settings | 7 |
| lumark-sfx | 7 |
| lumark-tasks | 8（另有集成测试） |
| lumark-ui | 176 |
| lumark-unreal | 74 |
| lumark-vault | 11 |
| lumark-viewer | 17 |
| zpaq-rs | 10 |

合计 **535 个库测试通过**；另有各 crate 集成/文档测试与示例诊断。

### 实机样本

- BNB（IoStore，92,368 条目，Oodle）：`SK_Chicken`、`ANIM_Chicken_Idle2`、`SM_Box_02`、`MI_Props`；
- FF7R（PAK，18,896 条目，Oodle/Zlib/Gzip）：Oodle 预检 + 资产读取；
- 导出：GLB/PSK/PSA/UEMODEL/USDA 结构校验 9/9 PASS；HDR 侧车；
- 渲染：GPU 900×600 2×SS 约 70–95 ms（V-4/V-5），CPU 回退可用。

## 四、已知限制

1. **P-5**：当前分支领先 origin 55 个提交，CI 未在 GitHub 验证；
2. **V-7**：GPU 实例化未实现（当前按材质合并 draw batch，非 instancing）；
3. **V-14**：世界场景 Provider 依赖 Zen 导入表/组件属性，尚未暴露；
4. **V-16/V-18/V-19**：异步帧泵、骨骼搜索、快捷键与动画 Slider 未完成；
5. **O-4**：随包发布流水线/首启检测仅完成查找路径；
6. **媒体**：按帧抽图 + ffplay 音频，非硬解音画同步播放器；归档内媒体依赖临时解压；
7. **USDA**：未在 UE/Blender/USD Composer 实际导入，只做结构/gltf crate 等外部校验；
8. **Oodle**：默认下载源未内置，需用户提供授权 zip URL/SHA-256。

## 五、发布流程（A-5）

1. 用户确认后推送 `master`（P-5）；
2. 观察 GitHub Actions；修复 CI 平台差异；
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
