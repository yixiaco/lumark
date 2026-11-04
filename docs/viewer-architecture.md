# Lumark 查看器渲染架构（V-9 / V-17）

> 更新：2026-11-03（U-1 渲染性能与 FModel 视觉对齐）。对应 V-0…V-6 的实现说明与验证基线。

## 分层

```
lumark-unreal（资产读取/材质解析/材质外观）
        │  Scene（引擎无关数据）
        ▼
lumark-scene：Scene / SceneNode / MeshData（含 MeshSkin）/ MaterialData（含 OrmLayout）
        │
        ▼
lumark-render：CPU 光栅（raster） + wgpu 30 离屏（gpu）
        │  RenderTarget（RGBA8，to_bgra）
        ▼
lumark-ui/render_service：RenderImage（缩略图）；asset_viewer_window 帧泵（V-17）
```

- 引擎无关层不引用 Unreal/GPU/UI；未来 Unity Provider 复用同一 `Scene`。
- 坐标系：UE Z-up（`scene.up_axis`），渲染层直接使用场景坐标；`unit_scale` 仅作记录。

## 公共 API

- `RenderSettings`：宽高、背景、相机轨道参数（yaw/pitch/distance/target/pan）、双灯颜色与强度、环境光、超采样（1–4）；
- V-4/V-6 增补：`shading: ShadingMode { Lit, Unlit, Normals }`、`show_grid`、`show_bones`、`hidden_material_slots`；
- P-7/P-8 增补：
  - `pan_pixels: [f32; 2]`：右键拖拽的屏幕空间平移，相机按像素 1:1 换算世界偏移；
  - `vertex_colors: bool`：是否把顶点色乘到基础色（查看器默认关闭，对齐 FModel）；
- U-1 增补：
  - `lighting_mode: LightingMode { Studio(默认), Headlight, Snooper }`；
  - `sky_upper_color` / `sky_lower_color`（Snooper 天空上下半球环境光）；
  - Snooper 模式使用 `key_direction/key_color/key_intensity` 作为太阳方向、颜色与强度；
  - `normal_map_convention: NormalMapConvention { Ue(默认), Fmodel }`；
  - 默认相机改为 FModel 取景：`fov_y_degrees = 45`、`distance_scale = 1.05`、yaw 35°/pitch 20°；
- `lumark_scene::MaterialData` 新增 `orm_layout: OrmLayout { AoRoughMetal(默认), SpecularMask }`、
  `roughness_min/max`、`specular`、`emissive_multiplier`、`ao_boost_color/exponent/has_ao_boost`；
  `MaterialAppearance` 新增 `two_sided`、`roughness_min/max`、`specular`、`emissive_multiplier`、
  `ao_boost_*`、`shading_model`、`blend_mode`（材质实例继承父材质）；
- `MaterialViewerData` 新增 `emissive` 与 `texture_infos`（贴图名称/用途/解码尺寸），供检查器展示；
- `render_bounds(scene)`：取景包围盒。蒙皮网格按休止姿态的蒙皮顶点统计（大网格按 ≤4096 点抽样），
  修正原 `Scene::compute_bounds` 只看节点变换导致的查看器取景偏移/裁切；
- `RenderTarget`：RGBA8 + `to_bgra()`（GPUI `RenderImage` 用）；
- `Renderer`（CPU）/ `GpuRenderer`（wgpu）均输出 `RenderTarget`，上层可统一回退。

## 渲染管线

1. **相机**：`Camera::fit(render_bounds, settings)` 拟合轨道相机，透视投影深度 0..1；
   `render_bounds` 遍历全部蒙皮顶点做 LBS 统计，包围盒精确、不再抽样。
2. **批次/实例化**：按「网格 + section + 材质 + 蒙皮节点」合批；静态网格共享本地空间顶点缓冲，
   每个节点只上传实例矩阵（GPU 实例化）；蒙皮网格保留 bind 空间顶点并独占批次。
3. **蒙皮**：`MeshData.skin`（关节/inverse bind/≤4 影响/姿态）；CPU 逐顶点 LBS；GPU 顶点含
   joints/weights，批次上传关节矩阵 storage buffer，WGSL 内 top-4 归一化混合。
