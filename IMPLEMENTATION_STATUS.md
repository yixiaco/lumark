# Lumark 实施状态

> 2026-11-04 起为「收尾」阶段（D 阶段），只记录本轮待完成/刚完成项。
> 历史实施记录已归档到 `docs/archive/implementation-status-2026-11-03.md`。

## 2026-11-04 收尾（D 阶段）

- [x] D-1 材质图与 FModel 对齐：按 FModel `CMaterialParams2` 选择基础色/法线/ORM/自发光贴图（参数触发名优先，`_D`/`_N`/`_SM` 等名称正则兜底，基础色回退第一张贴图），应用 DiffuseColor tint；解析 `M`/`AEM`/`AO` 环境光遮蔽贴图并接入 GPU/CPU 着色。BNB `NanaF6_Hold` 17 个槽位贴图全部正确，鞋/指甲/针织布料颜色与 FModel 观感一致。
- [x] D-2 透明材质按 BlendMode 区分 Masked/Translucent：GPU 新增 alpha 混合管线（不写深度、`LessEqual`、`ALPHA_BLENDING`）并按距相机从远到近绘制半透明批次；CPU 光栅收集半透明片元后按深度混合；着色器输出真实 alpha。测试：`translucent_material_blends_with_background`、`gpu_translucent_blend_or_skips`、`distinguishes_masked_and_translucent_blend_modes`。
- [x] 修复模型查看器画布空白回归：恢复 `img` + `object_fit(Contain)` 显示，补 `.h_full()`/`.items_stretch()` 尺寸约束，新增 `viewer_paints_rendered_image_element` 回归测试。
- [x] D-3 材质球接入 Headlight/Fmodel 法线约定/ORM 布局/双面：`MaterialBallSettings` + 复用 FModel 头灯 GGX、ORM 通道、AO 贴图与线性/色调映射管线；UI 材质查看器与旧式材质预览均已接入。测试：`material_ball_headlight_matches_viewer_modes`、`material_ball_uses_orm_normals_and_occlusion`。
- [x] D-4 World Provider 接入 `.umap` 世界场景加载：`load_scene` 识别 World/Map 导出 → `load_world_scene`，世界网格按包 ID 反查路径并解析槽位材质；实测 BNB `Map/LOGO.umap`、`KingS/GinkgoTree.umap`（178,600 顶点 / 105,348 三角形 / 16 材质）通过 Provider。
- [x] D-5 取景包围盒改为精确计算：`render_bounds` 遍历全部蒙皮顶点，测试 `render_bounds_include_every_skinned_vertex`。
- [x] D-6 图片查看器锚点极端窗口像素级测试：120×90 / 2400×1500 窗口下图片中心与画布中心 ≤1px，补充极端画布/缩放锚点单测。
- [x] D-7 release 稳态性能基准：release 900×900 Headlight+Fmodel（NanaF6_Hold）：SS1 首帧 47.5 ms / 暖帧 avg 43.7 ms；SS2 首帧 64.0 ms / 暖帧 avg 57.0 ms；全顶点 LBS 包围盒 3.15/3.21 ms。产物 `artifacts/bench-release-ss1.png`、`artifacts/bench-release-ss2.png`。
- [x] D-8 清理各处「已知限制」文档表述：活动文档改为「支持范围/验证边界/部署要求」，补充 D-3…D-7 证据；`viewer-architecture.md` 增加 D 阶段收尾结果表，`docs/archive/` 保留历史记录。

## 验收

- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；
- `cargo test --workspace -j 2 -- --test-threads=1`：74 个测试目标、801 个测试通过、0 失败
  （ui 206 / unreal 93 / render 49）；release 基准见 `docs/viewer-architecture.md`。
