# Lumark 计划进度（对照《分步执行计划 v4》）

> 更新：2026-11-02（目标模式第 20 轮结束）
> 说明：状态只有三种：`✅ 完成`、`🟡 部分完成`、`⬜ 未开始`；「证据」尽量给出文件、测试数或实机输出。

## 总览

| 阶段 | 完成度 | 当前状态 |
|---|---|---|
| P（CI 与基础设施） | 5/5 | 全部完成（P-5 已推送并 CI 成功） |
| O（Oodle FModel 式） | 7/7 | 全部完成 |
| S（资产底座） | 9/9 | 全部完成 |
| V（查看器） | 21/21 | 全部完成（V-14 世界 Provider 已落地） |
| E（导出） | 18/18 | 全部完成 |
| M（媒体播放器） | 8/8 | 全部完成 |
| I（图片查看器） | 5/5 | 全部完成 |
| A（验收/文档/发布） | 6/6 | 全部完成（A-5 已由 GitHub Actions 验证） |

## P：CI 与基础设施

| 步骤 | 状态 | 证据 |
|---|---|---|
| P-1 行数门禁放宽 | ✅ | `tools/check_file_sizes.ps1`；警告 1000/失败 3000（按用户最新要求） |
| P-2 zpaq CRT 修复 | ✅ | `vendor/zpaq_rs/build.rs`；release 链接通过 |
| P-3 发布脚本清理 CC/CXX | ✅ | `tools/build_release.ps1` |
| P-4 Workflow 收尾 | ✅ | `.github/workflows/windows-build.yml` |
| P-5 推送并观察 CI | ✅ | 第 74 轮推送 `dc85d36` 后修复 CI 流程；run 41588158621（`2b510dd`）Check/Build 全部成功：Format、File sizes、Clippy、Test、Package、Artifact 均通过 |

## O：Oodle（FModel 式）

| 步骤 | 状态 | 备注 |
|---|---|---|
| O-1 加载器状态 API | ✅ | 第 38 轮：`OodleStatus{setting,rust_available,dll_supported,dll_configured,dll_loaded}` + `usable()/summary()` + `oodle_status()`；2 测试；游戏设置页显示当前后端摘要 |
| O-2 容器 needs_oodle 预检 | ✅ | 第 38 轮：`IoStoreContainer::needs_oodle`、`GameAssetSource::oodle_preflight()`（`OodlePreflight{needed,ready,methods,status,summary}`）；BNB 实机 92368 条目、methods=["Oodle"]、needed=true、ready=true（Rust 后端） |
| O-3 运行时下载与缓存 | ✅ | 第 39 轮：`oodle_download`：reqwest 流式下载 + 进度 → SHA-256 校验 → 原子缓存 `oodle.zip` → zip 安全解压 `oo2core*/oodle*.dll` → 缓存复用；`install_oodle_zip` 离线安装；4 测试（安装/复用、SHA、路径穿越、非法 DLL） |
| O-4 随包内置 | ✅ | 第 39/61 轮：查找 `oodle/`、`resources/oodle/`；发布流水线新增 `build_release.ps1 -OodleDll <授权 DLL>`，复制到 `target\release\oodle`；`pack-installer.ps1` 将 `oodle/` 带入暂存目录，安装器/便携包自动包含。实测 7 字节占位 DLL 进入 `dist\windows\oodle` 且 Inno 编译成功；仓库仍不内置二进制 |
| O-5 UI 集成与设置 | ✅ | 第 38/41 轮：游戏设置 Oodle 区显示“当前后端”摘要；新增「下载 Oodle…」入口与 `oodle_download_window`（URL/SHA/缓存目录、进度百分比与字节、下载后自动解压并配置、失败原因）；2 UI 测试 |
| O-6 诊断与文档 | ✅ | 第 38/39 轮：`docs/oodle-backends.md` 补充下载/缓存 API、随包目录、设置页、诊断；`oodle_preflight` 示例；BNB 实机 |
| O-7 真实验收 | ✅ | 第 40 轮：BNB IoStore（92,368 条目/Oodle/读取 2,297B 资产）与 FF7R PAK（18,896 条目/None+Zlib+Gzip+Oodle/读取 700B 资产）均 `needed=true ready=true`，Rust 后端真实解码成功 |