4. **着色**：基础色/法线/ORM/自发光贴图；alpha < 0.5 裁剪；单面材质剔除背面、双面材质保留。
5. **叠加层**：地面网格、骨骼线段与线框边（LineList 管线，深度测试不写深度）；CPU 用 DDA 3D 线段光栅。
6. **输出**：颜色目标 `Rgba8Unorm`；Snooper 模式与 `mesh.frag` 一致只做 gamma，Studio/Headlight 做 Reinhard + gamma；超采样后盒式降采样回目标尺寸。
   GPU 背景由全屏三角形绘制渐变，与 CPU 垂直渐变一致。

## 光照模型

| 模式 | 说明 |
|---|---|
| `Studio`（默认） | 双方向光（主光 + 反方向补光）+ 环境光。保持 U-1 之前的查看器/缩略图观感，既有测试不回归 |
| `Headlight` | 严格对齐 FModel `default.frag`：`diffuse = max(N·V, 0.4) * base`；AO.R 只衰减基础色，AO.B 驱动 Skin Boost，AO.G 作腔体；specular mask 以 G=金属度/腔体、B=粗糙度 `mix(RoughnessMin, Max, B)`；自发光只叠加贴图 × 倍率；最后加 Schlick+GGX+Smith 头灯高光并做 Reinhard + gamma。不做法线朝视线的翻转 |
| `Snooper`（查看器默认） | 对齐 Snooper OpenGL `scene_lighting.glsl`：`F0 = mix(0.08*Specular, albedo, Metallic)`，太阳 `diffuse/specular` + 天空上下半球 `mix(lower, upper, N.z*0.5+0.5) * albedo * AO`；`R=Specular/G=Metallic/B=Roughness`，不做 Reinhard，只做 gamma |

查看器默认 `Snooper + Fmodel`；详情缩略图仍使用 `Headlight + Fmodel`；`Studio`/`Headlight` 保留兼容与回归测试。
Headlight 刻意忽略 `orm_layout`（FModel 只支持 specular mask 语义）；`Studio`、材质球 Studio 与导出仍按 `orm_layout` 解释 ORM/高光遮罩；Snooper 的材质 AO 用作 SSAO 近似。

## 法线约定与 ORM 布局

- **法线约定**：`Ue` 为既有行为（切线正交化，副切线 `cross(N, T) * sign(w)`）；
  `Fmodel` 对齐 FModel Snooper（副切线固定 `-cross(N, T)`，忽略 `w`）。查看器默认 `Fmodel`。
- **ORM 布局**：
  - `AoRoughMetal`：R=AO、G=粗糙度、B=金属度（默认）；
  - `SpecularMask`：G=金属度、B=粗糙度、R 为 AO 通道（FModel specular mask 语义）；
  - `lumark-unreal` 按金属/粗糙度贴图命名自动识别（含 `specular`/`_sm` 等 → `SpecularMask`），
    并由 `scene_material()` 写入 `MaterialData::orm_layout`。
- GPU 用 `camera.ambient.z/w` 传入光照模式与法线约定，`material.flags.z` 传入 ORM 布局；
  CPU 与 WGSL 两侧公式保持一致。

## 异步帧泵（V-17）

- UI 线程只做 **Arc 快照**（`RenderSnapshot`：场景 Arc + 相机/开关参数），场景构建与离屏渲染全部在
  `background_executor` 的 worker 内完成；骨骼/材质查看器的轻量光栅也走 worker；
- **单飞行帧 + dirty 合并**：同一时刻只允许一帧在渲染，拖拽/滚轮/工具栏只置 dirty；一帧完成后若有
  新输入，立刻用最新状态再渲染一帧；不再为每个鼠标事件 spawn 排队；
- **两档画质**：交互帧 `supersample=1`；静止 180ms 后自动补一帧 `supersample=2 + 4×MSAA`；
- 动画播放循环只推进时间并调用 `request_preview`，同样受单飞行帧合并；
- 骨骼/材质查看器的轻量光栅不再阻塞 UI 线程；
- 帧泵状态机 `frame_pump.rs` 为纯逻辑（不依赖 GPUI/GPU），有独立单测覆盖合并、静止计时与重排。

## GPU 性能（U-1）

1. **Batch 不再每帧克隆材质/贴图**：批次只保存场景材质索引，绘制时按索引借用 `MaterialData`；
   贴图走 64 项 LRU（名称/尺寸/格式/内容指纹）；
2. **回读只 poll 一次**：提交后登记 `map_async`，单次 `device.poll(wait)` 同时等待 GPU 与映射完成；
3. **渲染目标复用**：颜色/深度/多重采样目标与回读缓冲按 `(宽, 高, MSAA 采样数)` 缓存，
   尺寸不变时跨帧复用，避免每帧分配大纹理与 staging 缓冲。

