# Lumark 计划进度

> 2026-11-04 查看器 P-7…P-10 轮次：只保留本轮完成项。
> 历史进度已归档到 `docs/archive/plan-progress-2026-11-04.md`。

## 本轮总览

| 项 | 状态 | 说明 |
|---|---|---|
| P-7 右键拖拽平移 | ✅ | `RenderSettings.pan_pixels`；相机按 FOV/距离把屏幕像素换算为世界偏移；查看器右键拖拽、`F`/重置复位；方向/像素 1:1/缩放无关单测通过 |
| P-8 FModel 材质/光照对齐 | ✅ | CUE4Parse `CMaterialParams2` 触发名表（Apache-2.0）、RoughnessMin/Max、EmissiveMult、Skin Boost、blend/shading；Headlight 采用 `default.frag` 的 spec mask G/B + AO R/G/B；默认隐藏无贴图/半透明 section；CPU/GPU/材质球同步；BNB `NanaF6_2` 眼睛覆盖层消失、`N_Eye`/`N_Skin` 材质与贴图断言通过 |
| P-9 查看器面板与 UI | ✅ | 可调宽检查器（320px，260–640）、概要/材质/骨骼标签、section 显隐与材质详情、贴图名称/尺寸、双面/warnings；工具栏分组并移除 S0…S16/LOD，新增顶点色与属性 JSON；标题栏按资产类型取图标 |
| P-10 JSON 属性查看器 | ✅ | 导出表 + 属性树 JSON，对象引用尽量解析、4 MB/64 导出上限；预览携带 `Arc<str>`；查看器/详情入口；独立只读窗口（JSON 高亮、复制、行数/字节数） |
| 测试与构建内存清理 | ✅ | 全 crates 删除重复/低价值测试，当前 680 条；新增 `tools/test_workspace.ps1`，默认 `build.jobs = 4`，开发/测试配置仅保留工作区代码行号表；MSVC 链接峰值从 5.48 GiB 显著下降 |
| Snooper 色彩/光泽对齐 | ✅ | `LightingMode::Snooper`（太阳 + 天空上下半球、`F0=mix(0.08*Specular, albedo, Metallic)`、Snooper GGX/Smith、只做 gamma）；材质 R/G/B=高光/金属/粗糙，新增 `specular` 标量；查看器默认 Snooper，AO 近似 SSAO；检查器显示高光强度 |
| 验收 | ✅（受限） | fmt 通过；受影响包测试通过；render 47 / unreal 88 / legacy 19 / UI viewer 24 + BNB；完整 workspace 与 release 构建按用户低资源要求暂停 |

## 证据

- BNB 截图：`artifacts/viewer-nana-f6-2-fmodel.png`、`artifacts/viewer-nana-eye-base.png`、`artifacts/viewer-nana-skin-base.png`。
- Snooper 测试：`snooper_lighting_uses_sky_ambient_and_sun`、`snooper_specular_scalar_changes_highlight`、`snooper_frame_uses_gamma_only_encoding`、`wgsl_shader_parses_and_validates`。
- 新增测试：`camera_pan_moves_target_by_screen_pixels`、`viewer_pan_changes_render_and_resets`、`fmodel_params_use_spec_g_b_and_ao_g`、`fmodel_headlight_ao_r_darkens_result`、`default_sections_hide_missing_or_translucent_materials`、`viewer_resizable_inspector_sits_right_of_canvas`、`converts_nested_property_values`、`json_viewer_renders_readonly_editor`、`renders_real_nana_f6_2_viewer_frame_when_available`。
- 文档：`docs/viewer-architecture.md` 增加 P-7…P-10 表；`README.md`/`docs/game-assets.md` 更新查看器交互、材质面板与 JSON 入口；`THIRD_PARTY_NOTICES.md` 标注 CUE4Parse 触发名表归属。

## 进度说明

- 旧进度全部归档到 `docs/archive/`，活动台账只记录本轮。
- 按用户要求，完整性测试仅允许最多 2 线程且只跑受影响包；完整 workspace 不再重跑。
