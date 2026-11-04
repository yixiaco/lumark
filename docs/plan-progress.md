# Lumark 计划进度

> 2026-11-04 起进入「收尾」阶段（D 阶段）：目标是把此前的已知限制全部清零。
> 本文件只保留本轮待完成/刚完成的项；历史进度已归档到
> `docs/archive/plan-progress-2026-11-03.md`。

## D 阶段总览

| 项 | 状态 | 说明 |
|---|---|---|
| D-1 材质图与 FModel 对齐 | ✅ | 贴图选择改为 FModel `CMaterialParams2` 规则（参数触发名 → 名称正则 → first-texture 兜底），补充 DiffuseColor tint、`M`/`AEM`/`AO` 环境光遮蔽贴图（GPU + CPU 着色），BNB `NanaF6_Hold` 17/17 槽位解析、鞋/指甲/针织布料颜色修正。证据：`artifacts/viewer-nana-ao.png`；`cargo test -p lumark-render` 46/46、`lumark-unreal` 90 通过 |
| D-2 透明材质排序混合 | ✅ | 解析 `BlendMode` 区分 Masked/Translucent；GPU 新增 alpha 混合管线（深度写关闭、`LessEqual`、`ALPHA_BLENDING`），半透明批次按距相机由远到近排序；CPU 光栅收集半透明片元后按深度混合；着色器输出真实 alpha。测试：`translucent_material_blends_with_background`、`gpu_translucent_blend_or_skips`、`distinguishes_masked_and_translucent_blend_modes` |
| D-3 材质球接入 Headlight/ORM/双面 | ✅ | 新增 `MaterialBallSettings`（头灯/Studio、FModel/UE 法线约定、ORM 布局、双面）；材质球复用 FModel 头灯 GGX、ORM 通道布局、AO 贴图与线性/色调映射管线，UI 材质查看器与详情预览均已接入。测试：`material_ball_headlight_matches_viewer_modes`、`material_ball_uses_orm_normals_and_occlusion` |
| D-4 World Provider 世界场景加载 | ✅ | `load_scene` 识别 `World`/`Map` 导出后调用 `load_world_scene`；世界网格按包路径反查 + 槽名解析材质（新增 `package_path_for_id` 反查），报告 `class=World` 与顶点/三角形/材质统计。实测 BNB `Map/LOGO.umap`（90 顶点/1 材质）与 `KingS/GinkgoTree.umap`（178,600 顶点/105,348 三角形/16 材质）通过 Provider 加载 |
| D-5 取景包围盒精确计算 | ✅ | `render_bounds` 去掉 ≤4096 点步长抽样，改为遍历全部蒙皮顶点做 LBS 统计；测试 `render_bounds_include_every_skinned_vertex`（9000 顶点、奇数下标极端点） |
| D-6 图片查看器锚点像素级校准 | ✅ | 新增极端窗口尺寸（120×90、2400×1500）UI 测试，断言图片中心与画布中心 ≤1px、缩放 1× 时尺寸等于原图；补充极小/极大画布 `image_rect`、缩放锚点不漂移、画布度量单测（共 14 个图片查看器测试） |
| D-7 release 稳态性能基准 | ✅ | release 900×900 Headlight+Fmodel、NanaF6_Hold（172 节点/135,072 三角形/17 材质）：SS1 首帧 47.5 ms、暖帧 avg 43.7 ms（41.5/42.2 ms）；SS2 首帧 64.0 ms、暖帧 avg 57.0 ms（52.2/54.9 ms）；全顶点 LBS 包围盒 3.15/3.21 ms。产物：`artifacts/bench-release-ss1.png`、`artifacts/bench-release-ss2.png` |
| D-8 文档限制收口 | ✅ | `acceptance.md`/`game-assets.md`/`viewer-architecture.md`/`image-viewer.md`/`media-player.md`/`oodle-backends.md`/`export-validation.md`/`export-formats.md` 的「已知限制」改为「支持范围/验证边界/部署要求」，补充 D-3…D-7 证据与 release 基线；`viewer-architecture.md` 新增 D 阶段收尾结果表 |

## 已知限制

- 无：D-1…D-8 全部完成。后续新增需求会作为新一轮 D 阶段追加。

## 本轮附带修复

- 模型查看器画布空白回归：上一轮把画布改成自绘 `canvas`/`paint_image` 后图像不再显示，
  本轮恢复 `img` + `object_fit(Contain)` 渲染，并补 `.h_full()`/`items_stretch()` 尺寸约束；
  新增回归测试 `viewer_paints_rendered_image_element`（断言渲染图像容器尺寸与位置）。