## 材质图还原（U-1 续）

- **材质槽对象引用**：`SkeletalMaterials` 每条记录首字段是 MaterialInterface 的 `FPackageIndex`
  （负值为包导入），`skeleton.rs` 收集为 `material_imports`；`resolve_material_slots_exact`
  用网格包的 ImportMap 把导入解析为 `(目标包 ID, 公共导出哈希)`，再按包 ID 读取材质包，
  不再依赖「槽名 ≈ 材质资产名」的模糊匹配（模糊匹配仅作没有对象引用时的兜底）。
- **材质图求值**（`material_graph.rs`）：
  > cooked UE5 包会剥掉编辑器侧的 `Expressions` 数组，因此这里不解析材质表达式图，
  > 而是读取烘焙后仍保留的参数属性（`TextureParameterValues` 等）、父材质链与引用贴图；
  > 这正是 FModel 对 cooked 材质同样使用的数据来源。
  1. `TextureParameterValues` → 贴图对象引用 → 目标包/导出 → `MaterialTextureRef.package_id`，
     贴图按包 ID 直接解码，不依赖同目录命名；
  2. `Parent` → 父材质包/导出 → 递归读取并按「子覆盖父」合并（父链深度上限 8）；
  3. `ReferencedTextures`（属性或 `CachedExpressionData` 结构内）作为兜底；
  4. **贴图用途选择对齐 FModel `CMaterialParams2`**：参数触发名列表（`Color`/`Diffuse`/
     `Normal`/`ORM` 等）优先 → 参数名/贴图名正则（`_D`/`_N`/`_SM` 等）兜底 →
     基础色最后回退到材质链第一张贴图；`RGB`/`ObjectColor`/`_ID`/`DecalColor` 等遮罩类
     参数不会被当成基础色；
  5. **tint 与 AO**：基础色 tint 取 FModel `DiffuseColors` 触发名（默认白色），
     `M`/`AEM`/`AO` 参数解析为环境光遮蔽贴图，GPU/CPU 着色时按 FModel 规则乘到基础色。
- **alpha 裁剪与半透明混合**：读取 `BlendMode`/`BasePropertyOverrides.BlendMode` 得到 Masked/Translucent；
  Masked 材质按 `MaterialData.alpha_cutoff` 裁剪（Opaque 为 0），若基础色贴图 alpha 几乎全透明
  则视为颜色遮罩、不做裁剪，避免整块布料/皮肤被误裁；Translucent 材质走 GPU 混合管线
  （关闭深度写、`ALPHA_BLENDING`）并按距相机由远到近排序，CPU 光栅对应地把半透明片元
  收集后按深度由远到近混合。
- **查看器/缩略图/示例共用**：`ResolvedMaterial`（含贴图）随 `ViewerScene` 传入查看器，
  场景由 `static_mesh_scene`/`skeletal_mesh_scene` 构建，UI 预览缩略图同样使用带贴图场景。

## 三角形绕序与单面几何

- UE 使用左手坐标系、正面为顺时针；`lumark-render` 与 glTF 一样按右手系 **CCW 正面 + 背面剔除**。
  FModel 在 `Views/Snooper/Models/UModel.cs` 把顶点 `(X,Y,Z)` 换成 `(X,Z,Y)`（Y/Z 交换完成手性转换）
  并保持索引顺序，OpenGL 默认 CCW 正面，因此显示的是几何外侧。
- `lumark-unreal::scene::convert_mesh` 不改变坐标系，改为按三角形交换第 2/3 个索引
  （`[a,b,c] → [a,c,b]`），等价完成同一手性转换；索引数量与 section 结构不变。
  这样叶片从上方可见、人物头部不再透视内壁，CPU 光栅（无剔除）不受影响。
- 已知边界：世界坐标相对 FModel 仍左右镜像；GLB/USDA 导出器的索引绕序尚未同步，
  属于后续单独修复项。

## 静态网格材质引用

- UE5 `StaticMaterials` 每项包含 `MaterialSlotName` 与 `MaterialInterface`（`Object` 引用）。
  旧实现只取槽名，Zen 静态网格的槽名常是 `05 - Default` 这类占位文本，启发式匹配失败后
  材质回退默认灰白（白模）。
- `read_asset_metadata`/`read_static_mesh` 现在同时输出 `material_imports`（`FPackageIndex`），
  `resolve_static_materials` 在存在导入引用时走 `resolve_material_slots_exact`（与骨骼网格
  `material_imports` 相同的精确解析），没有引用时保持槽名/目录启发式以兼容旧式包。
  详情预览、Provider、世界场景与导出统一走该入口。