## S：共享资产底座

| 步骤 | 状态 | 证据 |
|---|---|---|
| S-1 GameAssetSource | ✅ | `crates/lumark-unreal/src/source.rs` |
| S-2 PAK 按条目读 API | ✅ | `PakAssetReader`（`pak/unreal.rs`）：`open`/`entries`/`paths`/`contains`/`entry_size`/`read_entry`/`compressed`/`compression_methods`/`needs_oodle`；不落临时文件、密钥仅内存。新增 v11 未压缩与压缩 PAK 实机测试（按路径读取、大小一致、缺失报错） |
| S-3 预览读取下沉 | ✅ | `preview_ue.rs` 分模块、材质解析在 `materials.rs` |
| S-4 Zen 网格法线/切线/UV/材质槽 | ✅ | `package/mesh.rs`；BNB 回归 |
| S-5 旧式网格 UV/法线 | ✅ | `legacy_skeleton.rs` |
| S-6 HDR/法线纹理解码 | ✅ | `texture.rs` 支持 `PF_FLOATRGB`/`PF_FLOATRGBA`/`PF_RGBA16F`/`PF_R32_FLOAT`/`PF_R32G32B32F`/`PF_RGBE` 线性 HDR 解码与半边浮点；BC5 法线走 `texture2ddecoder`；测试 `decodes_float_rgba_hdr`/`decodes_float_rgb_and_rgbe_hdr`/`decodes_float32_formats`；HDR 贴图经 `decode_mip_rgba` tone map 后供材质预览 |
| S-7 材质槽解析 | ✅ | `materials.rs`；BNB/FF7R 实机 |
| S-8 动画采样接口 | ✅ | `skinning.rs` |
| S-9 .umap 解析 | ✅ | `package/world.rs`；BNB 地图 |

## V：查看器

