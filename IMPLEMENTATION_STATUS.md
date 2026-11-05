# Lumark 实施状态

> 2026-11-05 UE5.2（NTR_IDOL）bulk data 修复轮次：只记录本轮完成项。
> 历史实施记录已归档到 `docs/archive/implementation-status-2026-11-04.md`。

## 2026-11-05 UE5.2 Zen bulk data 映射与外部 bulk

- [x] Zen bulk data 映射门控修正：映射不再只对 `header_version >= 5`（UE5.4+）读取，改为按
  对象版本 `DATA_RESOURCES`（1009）判定——有版本信息时按 `file_version_ue5`，无版本信息的
  cooked 包按结构探测（长度是 32 的倍数、整体位于导入公共导出哈希表之前、条目字段合理），
  探测失败回退为无映射；UE5.4+ 才读 8 字节对齐 pad。修复前该游戏所有包 `bulk_data` 恒为空。
- [x] 统一 `FByteBulkData` 读取（新增 `package/bulk.rs` + `BulkReader`）：映射索引形式按映射表
  取长度/偏移/标志，传统内联头形式解析 `ForceInlinePayload`/`LazyLoadable`/`Size64Bit`/`Unused`；
  **外部载荷在两种形式下都能从 `.ubulk` 切片**（修复“提供了 .ubulk 仍报缺少侧载”）。
  纹理与 SkeletalMesh 改用同一实现，错误文案区分“未附带侧载数据”。
- [x] `.ubulk` 解析兜底：目录索引没有独立 `.ubulk` 条目时按包 ID 读取 IoStore BulkData chunk。
- [x] StaticMesh 外部 bulk：`read_static_mesh_with_bulk` 在 `bInlined=false` 时先读整块几何
  `FByteBulkData`，再用内存块复用内联缓冲解析；LOD 头部按 UE5.6 / UE5.2–5.4 / UE5.5 三种布局
  依次尝试。UI 预览、场景加载、导出与 World 引用网格均传入 `.ubulk` 侧载数据。
- [x] 诊断示例 `inspect_mesh_bulk`（自动选择 StaticMesh 导出）。
- [x] **UE5.0–5.2 导出数据顺序修正**：Zen 头 `GraphDataOffset` 处是 `FExportBundleHeader`
  （16 字节）、`ExportBundleEntriesOffset` 处是 `FExportBundleEntry`（8 字节），导出数据按
  bundle 的 `Serialize` 顺序从 `HeaderSize` 起紧凑排列，`CookedSerialOffset` 只是 cook 时的
  虚拟偏移。新增 `bundle_data_offsets`：自洽时按 bundle 顺序定位，失败回退旧换算；与
  CUE4Parse `IoPackage.PopulateExports`（5.0–5.2 分支）一致。修复前多导出包（BodySetup/
  NavCollision/StaticMesh）按索引顺序取值，StaticMesh 落在几何中间，属性与几何全部读错。
- [x] `PerPlatformFrameRate`（`bCooked` + `FFrameRate`）属性读取，修复 `AnimSequence` 的
  `PlatformTargetFrameRate` 解析。
- [x] `PackageInfo` 增加 `header_size` / `cooked_header_size` / `package_flags` 供诊断。
- [x] 测试：`parses_initial_layout_bulk_data_map`、`lays_out_export_data_in_bundle_order`、
  `bulk::tests::*`（内联/外部/传统头回退/`Unused`/越界），实机用例 `tests/ntr_iostore.rs`
  （`LUMARK_NTR_GAME_ROOT` + `LUMARK_NTR_MAPPINGS` 门控，含纹理与 StaticMesh 两条链路）。
- [x] NTR_IDOL 实机：`T_Worker_Hair_SSC` 2048×2048 PF_DXT1 12 mip（mip0 2 MiB，外部 bulk）、
  `1y1` 1200×801 PF_B8G8R8A8（内联 bulk）、`SM_PillarFrame` 668 顶点/620 三角形、
  `NPC_Event_002_CG01` 57744 顶点/79724 三角形/2 section 均解出并写出 PNG：
  `artifacts/ntr-idol-texture-hair.png`、`artifacts/ntr-idol-texture-1y1.png`、
  `artifacts/ntr-idol-mesh-pillar.png`、`artifacts/ntr-idol-mesh-npc.png`。
- [ ] 已知限制：`BodySetup.AggGeom` 的 `FKConvexElem`/`KAggregateGeom` 内嵌布局未支持，
  含凸包碰撞体的包读取 `BodySetup` 属性会报错（不影响模型/纹理/材质预览）。

## 2026-11-04 查看器交互、FModel 材质与 JSON 属性