## 贴图 mip 与内存

- 贴图解码按查看器目标边长选 mip：`mip_level_for_limit` 计算最接近 `max_texture_size`
  的 mip 级，`Texture2D::decode_mip_rgba_with_size` 解码该级并返回该级尺寸。4K 贴图
  缩放到 512 时临时 RGBA 从约 64 MB 降到约 1 MB，同时省去大图最近邻缩放。
- 查看器帧图：`RenderImage::new` 每帧分配新 `ImageId`，GPUI 图集只通过
  `App::drop_image`/`remove_asset` 移除且没有 LRU；因此 `apply_frame` 返回被替换的旧帧，
  由调用方 `drop_image` 释放，避免历史帧常驻图集。
- 详情预览图：UE 预览输出使用按预览 key 哈希的稳定 PNG 路径，替换预览或离开 UE 资产时
  `remove_asset` 并删除旧文件，避免按 UUID 路径累积图片缓存与临时文件。

## 材质球（D-3）

材质查看器与详情预览使用 CPU 材质球（`lumark-unreal/raster.rs`），通过
`MaterialBallSettings` 与场景渲染对齐：

- `headlight`：FModel 头灯 GGX（`max(N·V, 0.4)` + Schlick/GGX/Smith）或 Studio 双灯；
- `fmodel_normals`：FModel 切线约定（副切线 `-cross(N,T)`）或 UE 约定；
- `orm_layout`：`AoRoughMetal`/`SpecularMask` 通道布局；
- AO 贴图、基础色 sRGB→线性、色调映射 + gamma 与场景/查看器保持同一套公式。

## 世界场景（D-4）

`lumark-unreal::provider::load_scene` 识别 `World`/`Map` 导出后转交 `load_world_scene`：
World Actor → StaticMeshComponent → 跨包 StaticMesh；新增 `package_path_for_id` 反查网格
所在容器路径，按 `StaticMaterials[*].MaterialInterface` 精确解析材质（缺引用时回退槽名），
并写入场景节点。加载报告 `asset_class=World`，包含顶点、
三角形、材质数量与告警。实测 BNB `Map/LOGO.umap` 与 `KingS/GinkgoTree.umap`
（178,600 顶点 / 105,348 三角形 / 16 材质）均通过 Provider 加载。

## 查看器默认与交互

- 3D 查看器默认显示地面网格；滚轮向上放大（`zoom` 增大），与 FModel 一致；
- 工具栏按「视图 / 渲染 / 动画 / 检查器 / 属性 JSON」分组，移除 S0…S16 与 LOD 切换按钮（移入检查器），
  统一使用标准 Medium 尺寸，空间不足时自动换行；
- 画布用 `img` + `object_fit(Contain)` 显示渲染结果，外层容器负责 `h_full`/裁剪/尺寸约束，
  图像只在画布范围内绘制；布局测试断言画布不覆盖工具栏且渲染图像容器尺寸正常；
- 左键轨道拖拽与 FModel `Camera.Modify` 对齐：鼠标向右拖拽时 `yaw` 减小（相机绕 `-Up` 轴旋转），
  向下拖拽抬高相机；`apply_orbit_drag` 为纯函数并有方向/边界单测；
- 右键拖拽屏幕平移：`pan_pixels` 累加，`Camera::fit` 按 `2*distance*tan(fov/2)/height` 换算世界偏移，
  场景跟手移动；`F`/“重置视角”同时清零平移；
- 轨道相机 `distance_scale = 1.05 / zoom`，取景包围盒用 `render_bounds`；
- 右侧检查器可拖拽调宽（默认 320px，范围 260–640px），标签为“概要 / 材质 / 骨骼”；
- “材质”页列出 section（含显隐、默认隐藏标记）与选中材质详情：blend/shading、双面、基础色 tint、
  金属度、粗糙度区间、自发光倍率、AO 增强、贴图名称/用途/尺寸、已加载资源与 warnings；
- 默认可见性对齐 FModel `Section.SetupMaterial`：材质缺失/无贴图或半透明 section 默认隐藏，
  顶部提供“全部显示 / FModel 默认”；
- “属性 JSON”由预览链路生成（导出表 + 属性树，4 MB/64 导出上限），打开独立只读窗口，
  支持 JSON 高亮、搜索与复制；