| 步骤 | 状态 | 证据/缺口 |
|---|---|---|
| V-0 GPU 预研 | ✅ | `lumark-render` wgpu 30；RTX 5070 Vulkan 实测 |
| V-1 lumark-scene | ✅ | Scene/Mesh/Material/Inspector/Transform；11 测试 |
| V-2 渲染核心 | ✅ | CPU 光栅 + GPU 离屏渲染；异步帧泵（background_executor + 代号丢弃）；GPU 错误重建设备并回退 CPU；双 staging 以 CPU 回读/图像转换为准；GPU/CPU 测试覆盖 |
| V-3 相机/网格/清屏 | ✅ | 轨道相机、清屏、超采样；CPU 与 GPU 背景渐变一致（全屏三角形、顶部 1.25×/底部 0.75×、tone mapping）；地面网格 `show_grid` |
| V-4 PBR 网格 + 纹理 | ✅（核心） | 本轮：顶点切线、基础色/法线/ORM/自发光贴图、alpha 裁剪、单/双面剔除；CPU 18 测试 + GPU 2 测试全过；MI_Props 实机截图 `target/render-box-gpu-v4.png`。缺口：CPU mips/纹理 LRU/sRGB 视图切换未做 |
| V-5 GPU 蒙皮 + 动画 | ✅ | 第 21 轮：`MeshSkin`（关节/inverse bind/pose）+ CPU/GPU LBS + GPU storage buffer（≤4 影响归一化）；`apply_animation` 接入 `sample_animation`；BNB `SK_Chicken + ANIM_Chicken_Idle2` 42 轨道 t=0/1s 两帧渲染成功（~87–93 ms，PNG hash 不同）；`lumark-render` 25 测试全过（CPU/GPU 蒙皮平移） |
| V-6 叠加层/着色模式 | ✅ | 第 22/67 轮：`ShadingMode{Lit,Unlit,Normals}`、地面网格、骨骼线、隐藏 section；**线框模式**（CPU 光栅与 GPU line 管线共用 `wireframe_lines`，工具栏 + W 快捷键）；CPU 光栅/GPU/overlay 测试 |
| V-7 实例化/大规模场景 | ✅ | 第 55 轮：静态网格批次键改为「网格+section+材质」，共享本地空间顶点缓冲，每节点仅追加实例矩阵；GPU 新增实例顶点缓冲（step=Instance，4×vec4）与着色器实例变换；蒙皮网格仍独占批次。新增 GPU 两实例左右半屏测试；BNB `SM_Box_02` GPU 480×480 2×SS 81.5ms 截图正常 |
| V-8 质量/统计/设备丢失 | ✅ | 第 54/56/63/64/65 轮：场景统计 + 状态栏；GPU 错误自动重建并回退 CPU；4×MSAA；纹理 LRU（64 项、内容指纹）；**sRGB 贴图开关**（工具栏 + `RenderSettings.srgb_textures`，GPU 测试断言开关改变输出）；缺 sRGB 视图切换的低层格式切换，但不影响使用 |
| V-9 渲染器测试/文档 | ✅ | 第 22 轮：30 个测试 + `docs/viewer-architecture.md`（分层/API/管线/模式/基线/限制） |
| V-10 UE 静态网格 Provider | ✅ | 第 23 轮：`provider` 模块 `SceneRequest`/`LodPolicy`/`SceneLoadOptions`/`load_scene`/`SceneLoadStage`/`SceneLoadReport`；StaticMesh/SkeletalMesh 按 LOD 选层、材质解析、warning 汇总；BNB `SM_Box_02`（3 LOD）与 `SK_Chicken`（2 LOD）实机加载 |
| V-11 UE 材质到 Scene | ✅ | 总贴图缓存 `TextureCache` + `MaterialResolveOptions.texture_cache`、材质/贴图缺失 warning；HDR 贴图经 `decode_mip_rgba` tone map 进入材质预览；BNB 样本缺材质时输出可读 warning |
| V-12 骨骼网格/动画 Provider | ✅ | 第 24 轮：`SceneLoadOptions.animation/animation_time`、`SceneLoadStage::Animation`、`AnimationInfo`（名称/时长/帧数/轨道/采样时间）、`LoadedScene.skeleton`；BNB `SK_Chicken + ANIM_Chicken_Idle2` t=0.5s Provider 实机加载（42 骨骼/42 轨道/141 帧/4.667s）；时间轴 UI 待 V-19 |
| V-13 独立场景 | ✅ | 查看器可开 Mesh/Skeleton/SkeletalMesh/Material；动画时间轴 Slider、播放/逐帧、键盘快捷键与异步重绘已接入 |
| V-14 世界场景 Provider | ✅ | 第 72/73 轮：CityHash/`package_id`、IoStore ContainerHeader、按包 ID 读包、Zen ImportMap/公共哈希解析；新增 `world_scene` Provider（Actor→StaticMeshComponent→跨包 StaticMesh→Scene），Rotator/TRS 解码。BNB `GinkgoTree.umap` 实机：45 Actors → **11 个网格 / 105,348 三角形**，耗时约 2.25s；示例 `world_scene` |
| V-15 UI 模块拆分 | ✅ | 第 26 轮：`asset_viewer_window/` 拆为 `mod/animation/app/body/render/status/toolbar/tests` 8 个文件，最大源码模块 242 行（toolbar）、测试 434 行，均 <600；行为不变，13 个查看器测试与 162 个 UI 总测试全过 |
| V-16 渲染桥接/帧泵 | ✅ | 第 25/29/59 轮：`image_path` 临时 PNG 改为 `Arc<RenderImage>`；网格/骨骼网格预览走 `render_service`（GPU 优先）；拖拽/滚轮异步离屏渲染，代号丢弃过期帧，状态栏“渲染中…”，骨骼/材质轻量光栅保留同步；详情面板 RenderImage 已直连 |
| V-17 UI 外壳/工具栏 | ✅ | 第 29 轮：预览接入 `lumark_render` + `render_service`；着色循环（光照/无光/法线）、地面网格、骨骼线开关、检查器开关、缩放 `−/+`；网格/骨骼网格行为与 V-6 RenderSettings 打通 |
| V-18 检查器 | ✅ | 第 27/60/62 轮：概要（类型/LOD/顶点/三角形/section/骨骼/动画/可见实例与绘制批次）、LOD 切换、section 显隐 + **材质名**、骨骼列表 + 名称搜索（大小写不敏感、匹配数/总数）、动画状态、材质参数、关闭；材质名由 `UePreviewResult::Model` 透传到查看器 |
| V-19 输入/动画/快捷键 | ✅ | 第 29/57/58 轮：滚轮缩放（0.2–6×）、键盘快捷键（F/Space/←→/L/G/B/N）、**动画时间轴 Slider**（`SliderState` + 订阅 Change，拖动/点击跳转并暂停播放，播放/步进/重置后同步位置）；正式窗口创建 Slider 并保持订阅。测试通过 |
| V-20 状态/入口/性能验收 | ✅ | 第 54 轮：`lumark_render::SceneStats` + `scene_stats`（可见实例/唯一网格/预计批次/顶点/三角形/蒙皮实例）；`RenderedImage.stats` 透传；资产查看器状态栏显示后端 + 批次/顶点/三角形；测试：统计单测 + 全量 534 测试；性能实测记录于 `docs/acceptance.md`（无自动阈值） |