- [x] P-7 右键拖拽平移：`RenderSettings.pan_pixels` + 相机按 `2*distance*tan(fov/2)/height` 做像素→世界换算；查看器右键拖拽更新 pan、左键仍轨道旋转、`F`/重置视角清零；状态栏提示同步。测试：`camera_pan_moves_target_by_screen_pixels`、`camera_pan_is_independent_of_zoom`、`pan_drag_accumulates_in_screen_space`、`viewer_pan_changes_render_and_resets`。
- [x] P-8 FModel 材质/光照对齐：材质触发名表按 CUE4Parse `CMaterialParams2`（Apache-2.0）首 UV 层迁入 `material_params.rs`；新增 RoughnessMin/Max、EmissiveMult、Skin Boost、blend/shading 解析；Headlight 改为 `default.frag` 的 spec mask G=金属度/腔体、B=粗糙度、AO R/G/B 公式；CPU/GPU/材质球同步；默认隐藏无贴图/半透明 section。测试：`fmodel_params_use_spec_g_b_and_ao_g`、`fmodel_headlight_ao_r_darkens_result`、`fmodel_headlight_vertex_colors_are_opt_in`、`fmodel_headlight_emissive_requires_texture`、`default_sections_hide_missing_or_translucent_materials`、`ao_parameter_feeds_specular_mask_and_occlusion`。
- [x] P-9 查看器 UI/检查器重构：`inspector/` 拆分为概要/材质/骨骼；检查器 `h_resizable` 默认 320px（260–640px）；材质页含 section 显隐、blend/shading、双面、tint、roughness 区间、AO/自发光、贴图名称/尺寸、warnings；骨骼树按父索引缩进；工具栏分组并移除 S0…S16/LOD 按钮，新增顶点色与“属性 JSON”；标题栏按资产类型取图标。测试：`viewer_resizable_inspector_sits_right_of_canvas`、`inspector_panel_renders_and_toggles`、`material_viewer_toolbar_hides_mesh_controls`、`viewer_keyboard_shortcuts_update_state`。
- [x] P-10 JSON 属性查看器：新增 `properties_json`（导出表 + 属性树、对象引用尽量解析、4 MB/64 导出上限、深度限制）；预览链路 `UePreviewOutput.properties_json` → `details_properties_json`；查看器工具栏与详情面板入口；独立只读窗口（JSON 高亮、复制、行数/字节数）。测试：`converts_nested_property_values`、`stops_at_max_depth`、`json_viewer_renders_readonly_editor`、BNB 实机 `renders_real_nana_f6_2_viewer_frame_when_available`。
- [x] BNB 实机验收：`NanaF6_2` 的 `N_EyeLip`/`N_EyeShadow` 默认隐藏，`N_Eye`/`N_Skin` 可见；`N_Eye` 基础色为 `eyeBro`，`N_Skin` spec/AO 均为 `NewNana_AO`。产物 `artifacts/viewer-nana-f6-2-fmodel.png`、`artifacts/viewer-nana-eye-base.png`。
- [x] 大文件/高内存测试清理：按用户要求删除 33 MB 的 `zpaq_large_file_uses_configured_worker`，以及 1 MB 的 `bin_roundtrip_large`、`img_roundtrip_large`、`lz_large_data`；只保留小文件测试。

## 2026-11-04 Snooper 色彩/光泽对齐（第二轮）

- [x] `LightingMode::Snooper`：按 Snooper OpenGL `scene_lighting.glsl` 实现太阳方向光 + 天空上下半球环境光；`F0 = mix(0.08*Specular, albedo, Metallic)`；`kD = (1-F)*(1-metallic)`；Snooper GGX/Smith；不做 Reinhard，只做 gamma。
- [x] 材质通道：`R=Specular/G=Metallic/B=Roughness`，新增 `MaterialAppearance.specular`（`Specular`/`Spec`/`Base Specular`，默认 0.5）并传入 `MaterialData`/CPU/GPU uniform；AO 用于近似 SSAO；检查器显示高光强度。
- [x] 查看器默认切到 `Snooper + Fmodel`：太阳/天空常量按 Snooper `SunActor`/`EnvironmentActor` 参考并针对无阴影/无天空盒的查看器调整曝光；背景改为天空蓝。
- [x] 测试：`snooper_lighting_uses_sky_ambient_and_sun`、`snooper_specular_scalar_changes_highlight`、`snooper_frame_uses_gamma_only_encoding`、`wgsl_shader_parses_and_validates`；BNB `NanaF6_2` 实机出新图 `artifacts/viewer-nana-f6-2-fmodel.png`，另导出皮肤基础色 `artifacts/viewer-nana-skin-base.png`。

- [x] 预览一致性：详情缩略图改用 `RenderSettings::snooper_viewer`，与查看器共享太阳/天空/背景；
  仅隐藏地面网格。查看器拖拽帧使用 0.75× 分辨率，静止后恢复 900×600 + SS2/MSAA4。
- [x] 眼睛：加载 `ScleraColor`/`EyeW` 并新增 `MaterialTextures.sclera`；视口/着色器在虹膜半径内混合
  `eyeBro`，半径外使用巩膜；`N_Eye` 断言已选中巩膜。

### Snooper 验收

- `cargo fmt --all -- --check`：通过。
- `cargo clippy -p lumark-render -p lumark-unreal -p lumark-ui -j 2 --all-targets -- -D warnings`：通过。
- `cargo test -p lumark-render --lib -j 2 -- --test-threads=2`：48 passed（含 `wgsl_shader_parses_and_validates`）。
- `cargo test -p lumark-unreal --lib -j 2 -- --test-threads=2`：88 passed。
- `cargo test -p lumark-ui --lib app::view::asset_viewer_window::tests:: -j 2 -- --test-threads=2`：25 passed；BNB 实机用例定向通过。
- 按用户资源限制，完整 workspace/UI 套件未重跑，只跑受影响包与查看器定向用例。