- 状态栏显示批次/顶点/三角形统计与后端标签，“渲染中…”只在后台帧进行时出现。

## 快捷键

| 按键 | 行为 |
|---|---|
| `F` | 重置相机（yaw 35°/pitch 25°/zoom 1×/pan 0） |
| `Space` | 播放/暂停动画 |
| `←` / `→` | 动画逐帧后退/前进 |
| `L` | 着色模式循环（光照→无光→法线） |
| `G` | 地面网格开关（默认开） |
| `W` | 线框开关 |
| `B` | 骨骼线开关 |
| `N` | 世界法线着色 |

正式查看器窗口创建时获取焦点；Ctrl/Alt 组合不拦截。动画时间轴使用官方 `Slider`，拖动/点击即跳转
并暂停播放，播放/步进/重置后自动同步滑块位置。

## 测试与基线

- `lumark-render` 单元测试：**47 个**（CPU：相机裁剪/像素平移/z-buffer/PBR/ORM 两种布局/FModel spec+AO/自发光/alpha/半透明混合/双面/
  蒙皮/着色模式/网格与隐藏槽/场景统计/线框/头灯背面可见/两种法线约定/全顶点包围盒/默认 Studio 不回归；
  GPU：三角形/贴图/ORM/自发光/双面/蒙皮/着色模式/网格/两节点实例化/4×MSAA/sRGB/背景渐变/线框/
  头灯背面/ORM 布局/AO/半透明混合，无 GPU 自动跳过）。
- `lumark-ui` 帧泵：状态机单测 4 个（输入合并、静止计时、飞行帧阻塞、新输入重排）+ 查看器端到端
  “静止后补高质量帧”测试；`orbit_drag_matches_fmodel_direction`、`pan_drag_accumulates_in_screen_space`、
  `viewer_pan_changes_render_and_resets`、`viewer_resizable_inspector_sits_right_of_canvas`、
  `default_sections_hide_missing_or_translucent_materials`、`viewer_applies_resolved_materials`、
  `viewer_paints_rendered_image_element`、`model_viewer_layout_stays_below_toolbar_when_resized`
  （放大窗口后画布不得覆盖工具栏）、图片查看器极端窗口锚点测试、JSON 查看器只读渲染测试。
- `material_graph` 单测：贴图用途分类、`FPackageIndex` 导入索引提取、AO/tint 选择、FModel
  spec mask/AO boost/roughness 区间；`lumark-unreal` 共 **88** 个测试，`lumark-legacy` 19 个。
- `lumark-ui --lib` 共 **187** 个测试全部通过；全工作区 680 条测试通过，0 失败。
  按用户资源限制，最终验收只跑受影响包（最多 2 线程），完整 workspace 套件暂停。
- 实机基线（NVIDIA RTX 5070 Laptop，Vulkan；BNB `NanaF6_Hold`：172 节点 / 135,072 三角形；
  17/17 材质槽通过对象引用解析，贴图按包 ID 解码；900×600，MSAA=1，含离屏渲染 + 回读）：

  | 场景 | SS1 | SS2 |
  |---|---|---|
  | 改前（旧渲染器，默认 fov40 / distance 2.2，Studio 取景偏移） | 438.4 ms | 565.9 ms |
  | 改后首帧（Headlight + Fmodel，fov45 / distance 1.05，取景修正） | 516.0 ms | 652.1 ms |
  | 对照：改后渲染器 + 旧相机/Studio 观感 | 537.6 ms | 744.9 ms |
  | 材质图还原后 900×900 SS2 Headlight+Fmodel（debug 暖帧，frames=3） | — | 694.6 / 746.2 / 710.1 ms |
  | **release 900×900 Headlight+Fmodel（首帧 / 暖帧 avg，frames=3）** | **47.5 / 43.7 ms** | **64.0 / 57.0 ms** |
  | release `render_bounds`（全顶点 LBS，135,072 顶点） | 3.15 ms | 3.21 ms |

  > 说明：早期 debug 基准与其他编译/测试并行运行，单次进程测量波动约 ±15%，仅作回归参考。
  > release 暖帧为 `cargo run --release` 独立进程测量（NanaF6_Hold，172 节点 / 135,072 三角形 /
  > 17 材质，含离屏渲染 + 回读），SS1 首帧 47.5 ms、后两帧 41.5/42.2 ms；SS2 首帧 64.0 ms、
  > 后两帧 52.2/54.9 ms。GPU 侧优化（目标/回读复用、材质不再克隆）主要收益在交互连续帧与
  > UI 线程解耦上；首帧包含管道/纹理上传与回读初始化。

