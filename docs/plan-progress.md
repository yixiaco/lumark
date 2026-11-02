# Lumark 计划进度（对照《分步执行计划 v4》）

> 更新：2026-11-02（目标模式第 20 轮结束）
> 说明：状态只有三种：`✅ 完成`、`🟡 部分完成`、`⬜ 未开始`；「证据」尽量给出文件、测试数或实机输出。

## 总览

| 阶段 | 完成度 | 当前状态 |
|---|---|---|
| P（CI 与基础设施） | 4/5 | P-5 推送/CI 验证待用户确认 |
| O（Oodle FModel 式） | 6/7 | O-1/O-2/O-3/O-5/O-6/O-7 完成；O-4 部分（查找路径完成，随包发布流水线待做） |
| S（资产底座） | 6/9 | S-2/S-6 待补，其余已完成 |
| V（查看器） | 9/21 | V-2/3/6/13/16/18/19 部分；V-7/8/14/20 待做 |
| E（导出） | 15/18 | E-0/E-1/E-2 待补或部分；其余完成 |
| M（媒体播放器） | 5/8 | 探测/抽帧/播放窗口/控制/文档完成；音频与归档内媒体待做 |
| I（图片查看器） | 5/5 | 全部完成（拆分/缩放锚点/拖拽/图标/文档） |
| A（验收/文档/发布） | 0/6 | 待 V/E/M/I 收口 |

## P：CI 与基础设施

| 步骤 | 状态 | 证据 |
|---|---|---|
| P-1 行数门禁放宽 | ✅ | `tools/check_file_sizes.ps1`；警告 1000/失败 3000（按用户最新要求） |
| P-2 zpaq CRT 修复 | ✅ | `vendor/zpaq_rs/build.rs`；release 链接通过 |
| P-3 发布脚本清理 CC/CXX | ✅ | `tools/build_release.ps1` |
| P-4 Workflow 收尾 | ✅ | `.github/workflows/windows-build.yml` |
| P-5 推送并观察 CI | ⬜ | 本地 `master` 领先 origin 22+ 提交，未推送 |

## O：Oodle（FModel 式）

| 步骤 | 状态 | 备注 |
|---|---|---|
| O-1 加载器状态 API | ✅ | 第 38 轮：`OodleStatus{setting,rust_available,dll_supported,dll_configured,dll_loaded}` + `usable()/summary()` + `oodle_status()`；2 测试；游戏设置页显示当前后端摘要 |
| O-2 容器 needs_oodle 预检 | ✅ | 第 38 轮：`IoStoreContainer::needs_oodle`、`GameAssetSource::oodle_preflight()`（`OodlePreflight{needed,ready,methods,status,summary}`）；BNB 实机 92368 条目、methods=["Oodle"]、needed=true、ready=true（Rust 后端） |
| O-3 运行时下载与缓存 | ✅ | 第 39 轮：`oodle_download`：reqwest 流式下载 + 进度 → SHA-256 校验 → 原子缓存 `oodle.zip` → zip 安全解压 `oo2core*/oodle*.dll` → 缓存复用；`install_oodle_zip` 离线安装；4 测试（安装/复用、SHA、路径穿越、非法 DLL） |
| O-4 随包内置 | 🟡 | 第 39 轮：DLL 查找新增 `oodle/`、`resources/oodle/`（exe 同目录已支持）；仓库不内置二进制，发布需放置已授权 DLL |
| O-5 UI 集成与设置 | ✅ | 第 38/41 轮：游戏设置 Oodle 区显示“当前后端”摘要；新增「下载 Oodle…」入口与 `oodle_download_window`（URL/SHA/缓存目录、进度百分比与字节、下载后自动解压并配置、失败原因）；2 UI 测试 |
| O-6 诊断与文档 | ✅ | 第 38/39 轮：`docs/oodle-backends.md` 补充下载/缓存 API、随包目录、设置页、诊断；`oodle_preflight` 示例；BNB 实机 |
| O-7 真实验收 | ✅ | 第 40 轮：BNB IoStore（92,368 条目/Oodle/读取 2,297B 资产）与 FF7R PAK（18,896 条目/None+Zlib+Gzip+Oodle/读取 700B 资产）均 `needed=true ready=true`，Rust 后端真实解码成功 |

## S：共享资产底座

