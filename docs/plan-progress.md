# Lumark 计划进度（对照《分步执行计划 v4》）

> 更新：2026-11-02（目标模式第 20 轮结束）
> 说明：状态只有三种：`✅ 完成`、`🟡 部分完成`、`⬜ 未开始`；「证据」尽量给出文件、测试数或实机输出。

## 总览

| 阶段 | 完成度 | 当前状态 |
|---|---|---|
| P（CI 与基础设施） | 4/5 | P-5 推送/CI 验证待用户确认 |
| O（Oodle FModel 式） | 0/7 | 现有加载器可手动指定；自动下载/随包/UI 未做 |
| S（资产底座） | 6/9 | S-2/S-6 待补，其余已完成 |
| V（查看器） | 4/21 | V-0/1 完成，V-2/3 部分，V-4 本轮完成核心 |
| E（导出） | 8/18 | E-5/E-6/E-8/E-9 完成；E-3/E-7/E-11/E-13…E-17 待做 |
| M（媒体播放器） | 0/8 | 全部待做 |
| I（图片查看器） | 2/5 | I-2/I-3 已做（光标锚点待补），I-1/I-4 待做 |
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
| O-1 加载器状态 API | ⬜ | 现有 `configure_oodle_library` 可手动指定 |
| O-2 容器 needs_oodle 预检 | ⬜ | |
| O-3 运行时下载与缓存 | ⬜ | 计划 reqwest+zip+SHA-256 |
| O-4 随包内置 | ⬜ | |
| O-5 UI 集成与设置 | ⬜ | |
| O-6 诊断与文档 | ⬜ | |
| O-7 真实验收 | ⬜ | |

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
| V-16 渲染桥接/帧泵 | 🟡 | 第 25 轮：资产查看器 `image_path` 临时 PNG 改为 `Arc<RenderImage>`（`render_target_to_image` 直连）、删除逐帧落盘/Drop 清理，13 个查看器测试全过；异步帧泵与详情面板 RenderImage 直连待 V-16 后续 |
| V-17 UI 外壳/工具栏 | ⬜ | |
| V-18 检查器 | ⬜ | |
| V-19 输入/动画/快捷键 | ⬜ | |
| V-20 状态/入口/性能验收 | ⬜ | |

## E：导出

| 步骤 | 状态 | 证据/缺口 |
|---|---|---|
| E-0 DTO/公共基础 | 🟡 | `ExportMesh/ExportBone/ExportOptions/validate`；原子写/Report 未抽 |
| E-1 GameExportConfig（settings） | 🟡 | `BatchExportConfig` JSON 在 unreal；settings 版本化未接 |
| E-2 游戏编辑窗口导出 UI | ⬜ | |
| E-3 贴图写入器（PNG/JPEG/TGA/WebP/HDR） | ⬜ | |
| E-4 材质旁挂 | 🟡 | glTF 内嵌、USDA 侧车；`materials.json`/独立贴图目录未做 |
| E-5 glTF 2.0 GLB | ✅ | 4 测试 + BNB 实机 + gltf crate 校验 |
| E-6 ActorX PSK | ✅ | 3 测试 + BNB 实机 + chunk 校验 |
| E-7 ActorX PSA | ⬜ | |
| E-8 UEFormat 公共写入器 | ✅ | 含在 E-9 |
| E-9 UEFormat UEMODEL | ✅ | 3 测试 + BNB 实机 |
| E-10 USDA | 🟡 | 网格/材质/贴图/GeomSubset；USD Skel 未做 |
| E-11 单资产编排 | ⬜ | |
| E-12 批量枚举/执行 | 🟡 | 后端完成（4 测试 + BNB 四格式）；暂停/取消/报告 JSON 未做 |
| E-13 批量任务接入 | ⬜ | |
| E-14 单资产导出 UI | ⬜ | |
| E-15 批量导出 UI | ⬜ | |
| E-16 外部验证/真实验收 | 🟡 | GLB 已 gltf crate 校验；PSK/UEMODEL/USDA reader 已有示例 |
| E-17 文档（export-formats） | ⬜ | 当前记录在 STATUS/game-assets |

## M / I / A

- **M-1…M-8**：全部 ⬜（媒体播放器未开始）。
- **I-1** 拆分图片查看器：⬜；**I-2** 滚轮缩放：🟡（指数缩放已做，光标锚点未做）；**I-3** 左键拖拽：✅；**I-4** zoom 图标：🟡（已改 `−/+` 文本，IconPark 资源未加）；**I-5** 文档：⬜。
- **A-1…A-6**：全部 ⬜。

## 最近提交

- `4d475d2` 批量导出后端与每游戏导出配置；BNB 四格式实机批量校验
- `097a970` USDA 导出与自动材质/贴图旁挂
- `6afb4e8` UEFormat UEMODEL 导出
- `66c5a05` glTF 2.0 binary 导出
- `3a7e173` ActorX PSK 导出
- 当前 `master` 领先 origin 22 个提交（P-5 待推送）。

## 下一轮（V-17 / V-18）

1. V-17 查看器外壳与工具栏：着色 DropdownMenu、网格/骨骼 Toggle、LOD Select、重置/检查器入口，窄窗折叠；
2. V-18 检查器：场景树、LOD/section 材质、骨骼搜索、动画信息、世界统计；
3. V-19 输入/快捷键/动画 Slider；
4. 记录进度并提交。
