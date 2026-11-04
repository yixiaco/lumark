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
| 大文件/高内存测试清理 | ✅ | 删除 33 MB `zpaq_large_file_uses_configured_worker` 与 1 MB `bin_roundtrip_large`/`img_roundtrip_large`/`lz_large_data`，只保留小文件测试 |
| 验收 | ✅（受限） | fmt 通过；受影响包 clippy `-j 2` 通过；render 55/ unreal 102/ legacy 51/ UI 218（唯一失败用例修复后定向通过）；archive zpaq 目标仅编译；完整 workspace/release 按用户要求暂停 |

## 证据

- BNB 截图：`artifacts/viewer-nana-f6-2-fmodel.png`、`artifacts/viewer-nana-eye-base.png`。
- 新增测试：`camera_pan_moves_target_by_screen_pixels`、`viewer_pan_changes_render_and_resets`、`fmodel_params_use_spec_g_b_and_ao_g`、`fmodel_headlight_ao_r_darkens_result`、`default_sections_hide_missing_or_translucent_materials`、`viewer_resizable_inspector_sits_right_of_canvas`、`converts_nested_property_values`、`json_viewer_renders_readonly_editor`、`renders_real_nana_f6_2_viewer_frame_when_available`。
- 文档：`docs/viewer-architecture.md` 增加 P-7…P-10 表；`README.md`/`docs/game-assets.md` 更新查看器交互、材质面板与 JSON 入口；`THIRD_PARTY_NOTICES.md` 标注 CUE4Parse 触发名表归属。

## 进度说明

- 旧进度全部归档到 `docs/archive/`，活动台账只记录本轮。
- 按用户要求，完整性测试仅允许最多 2 线程且只跑受影响包；完整 workspace 不再重跑。