| 步骤 | 状态 | 证据 |
|---|---|---|
| S-1 GameAssetSource | ✅ | `crates/lumark-unreal/src/source.rs` |
| S-2 PAK 按条目读 API | 🟡 | 现有预览/导出可读 PAK；独立 `PakAssetReader` 未抽 |
| S-3 预览读取下沉 | ✅ | `preview_ue.rs` 分模块、材质解析在 `materials.rs` |
| S-4 Zen 网格法线/切线/UV/材质槽 | ✅ | `package/mesh.rs`；BNB 回归 |
| S-5 旧式网格 UV/法线 | ✅ | `legacy_skeleton.rs` |
| S-6 HDR/法线纹理解码 | ⬜ | BC5 法线 Z 重建已有；浮点 HDR 未做 |
| S-7 材质槽解析 | ✅ | `materials.rs`；BNB/FF7R 实机 |
| S-8 动画采样接口 | ✅ | `skinning.rs` |
| S-9 .umap 解析 | ✅ | `package/world.rs`；BNB 地图 |

## V：查看器

| 步骤 | 状态 | 证据/缺口 |
|---|---|---|
| V-0 GPU 预研 | ✅ | `lumark-render` wgpu 30；RTX 5070 Vulkan 实测 |
| V-1 lumark-scene | ✅ | Scene/Mesh/Material/Inspector/Transform；11 测试 |
| V-2 渲染核心 | 🟡 | CPU 光栅 + GPU 离屏渲染完成；渲染线程/异步双 staging/device lost 未做 |
| V-3 相机/网格/清屏 | 🟡 | 轨道相机、清屏、超采样完成；网格地板/背景渐变未做 |
| V-4 PBR 网格 + 纹理 | ✅（核心） | 本轮：顶点切线、基础色/法线/ORM/自发光贴图、alpha 裁剪、单/双面剔除；CPU 18 测试 + GPU 2 测试全过；MI_Props 实机截图 `target/render-box-gpu-v4.png`。缺口：CPU mips/纹理 LRU/sRGB 视图切换未做 |
| V-5 GPU 蒙皮 + 动画 | ✅ | 第 21 轮：`MeshSkin`（关节/inverse bind/pose）+ CPU/GPU LBS + GPU storage buffer（≤4 影响归一化）；`apply_animation` 接入 `sample_animation`；BNB `SK_Chicken + ANIM_Chicken_Idle2` 42 轨道 t=0/1s 两帧渲染成功（~87–93 ms，PNG hash 不同）；`lumark-render` 25 测试全过（CPU/GPU 蒙皮平移） |
| V-6 叠加层/着色模式 | 🟡 | 第 22 轮：`ShadingMode{Lit,Unlit,Normals}`、地面网格、骨骼线段（CPU DDA + GPU LineList 管线）、`hidden_material_slots`；CPU+GPU 测试。缺口：wireframe 与 section 逐项 UI |
| V-7 实例化/大规模场景 | ⬜ | |
| V-8 质量/统计/设备丢失 | ⬜ | |
| V-9 渲染器测试/文档 | ✅ | 第 22 轮：30 个测试 + `docs/viewer-architecture.md`（分层/API/管线/模式/基线/限制） |
| V-10 UE 静态网格 Provider | ✅ | 第 23 轮：`provider` 模块 `SceneRequest`/`LodPolicy`/`SceneLoadOptions`/`load_scene`/`SceneLoadStage`/`SceneLoadReport`；StaticMesh/SkeletalMesh 按 LOD 选层、材质解析、warning 汇总；BNB `SM_Box_02`（3 LOD）与 `SK_Chicken`（2 LOD）实机加载 |
| V-11 UE 材质到 Scene | 🟡 | 总贴图缓存 `TextureCache` + `MaterialResolveOptions.texture_cache`、材质缺失/贴图缺失 warning；BNB 样本缺材质时输出可读 warning；HDR 标记待 S-6 |
| V-12 骨骼网格/动画 Provider | ✅ | 第 24 轮：`SceneLoadOptions.animation/animation_time`、`SceneLoadStage::Animation`、`AnimationInfo`（名称/时长/帧数/轨道/采样时间）、`LoadedScene.skeleton`；BNB `SK_Chicken + ANIM_Chicken_Idle2` t=0.5s Provider 实机加载（42 骨骼/42 轨道/141 帧/4.667s）；时间轴 UI 待 V-19 |
| V-13 独立场景 | 🟡 | 查看器可开 Mesh/Skeleton/SkeletalMaterial；入口/动画待完善 |
| V-14 世界场景 Provider | ⬜ | 依赖 Zen 导入表/组件属性：`WorldSummary` 仅有 Actor 清单，尚无 `StaticMeshComponent` 外部网格引用与 Transform，需先暴露 Zen import map |
| V-15 UI 模块拆分 | ✅ | 第 26 轮：`asset_viewer_window/` 拆为 `mod/animation/app/body/render/status/toolbar/tests` 8 个文件，最大源码模块 242 行（toolbar）、测试 434 行，均 <600；行为不变，13 个查看器测试与 162 个 UI 总测试全过 |
| V-16 渲染桥接/帧泵 | 🟡 | 第 25/29 轮：资产查看器 `image_path` 临时 PNG 改为 `Arc<RenderImage>`；网格/骨骼网格预览改走 `render_service`（GPU 优先）并在状态栏显示后端；异步帧泵与详情面板 RenderImage 直连待收尾 |
| V-17 UI 外壳/工具栏 | ✅ | 第 29 轮：预览接入 `lumark_render` + `render_service`；着色循环（光照/无光/法线）、地面网格、骨骼线开关、检查器开关、缩放 `−/+`；网格/骨骼网格行为与 V-6 RenderSettings 打通 |
| V-18 检查器 | 🟡 | 第 27 轮：新增 `inspector.rs` 面板：概要（类型/LOD/顶点/三角形/section/骨骼/动画）、LOD 点击切换、section 显隐、骨骼列表（≤200 + 总数）、动画时间/播放状态、材质参数与贴图状态、关闭按钮；缺口：骨骼搜索、世界统计、section 材质名 |
| V-19 输入/动画/快捷键 | 🟡 | 第 29 轮：画布滚轮缩放（0.2–6×，指数步进）；缺口：F 重置、Space/←→ 帧步进、L/G/B/N 快捷键、动画 Slider |
| V-20 状态/入口/性能验收 | ⬜ | |