## P-7…P-10 查看器交互与属性 JSON

| 项 | 结论 | 证据 |
|---|---|---|
| P-7 右键平移 | `pan_pixels` + 相机像素→世界换算；右键拖拽/`F`/重置；左键轨道不回归 | `camera_pan_moves_target_by_screen_pixels`、`camera_pan_is_independent_of_zoom`、`pan_drag_accumulates_in_screen_space`、`viewer_pan_changes_render_and_resets` |
| P-8 FModel 材质/光照 | 触发名表按 CUE4Parse `CMaterialParams2`（Apache-2.0）首 UV 层；Headlight 使用 G=金属度/腔体、B=粗糙度、AO R/G/B、RoughnessMin/Max、EmissiveMult、Skin Boost；默认隐藏无贴图/半透明 section | `fmodel_params_use_spec_g_b_and_ao_g`、`fmodel_headlight_ao_r_darkens_result`、`fmodel_headlight_vertex_colors_are_opt_in`、`fmodel_headlight_emissive_requires_texture`、`default_sections_hide_missing_or_translucent_materials` |
| Snooper 色彩/光泽 | `LightingMode::Snooper`：太阳 + 天空上下半球环境光、`F0 = mix(0.08*Specular, albedo, Metallic)`、R/G/B 高光/金属/粗糙通道、AO 近似 SSAO、只做 gamma；检查器新增高光强度 | `snooper_lighting_uses_sky_ambient_and_sun`、`snooper_specular_scalar_changes_highlight`、`snooper_frame_uses_gamma_only_encoding`、`wgsl_shader_parses_and_validates`、`renders_real_nana_f6_2_viewer_frame_when_available` |
| P-9 检查器/UI | 可调宽检查器；概要/材质/骨骼；section 显隐与材质详情；工具栏分组并移除 S0…S16；顶点色开关 | `inspector_panel_renders_and_toggles`、`material_viewer_toolbar_hides_mesh_controls`、`viewer_keyboard_shortcuts_update_state` |
| P-10 JSON 属性 | 导出表 + 属性树；对象引用尽量解析；4 MB/64 导出上限；独立只读窗口（JSON 高亮/复制） | `converts_nested_property_values`、`stops_at_max_depth`、`json_viewer_renders_readonly_editor`、BNB 实机 `renders_real_nana_f6_2_viewer_frame_when_available` |
| BNB 实机 | `NanaF6_2`：`N_EyeLip`/`N_EyeShadow` 默认隐藏，`N_Eye`/`N_Skin` 可见；`N_Eye` 基础色 `eyeBro`，`N_Skin` spec/AO 均为 `NewNana_AO` | `artifacts/viewer-nana-f6-2-fmodel.png`、`artifacts/viewer-nana-eye-base.png`；环境变量 `LUMARK_BNB_GAME_ROOT`/`LUMARK_BNB_MAPPINGS` |

## D 阶段收尾结果

| 项 | 状态 | 结论 |
|---|---|---|
| D-1 材质图与 FModel 对齐 | ✅ | 参数触发名/正则/first-texture 选择、DiffuseColor tint、`M`/`AEM`/`AO` 环境光遮蔽；BNB 17/17 槽位一致 |
| D-2 透明材质排序混合 | ✅ | BlendMode 区分 Masked/Translucent；GPU alpha 混合管线 + 远→近排序；CPU 半透明片元混合 |
| D-3 材质球接入 Headlight/ORM/双面 | ✅ | `MaterialBallSettings` 复用头灯/法线约定/ORM 布局/AO/色调映射 |
| D-4 World Provider 世界场景加载 | ✅ | `.umap` 经 `load_scene` 加载并解析跨包材质 |
| D-5 取景包围盒精确计算 | ✅ | 遍历全部蒙皮顶点，移除抽样误差 |
| D-6 图片查看器锚点像素级校准 | ✅ | 极端窗口下图片/画布中心 ≤1px，极端画布与缩放锚点单测 |
| D-7 release 稳态性能基准 | ✅ | 900×900 SS1 47.5/43.7 ms、SS2 64.0/57.0 ms（首帧/暖帧 avg） |
| D-8 文档限制收口 | ✅ | 全部活动文档改为「支持范围/验证边界」，不再保留未完成限制条目 |

进度与证据见 `docs/plan-progress.md` 与 `IMPLEMENTATION_STATUS.md`。