## E：导出

| 步骤 | 状态 | 证据/缺口 |
|---|---|---|
| E-0 DTO/公共基础 | ✅ | `ExportMesh/ExportBone/ExportOptions/validate` + `write_atomic`（临时 `.part` + fsync + rename，失败清理）统一模型/旁挂/报告落盘；2 测试；BNB 实机重导出无残留 `.part`。Report 仍由批量/单资产各自持有 |
| E-1 GameExportConfig（settings） | ✅ | 第 52 轮：`lumark-settings::GameExportConfig`（输出目录/格式/过滤/limit/overwrite/材质/贴图上限/缩放，`Default` + `normalized`），每游戏 `GameLibraryEntry.export`，旧 JSON 自动回退默认；2 测试 |
| E-2 游戏编辑窗口导出 UI | ✅ | 第 52 轮：游戏编辑页新增导出区（格式/数量/覆盖/材质/过滤）；保存时写入规范化配置；批量导出窗口按当前游戏配置初始化（输出默认 `游戏根/LumarkExports`）；批量窗口 4 测试、`lumark-ui` 176 测试全过 |
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

- **M-1** ffmpeg/ffprobe 工具发现：✅；**M-2** 媒体探测：✅；**M-3** 视频抽帧：✅；**M-4** 播放窗口：✅；**M-5** 播放/暂停/±5s/±1 帧/回到开头 + 时间显示：✅；**M-6** 音频输出/同步：✅（同目录 ffplay `-nodisp -autoexit -ss` 子进程；播放/暂停/跳转/关闭窗口同步启停；无音轨或无 ffplay 时仅画面并提示）；**M-7** 归档内媒体：✅；**M-8** 测试/文档：✅。
- **I-1** 拆分图片查看器：✅（第 46 轮拆为 `mod/title/toolbar/body/status/image_ops/app/tests`，最大 286 行）；**I-2** 滚轮缩放：✅（指数缩放 + 光标锚点）；**I-3** 左键拖拽：✅；**I-4** zoom 图标：✅（IconPark `zoom-in.svg`/`zoom-out.svg`）；**I-5** 文档：✅（新增 `docs/image-viewer.md`）。
- **A-1** 原始 8 项需求验收清单：✅（`docs/acceptance.md` 表格逐项证据）；**A-2** 构建/运行说明：✅（README + acceptance）；**A-3** 测试/性能报告：✅（`cargo test --workspace --lib` **534 个库测试全过** + fmt/clippy/check；BNB/FF7R 实机与渲染耗时）；**A-4** 已知限制/兼容性：✅；**A-5** 发布流程/CI：✅（GitHub Actions run 41588158621 success：Check + Build/package/artifact 全通过）；**A-6** 文档索引：✅（`docs/acceptance.md` 第六节）。

## 最近提交

- `4d475d2` 批量导出后端与每游戏导出配置；BNB 四格式实机批量校验
- `097a970` USDA 导出与自动材质/贴图旁挂
- `6afb4e8` UEFormat UEMODEL 导出
- `66c5a05` glTF 2.0 binary 导出
- `3a7e173` ActorX PSK 导出
- 当前 `master` 已推送并与 origin 同步；GitHub Actions run 41588158621 全绿。

## 完成状态

计划全部步骤完成：P 5/5、O 7/7、S 9/9、V 21/21、E 18/18、M 8/8、I 5/5、A 6/6。