## 验收（按用户资源限制：最多 2 线程、只跑受影响包）

- `cargo fmt --all -- --check`：通过。
- `cargo clippy -p lumark-render -p lumark-unreal -p lumark-ui -p lumark-archive -p lumark-legacy --all-targets -j 2 -- -D warnings`：通过（仅 vendor `wim` 既有 dead_code 警告）。
- `cargo test -p lumark-render --lib -j 2 -- --test-threads=2`：55 passed。
- `cargo test -p lumark-unreal --lib -j 2 -- --test-threads=2`：102 passed。
- `cargo test -p lumark-ui --lib -j 2 -- --test-threads=2`：218 个用例；完整运行 217 passed、唯一失败的可拖拽检查器用例修复后 `viewer_resizable_inspector_sits_right_of_canvas` 定向通过；查看器相关 28 个用例定向通过。
- `cargo test -p lumark-legacy --lib -j 2 -- --test-threads=2`：51 passed。
- `cargo test -p lumark-archive --test zpaq --no-run -j 2`：编译通过（删除大文件用例后；按用户要求未跑完整 archive 套件）。
- 完整 workspace 套件按用户要求暂停，不再重跑；release 构建同样未在低资源窗口执行。
## 2026-11-04 Snooper 全效果（P0/S-1）

- [x] P0-1 质量切换不跳：`fit_bounds` 固定为全部 LOD 并集；`logical_height=600` 让 pan 不随内部渲染分辨率变化；低/高分辨率相机矩阵一致性测试通过。
- [x] P0-2 空白模型：默认隐藏规则会隐藏全部 section 时自动全显并提示；无材质 StaticMesh 测试通过。
- [x] P0-3 预览一致：预览缓存 key 加入 `SNOOPER_PRESET_VERSION`；查看器/缩略图共享 `snooper_viewer`。
- [x] S-1 环境/API：新增 `SnooperEnvironmentSettings`/`SnooperEffects`/`ShadowSettings`/`SsaoSettings`/`SkySettings`/`FogSettings`/`GroundSettings`；`snooper_viewer` 按 Snooper 共享预算解析太阳/天空色，全效果默认开启（渲染器暂未消费）。
- [ ] S-2…S-9：见 `docs/snooper-renderer.md`；下一步实现 GPU G-buffer/阴影/SSAO/天空/雾/地面。

### 本轮验收

- `cargo test -p lumark-render --lib -j 2 -- --test-threads=2`：51 passed。
- `cargo test -p lumark-ui --lib app::view::asset_viewer_window::tests:: -j 2 -- --test-threads=2`：26 passed。
- `cargo fmt --all -- --check`、受影响包 clippy：通过。
- BNB 实机用例通过；效果尚未接入渲染器，截图仍为近似 Snooper 光照。
## 2026-11-04 Snooper 全效果阶段进展

- [x] 天空球蓝渐变 + 太阳盘（GPU 背景）
- [x] 指数高度雾（GPU/CPU，按 UE 厘米单位换算，工具栏可开关）
- [x] 无限深色地面 + 接触软阴影（`snooper_extra` 跳过模型 section/material 隐藏）
- [x] Snooper FXAA-lite（GPU 回读后处理）
- [x] 效果开关、预览缓存版本、详情缩略图地面/天空/雾一致
- [ ] 实时级联阴影
- [ ] SSAO

验证：`lumark-render --lib` 56 passed；viewer 定向 26 passed；预览 11 passed；BNB NanaF6_2 实机用例通过。
## 2026-11-05 Snooper 全效果验收

- 两级联太阳阴影 + 3×3 PCF；SSAO 深度重建后处理；方向天空/太阳盘；cm 单位指数高度雾；5000 单位地面+网格；Snooper FXAA-lite。
- 查看器默认 1.5×（1350×900），工具栏可切 1.0×/1.5×/2.0×；效果开关、预览缓存版本、缩略图背景一致。
- P0 质量切换/全隐藏 section/预览缓存回归测试保留。
- `lumark-render --lib` 59 passed；viewer 定向 26 passed；preview 11 passed；BNB NanaF6_2 实机通过。
## 2026-11-05 严格对齐 Snooper 验收

- 按 Snooper 源码移植：`atmosphere.glsl` 大气 LUT、`fog.glsl` 高度雾、`fxaa.frag` FXAA、`ssao.frag` 半分辨率 SSAO + 双边模糊、`SunCascades.cs` 4 级联阴影与 `scene_lighting.glsl` 阴影采样。
- CPU 回退：大气 LUT、SSAO、屏幕空间阴影、高度雾、FXAA。
- 缩略图：天空/地面/接触阴影/雾/FXAA 与查看器一致；查看器默认 1.5× 分辨率。
- 验证：`lumark-render --lib` 62 passed；viewer 26 passed；preview 11 passed；BNB NanaF6_2 实机截图。