## E：导出

| 步骤 | 状态 | 证据/缺口 |
|---|---|---|
| E-0 DTO/公共基础 | 🟡 | `ExportMesh/ExportBone/ExportOptions/validate`；原子写/Report 未抽 |
| E-1 GameExportConfig（settings） | 🟡 | `BatchExportConfig` JSON 在 unreal；settings 版本化未接 |
| E-2 游戏编辑窗口导出 UI | ⬜ | |
| E-3 贴图写入器（PNG/JPEG/TGA/WebP/HDR） | ✅ | 第 30/42 轮：`TextureExportFormat{Png,Jpeg,Tga,Webp,Hdr}` + `encode_texture`；PNG/JPEG/WebP 用 image 编码，TGA 手写 18 字节头 + BGRA，HDR 写 Radiance RGBE；4 测试 + BNB `MI_Props` 五格式实机侧车（HDR 512×512、1,048,625B） |
| E-4 材质旁挂 | ✅ | 第 30 轮：`material_sidecars` 生成 `textures/<材质>_BaseColor|Normal.<ext>` + `materials.json`（版本/格式/基础色/金属度/粗糙度/自发光/双面/贴图路径），`write_resources` 防路径穿越；3 测试 + BNB 实机 3 文件 × 4 格式 |
| E-5 glTF 2.0 GLB | ✅ | 4 测试 + BNB 实机 + gltf crate 校验 |
| E-6 ActorX PSK | ✅ | 3 测试 + BNB 实机 + chunk 校验 |
| E-7 ActorX PSA | ✅ | 第 31 轮：`PsaExport`（帧/骨骼/关键值）+ `write_psa`：ANIMHEAD 20100422、BONENAMES 120B、ANIMINFO 168B、ANIMKEYS 32B、SCALEKEYS 16B；Y 镜像与根骨骼 quat W 取反；2 测试 + BNB `SK_Chicken + ANIM_Chicken_Idle2` 实机 289,624 字节（141 帧 ×42 骨骼，fps 30.214） |
| E-8 UEFormat 公共写入器 | ✅ | 含在 E-9 |
| E-9 UEFormat UEMODEL | ✅ | 3 测试 + BNB 实机 |
| E-10 USDA | ✅ | 第 43/44 轮：网格/材质/贴图/GeomSubset + USD Skel（SkelRoot/Skeleton/joints/bind·rest 矩阵/SkelBindingAPI 蒙皮 primvars）+ **SkelAnimation**（`write_usda_with_animation`：translations/rotations/scales timeSamples、joints、`skel:animationSource`；四元数转 USD wxyz）；6 单测 + BNB `SK_Chicken + ANIM_Idle2` 实机 982,223 字节/141 帧/42 骨骼，`validate_exports` 通过 |
| E-11 单资产编排 | ✅ | 第 33 轮：`asset_export` 模块 `AssetExportConfig`（输出/格式/缩放/旁挂/贴图格式/质量/覆盖）+ `export_asset`（解析→多格式写盘→材质旁挂→报告）+ `AssetExportProgress` + `AssetExportReport` JSON；BNB `SM_Box_02` 实机四格式 + materials.json（0.11s） |
| E-12 批量枚举/执行 | ✅ | 后端完成（5 测试 + BNB 四格式）；取消、报告 JSON、耗时已实现 |
| E-13 批量任务接入 | ✅ | 第 32/36 轮：`export_archive_cancellable` + 取消/报告 JSON；`lumark-tasks` 新增 `TaskKind::Export`、`TaskRequest::Custom`、`TaskReporter`（自定义任务上报 `TaskProgress`）与自定义任务单测；批量导出窗口提交到 `TaskManager`，任务中心显示进度/取消，完成后写 `batch-report.json` |
| E-14 单资产导出 UI | ✅ | 第 34 轮：详情面板 `.uasset` 新增“导出资产…”入口；独立导出窗口 `export_window.rs`：格式多选（GLB/UEMODEL/USDA/PSK）、材质旁挂开关、贴图格式四选、后台导出、取消、结果统计与“打开输出目录”；默认输出 `桌面/LumarkExports`（暂不可自定义目录）。UI 2 测试（切换/渲染），`lumark-ui` 166 个全过 |
| E-15 批量导出 UI | ✅ | 第 35/36 轮：动作带“批量导出”入口；`batch_export_window/`：格式多选、材质/覆盖开关、数量限制（全部/10/50/100/500）、提交 `TaskManager` 后台任务、任务中心进度/取消、汇总报告 + `batch-report.json`、打开输出目录；3 个 UI 测试（含“提交 Export 类型任务”断言），`lumark-ui` 169 个全过 |
| E-16 外部验证/真实验收 | ✅ | 第 37 轮：新增 `validate_exports` 工具（GLB 用官方 gltf crate；PSK/PSA/UEMODEL/USDA 独立布局解析）；BNB 9 个文件（Chicken GLB/PSK/PSA/UEMODEL/USDA + Box GLB/PSK/UEMODEL/USDA）全部 PASS；详见 `docs/export-validation.md`。限制：尚未在 UE/Blender 实际导入 |
| E-17 文档（export-formats） | ✅ | 第 32 轮：新增 `docs/export-formats.md`（五种模型格式能力/限制、坐标单位、四种贴图、materials.json、批量 API、验证记录） |

## M / I / A

- **M-1** ffmpeg/ffprobe 工具发现：✅；**M-2** 媒体探测：✅；**M-3** 视频抽帧：✅；**M-4** 播放窗口：✅（`media_player_window.rs`：首帧、画面、工具条、状态栏）；**M-5** 播放/暂停/±5s/±1 帧/回到开头 + 时间显示：✅；**M-6** 音频输出/同步：⬜；**M-7** 归档内媒体：⬜（文件系统视频已路由到播放器，归档条目仍走原流程）；**M-8** 测试/文档：✅（后端 3 测试 + UI 3 测试；`docs/media-player.md`）。
- **I-1** 拆分图片查看器：✅（第 46 轮拆为 `mod/title/toolbar/body/status/image_ops/app/tests`，最大 286 行）；**I-2** 滚轮缩放：✅（指数缩放 + 光标锚点）；**I-3** 左键拖拽：✅；**I-4** zoom 图标：✅（IconPark `zoom-in.svg`/`zoom-out.svg`）；**I-5** 文档：✅（新增 `docs/image-viewer.md`）。
- **A-1…A-6**：全部 ⬜。

## 最近提交

- `4d475d2` 批量导出后端与每游戏导出配置；BNB 四格式实机批量校验
- `097a970` USDA 导出与自动材质/贴图旁挂
- `6afb4e8` UEFormat UEMODEL 导出
- `66c5a05` glTF 2.0 binary 导出
- `3a7e173` ActorX PSK 导出
- 当前 `master` 领先 origin 22 个提交（P-5 待推送）。

## 下一轮（I-1 / M）

1. I-1 拆分 `image_viewer_window.rs`（1035 行）为 mod/工具/测试等子模块，目标 <600 行；
2. I-5 图片查看器文档（缩放/平移/旋转/EXIF/图标）；
3. M 媒体播放器（ffmpeg 后端/播放窗口）；
4. 记录进度并提交。
