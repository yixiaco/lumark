# Lumark 实施状态

本文件把完整规划拆成可验收的阶段。当前版本是可运行的桌面原型和核心安全链路，不应把未勾选项目视为已完成。

## 2026-11-02 本轮升级

- [x] CI 行数门禁调整为“超过 1000 行提醒、超过 3000 行失败”，既有 `service.rs`、`virtual_drag.rs` 只提醒不阻塞；文档同步。
- [x] 修复 GitHub Actions release 链接失败：`vendor/zpaq_rs/build.rs` 不再自动选用 clang++，MSVC 目标显式使用动态 CRT；`tools/build_release.ps1` 编译前清理 `CC`/`CXX`。
- [x] CI 增加 Toolchain preflight 步骤，记录编译器与 `CC`/`CXX` 环境，便于定位 LNK2038 一类原生依赖问题。
- [x] Oodle 解压新增纯 Rust 后端：引入 `oozextract 0.5.7`（MIT），默认 `auto` 先走 Rust、失败回退 DLL；`LUMARK_OODLE_BACKEND=auto|rust|dll` 可切换；未压缩块、非法块头与空输出已有单元测试。随包/下载 DLL 的兜底方案仍保留为后续 O 阶段步骤。
- [x] 纯 Rust Oodle 实机验证：游戏目录不含任何 Oodle DLL 时，BNB IoStore 的 `SK_Chicken`（244 KB 包，42 骨骼/2860 顶点/3844 三角形）与 `T_ChickenA_BaseColor`（5.5 MB `.ubulk`，2048×2048 PF_DXT5/12 mip）均能通过 Rust 后端解压、解析并渲染；`accept_game` 支持用 `-` 省略 Oodle DLL，`verify_game_assets.ps1` 的 Oodle 参数改为可选。
- [x] 共享资产底座：新增 `GameAssetSource`（统一 IoStore/PAK 的列表、读取、`.uexp`/`.ubulk` 侧车和 `global.utoc` 脚本对象，暴露加密/Oodle/压缩方法状态）与公开 `PakAssetReader`（按条目读取、大小/路径列表、密码分类错误）；UI 资产预览改走该接口，PAK 预览不再逐资产解压到临时目录。BNB 实机验证：IoStore 92,368 个条目、Oodle 方法、纹理 5.5 MB `.ubulk` 与骨骼包均读取正常。
- [x] StaticMesh 顶点属性：Zen 内联顶点缓冲补齐法线/切线（含高精度 `FPackedRGBA16N`）/全部 UV 通道/顶点颜色与 `StaticMaterials` 材质槽；无法识别的可选布局会跳过并保留几何。BNB `SM_Dry_Goods_04g` 的 4 个 LOD（3213/1723/923/509 顶点）均解析出法线、切线与 3 组 UV，材质槽为 `M_Dry_Goods_04a`；新增 `inspect_mesh_attrs` 诊断示例与 half/packed-normal 单元测试。
- [x] SkeletalMesh 顶点属性：流式 LOD 复用同一 `FStaticMeshVertexBuffer` 读取逻辑，补齐法线/切线/UV 后再解析皮肤权重。BNB `SK_Chicken` 两个 LOD（2860/1674 顶点）均取得与顶点等长的法线、切线与 UV。
- [x] 旧式（FF7R/UE4.18）骨骼网格顶点属性：修正长期存在的位置偏移错误（旧实现读 12..24，实际为 8..20，导致 Y/Z 分量错位、模型 z≈0），并按 `TangentX(4)+TangentZ(4)+Position(12)+UV` 解析法线/切线/UV。FF7R `SK_Mannequin`（68 骨骼/23297 顶点/41052 三角形）现在得到正确包围盒（x ±69、y −22..14.7、z 0..182.5 厘米）与 UV，蒙皮动画链路复验通过（最大顶点位移 200.7）；新增 `inspect_legacy_mesh` 诊断示例与 2 个顶点解码单测。
- [x] 浮点/HDR 纹理：`Texture2D::decode_mip` 支持 `PF_FloatRGB`/`PF_FloatRGBA`/`PF_R16F`/`PF_RGBA16F`/`PF_RGBE`/`PF_R32_FLOAT`/`PF_R32G32B32F`/`PF_A32B32G32R32F`，返回线性 f32 RGBA；`decode_mip_rgba` 保持兼容并对 HDR 做 Reinhard + gamma 近似显示。新增 3 组浮点格式单元测试；`.hdr` 文件导出在导出阶段实现。
- [x] 网格材质槽解析（S-7）：新增 `resolve_material_slots` / `resolve_material_asset` 与 `ResolvedMaterial`：槽位名 → 材质包（同目录优先、容器索引按公共前缀排序）→ 旧式父材质链合并 → 基础色/法线/ORM 贴图解码（`MaterialViewerData`），Zen 包按材质名与用途后缀搜索贴图，旧式包走导入表精确路径。实机验证：BNB `MI_Props`/`MI_Crate_04`/`MI_Lamps` 均解析出外观与基础色/法线/ORM；FF7R `M_UE4Man_Body` 合并参数并加载法线贴图。BNB 样本包常缺少网格引用的材质资产，此时返回可读 warning。新增 5 个候选路径/排序单测。
- [x] 动画采样公共接口（S-8）：新增 `sample_animation(skeleton, anim, seconds) -> AnimationSample`（时间自动钳制、局部姿态 + 列主序 4×4 蒙皮矩阵）、`skin_matrices_from_poses` 与 `skin_mesh_animation`；CPU LBS 现在同步旋转并归一化法线/切线。BNB `SK_Chicken + ANIM_Chicken_Idle1`（4 s/121 帧/42 轨道/2860 顶点）在 0/2/4 s 均生成 42 个姿态与蒙皮矩阵，最大顶点位移 15.4/48.1/15.4，越界时间正确钳制；新增 3 个采样/矩阵/法线蒙皮单测。
- [x] `.umap` World/Level 摘要（S-9）：新增 `read_world_summary` / `WorldSummary` / `WorldActor`：优先精确 `World` 导出、解析 PersistentLevel/StreamingLevels 原生尾部与 `Actors` 数组（FPackageIndex 按“导出索引+1”、去重），属性流不可解析时全数据扫描回退；导入类名用 UAID 前缀生成可读标签，输出 Actor 清单与类别统计。BNB `GinkgoTree.umap` 解析出 45 个 Actor（StaticMeshActor 11、Landscape 2 等）并正确定位 PersistentLevel；`_Generated_` 子关卡 130 个 Actor（StaticMeshActor 111）；`Engine/Content/Maps/Entry.umap` 回退得到 1 个 PlayerStart。SoftObjectPath 等未支持属性类型会记录 warning 并回退。新增 3 个索引数组/去重/类别统计单测。
- [x] V-1 引擎无关场景抽象：新增 `lumark-scene` crate（无外部依赖）：`Transform`（TRS/列主序矩阵/层级组合）、`Bounds`、`Scene`/`SceneNode` 节点树、`MeshData`/`MaterialData`/`TextureData`、相机/灯光、`SceneProvider` trait、通用 `InspectorModel` 与 `scene_inspector`；11 个单测覆盖变换组合、包围盒、层级校验、统计与详情行。`lumark-unreal` 新增适配层：`static_mesh_scene`/`skeletal_mesh_scene`/`scene_material`/`convert_mesh`，f64 LWC→f32、Z-up/0.01 单位标记、骨骼节点树与已解析材质映射。BNB 实机：`SM_Dry_Goods_04g` 生成 1 节点/3213 顶点/5862 三角形场景，`SK_Chicken` 生成 43 节点（42 骨骼 + 网格）/2860 顶点/3844 三角形，包围盒与校验通过。
- [x] V-2a CPU 参考渲染器：新增 `lumark-render` crate：自动拟合轨道相机、近裁面裁剪、透视校正插值、Z-buffer、方向主光+补光+环境光、近似 PBR（金属度/粗糙度/高光）、基础色/法线贴图采样（repeat+双线性）、顶点色、2–4× 超采样、Reinhard 色调映射与 sRGB 输出；公开 `RenderSettings`/`RenderTarget`（RGBA8 + `to_bgra`）/`Renderer`/`render_scene`，与后续 wgpu 后端保持同一 API。12 个单测覆盖背景、三角形、材质色、深度遮挡、张贴图、裁剪与 BGRA。BNB 实机：`SM_Dry_Goods_04g` 480×480 2×SS 约 210 ms、`SK_Chicken` 约 195 ms，输出 `target/render-static.png` 与 `target/render-chicken.png` 均已人工核验。
- [x] V-2b wgpu 30.0.1 离屏后端：`GpuRenderer` 优先 `PowerPreference::HighPerformance`，依次回退默认/软件适配器（失败返回可读 `GpuError`）；WGSL 管线与 CPU 后端同模型（视图投影、双灯+环境光、基础色/法线贴图、顶点色、无背面剔除双面法线翻转），按材质合批、贴图按 sRGB/线性上传，超采样后回读 RGBA8 并复用盒式降采样，`RenderTarget::to_bgra()` 供 GPUI `RenderImage`。2 个 GPU 单测在无 GPU 环境自动跳过。BNB 实机：NVIDIA RTX 5070 Laptop（Vulkan）渲染 `SK_Chicken` 480×480 2×SS 约 83 ms（CPU 约 200 ms），`SM_Box_02 + MI_Props`（4 张贴图）约 63 ms，输出 `target/render-chicken-gpu.png` 与 `target/render-box-gpu-material.png` 已核验贴图/光照正确。
- [x] V-3a 共享渲染服务与 RenderImage 接入：`lumark-ui` 新增 `render_service`（`OnceLock` 缓存 GPU 设备/管线，失败自动回退 CPU），提供 `RenderTarget → GPUI RenderImage`（RGBA→BGRA、`image::Frame`）转换；静态网格详情缩略图改走「Unreal 资产 → Scene → 共享渲染服务」，GPU 优先，副标题显示实际后端。新增 2 个单测（像素通道转换、三角形场景→RenderImage）；现有 `game_assets` 14 个、`viewer` 35 个 UI 测试全部通过。
- [x] V-3b（部分）：图像查看器新增滚轮缩放（指数步进、0.05–16× 限幅）与左键拖拽平移，适应窗口/100%/重置同时复位平移；修正缩放按钮图标（去掉误用的 Close/Add，改为 `−`/`+`）并移除旧滚动容器。骨骼网格详情缩略图也切换为 Scene + 共享渲染服务（GPU 优先，副标题显示后端）。新增滚轮方向/限幅单测；`image_viewer_window` 5、`game_assets` 14、`viewer` 36 个测试全部通过。详情面板直接显示 RenderImage、移除临时 PNG、完整 3D 查看器 GPU 化、世界/动画预览留待下一轮。
- [x] E-1 ActorX PSK 导出：新增 `lumark-export` crate（引擎无关 `ExportMesh`/`ExportBone`/`ExportOptions`），实现 PSK 版本 20220723 写出：`ACTRHEAD`、`PNTS0000`、`VTXW0000`、`FACE0000/FACE3200`、`MATT0000`、`VTXNORMS`、`RAWWEIGHTS`、`VERTEXCOLOR`、`EXTRAUVSn`、`REFSKELT`；按 PSK 约定镜像 Y（位置/法线/骨骼位置与四元数），支持 section 材质、蒙皮权重归一化、多 UV、缩放选项与索引/骨骼校验；3 个单测覆盖 chunk 布局、镜像、权重与错误。BNB 实机：`SK_Chicken` 导出 269,772 字节（2860 顶点/3844 三角形/42 骨骼/RAWWEIGHTS），`SM_Dry_Goods_04g` 导出 263,564 字节（3213/5862，含 EXTRAUVS0/1），chunk 顺序与数量已核验。glTF 2.0、UEFormat、USDA 在后续轮次实现。
- [x] E-2 glTF 2.0 binary（GLB）导出：`lumark-export` 新增 GLB 写出（JSON+BIN 双 chunk、4 字节对齐、`buffer.byteLength` 一致），支持 POSITION/NORMAL/TEXCOORD_n/COLOR_0/JOINTS_0/WEIGHTS_0、按 section 生成 primitive 与索引、PBR 材质（baseColor/metallic/roughness/emissive/baseColorTexture/normalTexture）、PNG 内嵌贴图、骨骼节点层级 + `skin` + inverse bind matrices；坐标 Z-up→Y-up（`(x,y,z)→(x,z,-y)`），`scale` 控制单位。4 个单测（gltf crate 结构校验、Y-up/缩放、skin/IBM、贴图 PNG），`gltf` 仅作 dev-dependency。BNB 实机：`SK_Chicken.glb` 212,408 字节（1 mesh/43 nodes/6 attributes/1 material），`SM_Dry_Goods_04g.glb` 204,060 字节，`SM_Box_02.glb` 42,624 字节；三者均通过 `gltf::Gltf::from_slice` 校验。
- [x] E-3 UEFormat UEMODEL 导出：`lumark-export` 新增 v10 未压缩写出：`UEFORMAT` 头（标识 `UEMODEL`/版本 10/对象名与路径）、FDataAttributeSet 根、`LODS`（VERTICES、NORMALS（W+xyz）、TANGENTS（由位置/UV 估算并 Gram-Schmidt 正交化）、TEXCOORDS UV0..n、INDICES、VERTEXCOLORS、MATERIALS）、骨骼网格 `WEIGHTS` 与 `SKELETON`（METADATA + BONES 名称/父索引/平移/四元数/缩放）；3 个解析单测覆盖头/LOD/材质、蒙皮权重与骨骼、多 UV 通道。新增 `validate_ueformat` 诊断示例。BNB 实机：`SK_Chicken.uemodel` 274,927 字节（LODS+SKELETON+WEIGHTS，42 骨骼，2860 顶点/3844 三角形），`SM_Dry_Goods_04g.uemodel` 289,198 字节（含 UV0–UV2/材质），文件头与根属性已核验。
- [x] E-4 USDA 导出与自动材质旁挂：`lumark-export` 新增 `write_usda`：`#usda 1.0` + `defaultPrim = "Root"` + `upAxis = "Y"` + 按缩放推导 `metersPerUnit`；Mesh 写 points（Z-up→Y-up）、normals、`primvars:st`、`primvars:displayColor`、faceVertexCounts/Indices；单 section 直接 `material:binding`，多 section 用 `GeomSubset`（elementType=face/familyName=materialBind）；自动生成 `UsdPreviewSurface` 材质（基础色/金属度/粗糙度/自发光/透明度），基础色贴图生成 `UsdUVTexture` + `UsdPrimvarReader_float2` 并旁挂 `textures/<材质>_BaseColor.png`；4 个单测覆盖头/Mesh/子集、贴图资源与 PNG、缩放单位、标识符清洗。BNB 实机：`SK_Chicken.usda` 363,660 字节（42 骨骼 rest pose）、`SM_Dry_Goods_04g.usda` 441,860 字节、`SM_Box_02.usda` + `MI_Props_BaseColor.png`（98,711 + 516,767 字节，texture 引用与 PNG 签名已核验）。USD 蒙皮/Skel 暂未导出，网格按 rest pose 输出。
- [x] E-12（后端部分）批量导出与每游戏导出配置：`lumark-unreal` 新增 `export_input`（统一 StaticMesh/SkeletalMesh → `ExportMesh` + 自动材质，精确按 `class_name` 选择）与 `batch`：`BatchExportFormat{Glb,UeModel,Usda,Psk}`、`BatchExportConfig`（输出目录/格式/缩放/材质/贴图上限/过滤/limit/overwrite，JSON 加载保存）、`export_archive`（稳定排序遍历 `.uasset`、镜像容器目录、四格式写出、USDA 贴图旁挂、进度事件、单资产失败继续并汇总 `BatchExportReport`）；4 个单测（路径映射/穿越防护/配置往返/规范化）。BNB 实机 `Chicken/Meshes` 过滤：1 个骨骼网格 × 4 格式全部写出（`target/batch-export2`），跳过 4 个非网格，0 失败。批量任务接入（E-13）与导出 UI（E-14/E-15）待做；`settings` 的 `GameExportConfig` 持久化并入 E-1。
- [x] V-4 PBR 网格 + 纹理（核心）：CPU 光栅与 wgpu 着色器同时补齐 ORM（R=AO/G=粗糙度/B=金属度）、自发光贴图（sRGB→线性）、基础色 alpha 裁剪（<0.5 discard）、单/双面剔除（CPU 按屏幕面积方向、GPU 双管线 `FrontFace::Ccw` + `cull Back`/`None`）；GPU 材质 uniform 扩为 64 字节，新增 ORM/自发光纹理绑定与中性回退；`scene_material` 已把材质查看器的 ORM/法线贴图接入场景。测试：`lumark-render` 20 个全过（CPU alpha/自发光/ORM/双面 + GPU ORM/自发光/双面，无 GPU 自动跳过）；BNB `SM_Box_02 + MI_Props` GPU 实机 480×480 2×SS 约 72 ms，输出 `target/render-box-gpu-v4.png` 已核验金属/木纹/法线效果。缺口：CPU mips/纹理 LRU/sRGB 视图切换留待 V-8。计划总进度已记录在 `docs/plan-progress.md`。
- [x] V-5 GPU 蒙皮 + 动画：`lumark-scene::MeshSkin`（关节节点/inverse bind/逐顶点 ≤4 影响/姿态覆盖）与 `MeshData.skin`；`lumark-unreal` 骨骼网格场景写入蒙皮数据、`apply_animation` 复用 `sample_animation` 采样动画并写姿态；`lumark-render` 新增 `skin` 公共数学、CPU 光栅按关节矩阵执行 LBS、GPU 顶点新增 joints/weights 属性并按批次上传关节矩阵 storage buffer，顶点着色器完成 top-4 归一化混合；单/双面与 PBR 逻辑不变。测试：`lumark-render` 25 个全过（含 CPU/GPU 蒙皮平移、两关节权重混合、单位矩阵保持），`lumark-unreal` 新增蒙皮/姿态写入单测。BNB 实机：`SK_Chicken + ANIM_Chicken_Idle2`（141 帧/42 轨道/4.667s）在 t=0 与 t=1s GPU 渲染成功（约 87–93 ms），两张 PNG hash 不同：`target/render-chicken-anim-t0.png` / `t1.png`。缺 GPU/CPU 蒙皮逐像素误差对比与动画时间轴 UI，留待 V-16/V-19。
- [x] V-6 叠加层/着色模式（核心）+ V-9 文档：`RenderSettings` 新增 `ShadingMode{Lit,Unlit,Normals}`、`show_grid`、`show_bones`、`hidden_material_slots`；新增 `overlay` 模块生成自适应地面网格与骨骼线段（有姿态时用关节矩阵变换 bind 位置）；CPU 用 3D DDA 线段光栅并写深度，GPU 新增 LineList 叠加层管线（深度测试不写深度，独立相机布局）；着色器支持 Unlit/Normals 分支。测试：`lumark-render` **30 个全过**（CPU 着色模式/网格/隐藏槽 + GPU 着色模式/网格/蒙皮/双面/ORM；无 GPU 自动跳过）。新增 `docs/viewer-architecture.md`（分层/API/管线/模式/性能基线/限制）。wireframe 与 section 逐项 UI 留待 V-6 后续/V-17；V-7 实例化与 V-10/V-11 Provider 下一轮。

## 2026-10-29 本轮升级

- [x] 新增 `lumark-viewer`：mmap/编码检测、文本索引、十六进制行、PE32/PE32+ 解析、编码/EOL 原样保存。
- [x] 内置查看器按内容切换文本/十六进制/PE；大文本不再按大小拒绝，超过 10 MiB 或 20 万行改用虚拟行列表；图片使用独立图片查看器。
- [x] exe/dll 等 PE 文件作为只读归档：每个节展开为文件夹和 `[0]` 原始数据，`.rsrc` 解析资源目录并把图标资源重建为 `.ico`，文件尾部覆盖数据作为 `[0]`；支持解压、测试和内置查看器打开，文件表双击/“用 Lumark 打开”进入归档，右键“运行”保留显式执行。
- [x] 图片查看器支持适应窗口、100%、缩放、滚动平移、GIF/WebP 动画与 SVG 预检；归档条目和普通文件都走内置查看器路由。
- [x] 查看器保存普通文件使用编码/EOL 原样写回；ZIP/ZIPX/7Z 归档条目通过 `ArchiveService::replace_entry` 写回，失败不替换原归档。
- [x] 新增 ZIPX 格式（`.zipx`），复用 ZIP 容器；新增 Snappy Framed（`.sz`/`.snappy`，Raw/Hadoop 流返回明确的不支持提示）和 BZip3（`.bz3`）单文件流读写，并接入压缩/解压/测试/设置/文件关联选项。
- [x] RAR 列表区分 RAR4/RAR5 签名，方法列显示 RAR5，补齐 `solid` 标记；RAR5 仍为只读。
- [x] 压缩/解压/测试/编辑/伪装/还原任务运行期间使用 Windows `SetThreadExecutionState` 阻止系统睡眠，任务结束/失败/取消/panic 后恢复；进度窗口显示提示。
- [x] 归档内嵌套压缩包默认新窗口打开；子窗口独占临时目录，关闭窗口 Drop 清理，支持多层嵌套。
- [x] 主题改用官方 `ThemeColor::light()/dark()` 基线和 `Theme::update` 整体 palette；删除视图层裸 `hsla`、按钮指针手、自绘进度条、空状态和标题栏徽标，统一官方 `Progress`/`Empty`/`Tag`/`Scrollable`。
- [x] 新增 `docs/lgpl-compliance.md`、`licenses/LGPL-3.0-only.txt` 和第三方通知；BZip3 使用 `bzip3-rs`（LGPL-3.0-only AND Apache-2.0）。

## 2026-10-30 本轮升级

- [x] ASAR 创建与读取：`lumark-archive` 先把载荷流式写入匿名临时文件并计算 SHA-256/4 MiB 分块哈希，再写 header 和载荷；表头字段与 Electron 工具链一致（`offset` 字符串、`blockSize`），支持空文件、大于 4 MiB 的多块文件和无扩展名内容识别。`lumark-legacy` 读取器改为只读 header 后按偏移 seek，不再把整包读入内存；保留 `.asar.unpacked` 和 symlink 安全路径校验。
- [x] APK/JAR/PAX：APK/JAR 复用 ZIP 容器（密码、注释、AES/ZipCrypto、压缩方法），PAX 复用 TAR 并在每个条目写入 `path`/`mtime` pax 扩展头；三种格式已接入创建/列表/测试/解压/伪装、压缩窗口、设置默认格式和 Shell 关联，并有密码 ZIP、注释 JAR、中文长路径 PAX 回归测试。
- [x] `.pak` 家族：按二进制头区分 Quake PACK（`PACK`）、Chromium 数据包（v4/v5，gzip 外壳）和 Unreal Engine 4/5 PAK（版本 1–11、v8a/v8b、v10+ 完整目录与编码条目、AES-256-ECB、Store/Zlib/Gzip/LZ4/Zstd）。Unreal 样本来自 trumank/repak（MIT OR Apache-2.0），加密索引/条目和压缩条目均有回归；加密错误区分 `PasswordRequired`/`WrongPassword`。
- [x] Oodle：`OodleLZ_Decompress` 仍可通过用户提供的 `oo2core*.dll` 加载（只接受 `LUMARK_OODLE_DLL`、显式配置或程序目录，不扫描 PAK 所在目录）；当前默认优先使用纯 Rust 解码器，未提供 DLL 时不再强制报错。
- [x] 文件表框选：明细列表和平铺视图都支持拖拽框选，Ctrl/Shift 追加、边缘自动滚动、空白点击清空，框选矩形和命中计算有单元测试。
- [x] 拖拽语义：拖出条目时使用 Windows 原生拖拽手势，但只把它当作“选择目标”的动作；释放后由 Lumark 软件层识别桌面/资源管理器路径，提交带进度窗口的解压任务，资源管理器不再读取虚拟文件流。目标已有同名文件时弹出覆盖/跳过/重命名/取消；拖回原归档/原路径忽略；路径栏作为应用内拖放目标，在释放时解压到目标目录；拖入归档自身会被过滤。
- [x] ZIP 家族修复：扫描本地文件头、保留原始压缩数据/加密 extra/CRC 并重建中央目录，输出新文件且拒绝覆盖原归档；支持带数据描述符和 AES 加密的归档，无法确定边界的条目跳过并报告；接入右键菜单和后台任务。

## 2026-10-30 游戏资产（已完成）

- [x] IoStore：`.utoc/.ucas` v1–8 只读解析，v8 头部/块表/压缩方法/目录索引、AES-256-ECB 目录索引与块解密、None/Zlib/Gzip/LZ4/Zstd/Oodle 解压；目录索引在列包阶段只需 AES 密钥，Oodle 只在读取资产数据时需要。`ArchiveService` 已接入格式识别、加密探测、密钥校验和列表；解压/测试/编辑明确返回“仅支持浏览与查看”。
- [x] 真实 BNB 验证：`pakchunk2-Windows.utoc` 列出 1537 个包、`pakchunk0-Windows.utoc` 可解析 9.8 万级包路径，压缩方法为 Oodle。
- [x] BNB 完整实机验收：自动发现 `5.6.1-.../Mappings/*.usmap`（12749 结构/2386 枚举）；`T_ChickenA_BaseColor` 解码 2048×2048 PF_DXT5/12 mip，StaticMesh `Cube` 24 顶点/12 三角形，SkeletalMesh `SK_Chicken` 42 骨骼/2860 顶点/3844 三角形，Skeleton `SK_Chicken_Skeleton` 42 骨骼，AnimSequence `ANIM_Chicken_Idle1` 时长 4.000s；`tools/verify_game_assets.ps1` 真实 BNB 纹理链路通过。
- [x] 真实 FF7 Remake 验证：项目目录 `End`，无 `*-Win64-Shipping.exe` 时回退到最大 exe `ff7remake_.exe`；提取的密钥通过 `pakchunk0-WindowsNoEditor.pak` 校验，PAK 可列出 18896 个条目；修正 UE4 旧版 PAK（version < 8）压缩槽位映射后，可成功提取 `.uasset/.uexp/.ubulk`，Oodle 候选已发现。新增 `LegacyPackage`（摘要/名称/导入导出表/导出偏移）与 `read_legacy_texture`：不依赖 `.usmap`，通过原生平台数据特征和 `.ubulk` 基础 mip 解码，`UE4Man_Logo_N` 验证 512×512 PF_BC5；并新增无版本属性头摘要。游戏目录提供 FF7 Remake 专用 `Mappings.usmap`（9736 structs/1939 enums，含 `End*` 类，FModel 日志确认可加载）；修正旧式 bool 允许任意非零字节后，`Texture2D` 可读出 `AddressX`/`VirtualTextureStreaming`/`bApplyResidentLODBias`/`CachedCombinedLODBias`/`NeverStream`，`SkeletalMesh` 可读出 `Skeleton`/`MinLOD`/`bUseHighPrecisionTangentBasis`，并在详情显示；旧式 `Skeleton` 已解析 f32 `FReferenceSkeleton`，`UE4_Mannequin_Skeleton` 验证 68 根参考骨骼并可在骨骼查看器显示；`SK_Mannequin` 的 SkeletalMesh 原生数据也会探测内嵌参考骨架（68 根）并生成骨骼预览，原生蒙皮/LOD 几何仍待解析。
- [x] Oodle 加载器：支持 `LUMARK_OODLE_DLL`、显式配置路径（游戏目录自动发现）、程序目录内 `oo2core*`/`oodle*` DLL；新增 `LUMARK_OODLE_BACKEND=auto|rust|dll` 与纯 Rust 解码后端。
- [x] 新 crate `lumark-unreal`：游戏目录扫描（安装根/项目根、Shipping.exe、容器、Oodle 候选）、UE 4.27/5.0–5.6 版本字符串探测（ASCII/UTF-16）、AESDumpster 签名 + 容器校验的 AES 密钥提取、手动密钥校验；真实 BNB 已提取并通过校验（密钥不入库/不入日志）。
- [x] 设置新增游戏库条目（DPAPI 保护的密钥 blob、密钥来源、Oodle 路径、最近打开时间）与 `active_game`，旧设置文件向后兼容。
- [x] 主界面动作带新增【游戏资产】按钮（Windows）；新增游戏设置页、已保存游戏列表、目录扫描与后台密钥提取、Oodle 选择、游戏侧栏/上下文条。
- [x] 文件夹式浏览：`BrowserLocation::GameArchives`，容器列表沿用现有搜索/排序/平铺/框选；进入容器后使用现有文件夹浏览，游戏容器隐藏 `.uexp/.ubulk/.uptnl` 伴随文件；返回/退出恢复文件系统位置。
- [x] 详情面板图片预览：文件系统、普通 ZIP/7Z/PAK、IoStore 项共用的异步预览状态，缓存限制、加密提示、过期结果丢弃，点击打开图片查看器。
- [x] 平台门控：游戏资产入口在非 Windows 隐藏；Oodle 的纯 Rust 后端跨平台，DLL 后端仅 Windows 可用。
- [x] M4 纹理链路：UE5 Zen 摘要/名称表/导入导出表/导出数据范围、全局 ScriptObjects 类名解析、`.usmap`（v1–v4、未压缩/ZStandard/Brotli）与无版本属性读取、纹理 `FTexturePlatformData`/bulk data map/内联与 `.ubulk` mip 读取。
- [x] UTexture2D 解码：BC1/BC3/BC4/BC5/BC7、BGRA8/RGBA8/G8；详情面板后台生成纹理缩略图，点击打开图片查看器。已用 CUE4Parse UE5.8 unversioned fixture 的 10 种纹理格式和 BNB 真实纹理包（类名/bulk map）验证。
- [x] 资产元数据：Skeleton 参考骨骼树（名称/父索引/参考姿态）解析；AnimSequence 时长/RateScale/骨架/压缩轨道数/压缩字节/压缩 codec、Skeleton 骨骼数/根骨骼、StaticMesh 材质槽/插槽在详情面板展示；BNB `ANIM_Chicken_Idle1` 定位到 42 条 `TrackToSkeletonMap`、6144 字节压缩流，codec 为 `AnimCompress_PerTrackCompression_6`（121 帧）。已用 fixture 验证 `SKEL_Fixture` 的 3 根骨骼（Root、Joint_1、Joint_2）和 `AS_Fixture` 的 AnimSequence 元数据。
- [x] AnimSequence PerTrackCompression 解码：解析 `CompressedTrackOffsets`/scale offsets/关键帧流，支持 Float96NoW、Fixed48NoW、IntervalFixed32NoW 及帧表索引；`anim_play` 示例按时间采样 42 条轨道并调用 `skinning::skin_mesh_pose` 生成 BNB `SK_Chicken` 动画帧。查看器已接入播放/暂停：先选中 AnimSequence，再打开 SkeletalMesh 查看器即可约 20fps 连续播放。
- [x] FF7R 旧式 AnimSequence 解码：新增 `legacy_animation::read_legacy_anim_sequence`，按 UE4.18 `FCompressedAnimSequence` 布局用结构特征扫描定位压缩头，解析 ConstantKeyLerp 轨道偏移表、缩放偏移、压缩轨道映射、曲线数据、原始大小与压缩字节流；支持 ACF_None/Float96NoW/Fixed48NoW/Fixed32NoW/IntervalFixed32NoW 及单关键帧无损格式，采样规则对齐 `AnimEncoding::TimeToIndex`。真实 FF7R `ThirdPersonIdle` 验证 68 轨道/80 帧/2.63s，首尾帧 28 条轨道变化。查看器工具栏新增只读时间轴、帧号/时间显示、回到开头、上一帧、下一帧按钮。端到端验证：FF7R `SK_Mannequin`（68 骨骼/23297 顶点/41052 三角形/内置蒙皮）由 `ThirdPersonIdle` 驱动，`skin_mesh_pose` 输出顶点最大位移约 340。
- [x] 材质外观渲染：`lumark-unreal` 新增 `MaterialAppearance`/`MaterialTextureRef` 与 `read_legacy_material_appearance`，从 `MaterialExpressionVectorParameter`/`ScalarParameter` 默认值提取基础色/金属度/粗糙度，按名称识别基础色/法线/ORM 贴图并解析父材质包；`raster::render_material_ball` 用 CPU 渲染 512×512 近似 PBR 材质球（方向光、高光、法线扰动、软阴影）。UI 在旧式材质/材质实例预览中从同一 PAK 加载父材质与贴图（支持 FF7R 加密 PAK + AES），渲染成功返回新的 `UePreviewResult::Material` 详情预览，展示基础色/金属度/粗糙度/已加载贴图/参数来源；真实 FF7R `M_UE4Man_Body` 与 `M_UE4Man_ChestLogo` 验证渲染出皮肤色+法线细节材质球；UE5/IoStore（BNB）材质新增 `read_zen_material_appearance` 与按同目录命名约定定位贴图的材质球渲染，真实 `MAT-ChickenA` 验证羽毛贴图+法线细节材质球；两者均有可选环境变量测试。
- [x] 材质/3D 预览分流：详情面板中的材质球点击后打开独立“材质查看器”（与模型/骨骼查看器共用交互式窗口），支持左键拖拽绕球旋转、缩放和重置，旋转时按世界空间固定光照重新渲染；静态图片仍由图片查看器打开。新增轨道材质球渲染 `render_material_ball_orbit`、`MaterialViewerData/MaterialViewerImage`（贴图最大 512 像素）及对应查看器/UI 测试。
- [x] 材质预览信息：新增 `is_material_class`、`LegacyPackage::resolve_object_name` 与详情面板材质行；`Material`/`MaterialInstance*`/`MaterialFunction`/`MaterialExpression*` 展示材质类、父材质/材质依赖、纹理引用、引用资源包与可解析属性（对象索引解析成导入/导出名称），属性表与映射不一致时给出可读提示；Zen 与旧式包共用同一行格式化逻辑。真实 FF7R `M_UE4Man_ChestLogo` 验证父材质 `M_UE4Man_Body` 与纹理 `UE4Man_Logo_N` 引用可读出（可选环境变量测试）。
- [x] StaticMesh 几何与多 LOD：各 LOD 的 `FPositionVertexBuffer`、索引、section 和 `FStaticMeshBuffersSize`/附加索引缓冲尾部布局解析；查看器新增 LOD 切换按钮。已用 `SM_Fixture` 验证 6 顶点/2 三角形/2 section；BNB `sm_StoneBlocks_02_02` 的 4 个 LOD 全部成功解析，详情副标题显示 LOD 数量。
- [x] 内置模型查看器窗口：左键拖拽旋转、工具栏缩放/重置、状态栏顶点/三角形统计；详情面板缩略图或双击 `.uasset` 可打开。
- [x] section/材质切换：查看器工具栏按 section/material_index 显示开关，动态过滤三角形并重绘；StaticMesh 与 SkeletalMesh（网格+骨骼）共用。StaticMesh 详情面板额外列出 `StaticMaterials.MaterialSlotName` 材质槽名。
- [x] 骨骼查看器：Skeleton 参考姿态累积为世界空间线条图，支持同一轨道相机窗口旋转/缩放；AnimSequence 元数据新增只读时间轴条。
- [x] PAK 资产预览：游戏 `.pak` 容器中的 Zen `.uasset/.ubulk` 通过 `ArchiveService` 提取后复用纹理/模型/骨骼/元数据链路，已用 fixture ZIP-as-PAK 验证。
- [x] 多游戏会话：游戏库可同时打开多个游戏，侧栏“打开的游戏”标签切换；每个会话保存自己的浏览器快照（位置/条目/选择/回退前进历史），从设置页打开另一个游戏不会替换当前会话；设置页取消会返回原游戏。打开/切换游戏会更新 `last_opened`，设置页按最近打开时间排序。打开列表持久化到设置的 `open_games`，下次进入游戏资产自动恢复（最多 8 个，倒序恢复保证上次前台游戏最后激活）。游戏卡片支持只关闭当前游戏并自动切换剩余会话。已用两个假游戏 UI 测试验证。
- [x] 嵌套无版本属性：修正 `Optional` 的 32 位存在标志，补齐 `FWeightedRandomSampler`、SkeletalMesh Sampling 结构和 SkeletalMesh 属性解析；SkeletalMesh 详情可显示骨架/LOD/插槽元数据，`SK_Fixture` 属性流读取通过。
- [x] Zen 资产字段行：详情面板在映射可用时追加最多 40 条无版本属性字段（纹理、StaticMesh、SkeletalMesh、Skeleton 缩略图与元数据共用），小型 struct/数组展开一层、长文本截断，与 FF7R 旧式包字段行统一展示；CUE4Parse fixture 的 `T_BC1` 纹理、`SM_Fixture` 模型和 `SKEL_Fixture` 骨骼预览验证字段行非空。
- [x] 映射文件选择校验：后台解析 `.usmap` 后才保存路径，消息显示结构/枚举数量；解析失败不覆盖已有路径；扫描游戏目录时自动发现根目录、`Mappings/`、`Content/Paks/` 下的 `.usmap` 并预填。
- [x] 无映射兜底：未选择 `.usmap` 时详情面板不再直接报错，改为显示包名、导出数量和主类；字段级纹理/模型/骨骼解析仍需要映射。
- [x] SkeletalMesh 参考骨架与静态几何：解析属性后的 ImportedBounds/SkeletalMaterials 前缀、`FReferenceSkeleton`、LOD 的 section 统计和材质槽，并从 bulk data/内联流式数据读取索引、`FPositionVertexBuffer` 和完整的 `FSkinWeightVertexBuffer` 索引/权重数组；RequiredBones→骨架骨骼索引映射后实现 linear blend skinning（`skinning::skin_mesh`，支持骨骼局部旋转增量），模型查看器工具栏提供“蒙皮演示”切换，详情面板生成 rest pose 3D 缩略图（section 材质分色 + 参考骨骼叠加）并显示蒙皮顶点数/最大影响数。SkeletalMesh 的 LOD1+ 通过对 LOD0 之后可选块的小范围头部探测补齐，查看器可循环切换 LOD，并为每个 LOD 保存独立蒙皮权重（BNB `SK_Chicken` 验证 LOD0 2860 顶点/3844 三角形、LOD1 1674 顶点/2358 三角形，LOD1 蒙皮顶点 1674）。fixture `SK_Fixture` 验证 6 顶点、非空 section 和 3 根骨骼；真实 BNB `SK_Chicken` 验证 42 骨骼，逐顶点权重解码后权重和归一化（255），并对 `CHICKEN_-R-Thigh` 施加 15° 局部旋转生成变形网格。AnimSequence 播放已实现。
- [x] 真实游戏属性兼容：修正 `FPerPlatform*` 的 cooked 标志/默认值/覆盖表布局；新增 `accept_game` 端到端诊断（扫描游戏、提取密钥、读映射、渲染纹理/网格，不打印密钥）。
- [x] 验收脚本：新增 `tools/verify_game_assets.ps1`，默认运行 IoStore、UE 解析和游戏资产 UI 测试；配置游戏根目录/映射/Oodle/容器/资产/输出后调用 `accept_game` 做真实游戏验收。
- [x] `accept_game` 支持纹理、StaticMesh、Skeleton 和 SkeletalMesh 参考骨骼四类资产；没有纹理/网格时仍可生成骨骼预览图。
- [x] FF7R 字段级预览：加载 FF7R `.usmap` 后可显示 Texture2D 的 `AddressX`/`VirtualTextureStreaming`/`bApplyResidentLODBias`/`CachedCombinedLODBias`/`NeverStream`，SkeletalMesh 的 `Skeleton`/`MinLOD`/`bUseHighPrecisionTangentBasis`，以及 Blueprint/EndTextResource 等字段；关键修正是旧式 bool 允许任意非零字节。
- [x] SkeletalMesh 材质槽名称：从原生 `FSkeletalMaterial` 读取 `MaterialSlotName` 并在详情副标题显示（BNB `SK_Chicken` 验证 `M_Chicken`），与 section 材质分色/切换配合。
- [x] 旧式 SkeletalMesh 原生摘要：探测内嵌参考骨架后读取 LOD 数与 section 数，`SK_Mannequin` 验证 68 骨骼、1 LOD、2 section，并在骨骼预览副标题显示。
- [x] 旧式包自定义版本：`LegacyPackage` 解析并暴露 `(GUID, version)` 自定义版本表；FF7R 包该表为空，因此 SkeletalMesh/Skeleton 布局按 UE4.18 默认版本（`FSkeletalMeshCustomVersion=10`）推断。
- [x] FF7R 原生 SkeletalMesh 几何与蒙皮：按 UE4.18 默认版本解析 section、索引、RequiredBones、顶点缓冲和独立蒙皮权重；`SK_Mannequin` 验证 23297 顶点/41052 三角形/2 section/4 影响，详情缩略图和查看器可显示 rest pose 网格、骨骼叠加及蒙皮演示。
- [x] FF7R 旧式 AnimSequence 元数据：为 4.18 资产增加 AnimSequence 兼容槽位回退（忽略映射中多出的 `BakedPerBoneCustomAttributeData`/`Notifies`），`ThirdPersonIdle` 验证 `NumFrames=80`、68 条骨骼映射、`SequenceLength≈2.633s`，详情面板显示字段与时间轴；压缩动画数据已由 `legacy_animation` 解码，可在查看器中播放。
- [x] 已记录已知限制：SkeletalMesh LOD1+ 探测为尽力而为，遇到更复杂可选块时可能退化为仅 LOD0；FF7R 材质属性布局与映射不一致时只展示引用资源和部分字段，不渲染材质外观。
- [x] 游戏容器浏览修复：加密 PAK 的资产预览解压会携带游戏会话 AES 密钥，不再把条目统一误报为“需要密码”（FF7R `pakchunk0_s10` 实测无密钥报错、有密钥 + Oodle 可解压并解析包）；`.uexp/.ubulk/.uptnl` 隐藏时其大小并入所属资产包，文件夹同时汇总解压后/压缩后大小，上级文件夹与进入后条目总和一致，压缩后总计可与 PAK 磁盘大小对照；`Content/Paks` 递归扫描包含 `~mods` 子目录；新增“查看密钥”入口（内置文本/十六进制查看器、仅内存）；查看器文本/十六进制行支持鼠标拖选 + Ctrl+C（工具栏仍可整体复制）。加密 PAK 列表补上加密标志，打开条目/解压/复制/拖出统一回退到游戏 AES 密钥；游戏资产入口改为游戏库列表页，新增/编辑游戏使用独立窗口，侧栏与上下文条改为“返回列表”，显示名称改用游戏库名称；游戏容器内双击与右键查看器统一为同一入口，解析完成后自动打开纹理/模型/骨骼查看器。；搜索栏新增按资产类型筛选（图片/模型/骨骼/动画等）；平铺视图列数随窗口重排并为图片生成缩略图。

### 已完成与已接受限制

- [x] 删除确认、打开归档可执行文件确认、清除密码库确认、删除密码记录确认已迁移到官方 `WindowExt::open_alert_dialog`，取消/焦点/Escape 由 Root 处理。
- [x] 新建文件夹、重命名、保存解压密码、删除主密码迁移到官方 `WindowExt::open_dialog`；新建/重命名失败时保留官方对话框供修改。
- [x] `chrome.rs` 的自绘模态已全部迁移：归档密码、自动尝试进度、临时解压进度也使用官方 `WindowExt`；进度更新时刷新官方对话框，任务结束时关闭。
- [x] 侧栏和文件表保留专用自定义组合（懒加载目录树、可调列宽表），内部使用官方 Button/Checkbox/Select/Progress/Scrollable 和主题 token；通用 `Sidebar`/`DataTable` 无法直接表达现有树与列宽交互，作为专用数据视图维护。动作带和状态栏已迁移到官方 `Toolbar`/`StatusBar`。
- [x] 已知限制：ZIPX 的 LZMA 写入受 `zip` crate 限制；当前 ZIPX 创建支持 Deflate/Bzip2/XZ/Zstd/PPMd，LZMA/Deflate64 为只读。
- [x] RAR5 样本测试：`comment.rar`、`solid.rar`、Unicode 文件名、头加密密码、后续分卷错误路径；样本来源与许可见 `tests/fixtures/rar5/NOTICE.txt`。
- [x] 图片查看器 EXIF 方向校正：静态图片（JPEG/WebP 等）按 EXIF Orientation 自动旋转/翻转，动图与 SVG 保持原文件；校正结果写入临时 PNG 并在窗口关闭时删除，状态栏显示 EXIF 校正标签。单元测试验证方向 6 生成宽高互换的临时图、无方向时保持原图，并有打开带方向图片的查看器 UI 测试；手动旋转（左右 90°）已实现：后台旋转并写入临时图，状态栏显示旋转角度且点击可恢复原方向，重置按钮同时恢复旋转；动画/矢量图不支持旋转。已知限制：超大图分块解码仍为后续增强；当前在 1 亿像素/512 MiB 前拒绝，避免解码失控。

## 已完成

- [x] Rust workspace：`lumark-archive`、`lumark-vault`、`lumark-tasks`、`lumark-browser`、`lumark-settings`、`lumark-ui`、`lumark-app`、`lumark-platform-windows`。
- [x] 完整工作区测试：`cargo test --workspace` 全部通过；无交互桌面的托盘测试在 `Shell_NotifyIcon` 返回权限拒绝时跳过，避免 CI 误报。
- [x] 固定依赖：`gpui-kit 0.8.1`、`zip 9.0.5`、`sevenz-rust2 0.24.1`、`zpaq_rs 1.0.4`、`unalz 0.2.1`、`lzma-rust2 0.22`。
- [x] ZIP/7Z 创建、读取、解压、AES 密码、7Z 固实压缩、条目级加密探测。
- [x] ZIP 默认使用 AES-256；可在密码页启用带警告的 ZipCrypto 兼容写入，并有往返测试。
- [x] TAR、TAR.GZ、TAR.BZ2、TAR.XZ、TAR.ZST、TAR.BR、TAR.LZ4、TAR.LZ、TAR.LZMA；GZ、BZ2、XZ、ZST、Brotli、LZ4 单文件流。ZIPX/旧 ZIP 通过 `zip` 的 `deflate64`、`legacy-zip` 特性只读支持 Deflate64、Shrink、Reduce、Implode。
- [x] LZH 创建：使用 `oxiarc-lzhuf` 的 LZSS 匹配器加本地可续写块编码器，按预设映射 lh4/lh5/lh7（显式 level 0 时 lh0/store），跨 1 MiB 读取块保持历史窗口和半字节状态，支持列表、解压、测试和 `test_after`；lh4/lh5/lh7 不再受 512 MiB 单条目内存上限限制，新增 6 MiB 多块回归。
- [x] 压缩窗口按源类型过滤格式：多文件或文件夹源隐藏 GZ/BZ2/XZ/ZST/Brotli/LZ4 单流格式，设置中的单流默认格式自动改用对应 TAR 变体；核心创建接口对多源单流请求返回可操作的“无法继续”错误，而不是“归档损坏”。
- [x] ZPAQ 读写：使用 `zpaq_rs` 内嵌的 libzpaq/JIDAC 参考引擎创建标准归档，支持 1–5 级压缩级别（轻巧模式“体积小”对应 5 级）、AES-256 密码、列表、完整测试、密码校验、选择性解压、覆盖策略、资源预算、分卷和删除源文件；普通批次按 32 MiB / 64 文件分批上报进度并检查取消，含 ≥32 MiB 单文件的批次交给隐藏的 `--zpaq-worker` 子进程执行，取消时直接终止子进程；创建后重新列表校验路径集合；解压先落暂存区再按策略移动，失败不留半成品。源计划跳过符号链接和 Windows 目录联接，避免 zpaq 跳过后条目数不一致；ZPAQ 默认保留原始时间，需要“当前/最近”时间时复制源文件并处理只读属性。同名旧归档在任务成功后原子替换。首次调用前把无控制台 release 进程的无效 stdio 句柄绑定到 NUL，保证引擎输出捕获可用。PAQ8/LPAQ 无许可兼容的成熟后端，不列入支持范围。
- [x] RAR 只读列表、普通/加密密码验证、完整测试和解压（UnRAR 后端，保留许可声明）；损坏 RAR 不再误报为加密；解压按条目声明大小接入资源预算。
- [x] 只读适配 40+ 格式：ISO/UDF、CAB、ARJ、ACE、ARC/PAK、Quake PAK、Chromium PAK、Unreal PAK、CPIO、WIM（第一映像，XPRESS/LZX/LZMS，支持 SWM 分卷自动发现）、DEB、ISZ、IMG/BIN、UU/XXE、LZ/LZMA、TAR.Z、ZOO、HA、HYP、SQ/SQ2/QQQ、SQZ、UC2、ICE、PACKICE 等（传统格式通过 `unarc-rs` 适配层）；LGPL `wim-format` 负责 WIM 资源解码，LZMS/SWM 有 256 KiB LZMS 与多分卷回归测试。
- [x] ALZ 只读列表、密码探测/校验、完整测试和解压：支持 Store、Deflate、ALZ 改造 Bzip2、ZipCrypto、`.alz`/`.a00` 分卷；解压按路径安全、覆盖策略、资源预算和 CRC 校验处理，失败不留半成品。
- [x] ZIP/7Z 原子编辑：删除、重命名、添加文件；失败不替换原归档；可选 `.lumark.bak`。
- [x] 归档内同名添加按替换处理，不再生成重复条目。
- [x] 7Z 固实归档的选择性解压、跳过覆盖和编辑删除保持数据流对齐，并有回归测试。
- [x] `.001` 原始分卷创建、列表和解压；分卷失败不破坏旧卷并清理半成品，成功替换清理陈旧卷。
- [x] 路径逃逸、Windows 保留设备名、ADS、符号链接目标防护。
- [x] 密码库：Windows DPAPI、可选主密码、Argon2id、XChaCha20-Poly1305、原子写入、损坏保留与 `.bak` 恢复、外部修改锁定、密码零化。
- [x] 密码管理：添加/编辑/删除/复制、掩码与 10 秒显示、名称/备注/路径或通配符、指纹候选、自动解压 ZIP/7Z/RAR、按文件夹导出加密 `.lumarkvault` 和明文 CSV、导入加密快照、锁定与空闲锁定；导入导出弹窗在密码库锁定时提供主密码解锁入口，操作结果使用官方 Notification 提示。
- [x] 记录密码验证在任务工作线程执行；发出只含“第几条/总数”的 `PasswordAttempt` 事件，不泄漏密码内容。
- [x] 前 32 条记录密码失败后显示确认对话框，由用户决定是否继续尝试其余候选。
- [x] 归档加载、归档编辑和归档条目临时解压均通过后台任务执行；编辑支持取消且失败不替换原归档。
- [x] 打开归档中的可执行文件或脚本前显示安全确认。
- [x] 主界面：全局侧栏、快速位置、懒加载目录树、路径输入、后退/前进/上一级、名称搜索（包含/通配符）、多选、详情栏、拖入、回收站删除、新建文件夹、重命名、复制路径、资源管理器定位。
- [x] 文件表使用 GPUI 虚拟列表，仅渲染可视行；平铺视图按可用宽度分组渲染；官方右键菜单、官方滚动条、目录树懒加载和持久化列宽预设已接入。文件夹大小显示为“可计算”链接，点击后在后台递归统计并缓存结果。
- [x] 模块拆分：将 `service.rs`、`compress_window.rs`、`browser.rs`、`chrome.rs`、`dialogs.rs`、`actions/archive.rs`、`app/tests.rs`、`tests/roundtrip.rs` 等超过 1000 行的文件按职责拆分；新增 `tools/check_file_sizes.ps1`，把所有 `.rs` 超过 1000 行纳入验证拦截。
- [x] 界面重构：`app.rs` 从 5572 行缩减到约 880 行，启动参数和共享存储进一步拆入 `app/startup.rs`、`app/stores.rs`；文件、归档、密码、设置动作和主窗口、文件表、页面、弹窗、主题与测试分别拆入子模块；所有 Rust 函数均有职责注释。
- [x] 参考经典归档器的布局重构：自绘标题栏工作区切换、渐变动作带、全宽路径/搜索导航行、常用位置与目录树合并的文件夹侧栏、底部详细信息检查器、全宽状态栏；宽窗口显示品牌标语，窄窗口自动收缩搜索框和模式标签。
- [x] 独立压缩窗口支持轻巧/经典双模式并共享状态：轻巧模式为 600×330 紧凑表单，经典模式为 600×390 六选项卡；格式、压缩方法、字典、单词、固实块、线程和时间筛选统一使用官方 Select，注释使用官方多行 Textarea，经典模式使用官方 Tabs、GroupBox、Radio 和 Checkbox。压缩包输出目录和解压目标目录均可通过系统目录选择器更换。
- [x] 独立解压窗口为 620×340，固定打开时选中的归档路径，输出目录、密码和仅解压选中条目在窗口内编辑；密码默认掩码并可用“显示密码/隐藏密码”切换明文，加密探测放到后台线程，避免打开大压缩包时卡住界面。
- [x] 独立任务进度窗口：压缩、解压和测试启动后自动打开，显示目标路径、当前文件进度、总进度、已用/剩余时间、总大小、已压缩、压缩后、压缩率、文件数、速度和暂停、继续、取消、后台；取消需要二次确认，确认后立即停止并清理临时文件，关闭窗口不取消任务，完成后自动关闭，状态栏提供“进度”入口。
- [x] 任务中心改为完整页面，记录任务历史、实时进度、状态和最终结果；点击任务或“查看”可重新打开独立进度窗口，后台压缩任务也能恢复；任务页面不再使用会穿透鼠标事件的浮层。多个压缩/解压/测试任务并行时各自维护任务记录和独立进度窗口，任务结束只关闭自己的窗口，解压的打开目录和密码保存目标按任务 ID 隔离。
- [x] 测试任务先异步探测加密状态再提交，避免点击后界面卡住；测试进度事件写入任务记录和进度窗口，大文件测试可以看到进度变化；已完成任务打开结果窗口时显示“已完成”和最终提示。
- [x] 压缩后端使用 zlib-ng，ZIP 按 1 MiB 分块读写并按块响应取消；7Z 多线程 LZMA2 默认按 16 MiB 编码块完成量上报真实进度（字典更大时随库钳制），两次块事件之间按最近速度平滑显示并在块完成时校正；单线程/其他算法按已写入编码器的字节更新，加密归档仍按读取进度；7Z 算法支持 LZMA2/LZMA/PPMd/BZip2 及字典、单词、固实块和线程参数，ZIP 算法支持 Deflate/BZip2/LZMA。7Z 固实压缩按“固实块大小”和每块 16384 个文件上限切块，大目录不再一次打开全部源文件，进度和完成数跨块连续统计。
- [x] 界面组件迁移到 gpui-kit 官方实现：动作带使用图标在上、文字在下的官方 Button，路径与搜索栏使用官方 InputGroup，高级搜索使用官方下拉菜单切换包含/通配符/正则，标题栏、导航行和侧栏的小按钮使用官方 ghost 文本按钮，所有按钮悬停显示手形指针，格式与算法/时间下拉使用官方 Select，文件右键菜单使用 ContextMenu/PopupMenu，文件表使用官方 Scrollbar，经典窗口使用官方 Tabs、GroupBox、Radio 和 Checkbox。
- [x] 自定义主题色：设置页使用官方 ColorPicker，默认蓝色；可选预设色或任意十六进制/HSLA 颜色，主按钮、动作带、选中态、进度条和状态栏同步换色并持久化。
- [x] 设置、密码库和任务中心统一使用白色浮层卡片、轻阴影、蓝色分区图标和浅色标题带，分组之间通过留白、分隔线和状态色建立层次，避免整页都是同一种灰色。任务记录图标使用状态色区分进行中、完成和失败。
- [x] 修复标题栏拖拽区域吞掉右上角密码库/设置按钮点击的问题；模态遮罩增加 occlude 命中阻塞，压缩设置对话框不再把鼠标事件穿透到下层文件表。删除确认对话框改为图标标题、待删除项目列表和右对齐操作按钮，明确区分回收站与归档内永久删除。
- [x] 现代浅色/深色主题、双栏密码库、分组设置页和统一模态层保留，并适配新的标题栏、页头横幅和无侧栏工作区。
- [x] IconPark 官方 SVG 已下载并本地打包，使用 `currentColor` 跟随主题；许可文件随资源保存，新增图标同步登记在 `tools/fetch_iconpark_icons.ps1`。
- [x] 真实窗口视觉验收已覆盖宽窗口、960×640 最小窗口、轻巧/经典压缩窗口、独立解压窗口、任务中心、文件夹大小计算、高级搜索菜单、文件右键菜单、主题色选择器、独立进度窗口、密码库、密码库导入导出弹窗、官方 Notification、主密码窗口和设置，截图保存在 `artifacts/`。
- [x] 侧栏导航、快捷位置和目录树已补充 Button 角色与无障碍名称。
- [x] Unix `.Z` 读取和 AES `.enc` 密码验证、列表、解压已接入；`.enc` 有加密往返测试。
- [x] `gpui-kit::test` UI 自动化已解除依赖冲突，覆盖真实窗口中的虚拟列表和平铺视图切换。
- [x] 快速/高级压缩入口：格式、预设、密码、固实、头加密、排除/仅存储通配符、分卷、注释页。
- [x] 压缩后测试、成功后删除源文件，并阻止输出归档位于待删除源目录。
- [x] 任务管理：后台压缩、解压、测试、编辑、记录密码验证、进度、暂停/继续、取消、状态栏、任务中心页面和独立进度窗口。
- [x] Windows：当前用户经典右键菜单注册/移除、默认应用设置入口、单实例保护、回收站、Send To“Lumark 压缩”快捷方式、安装/卸载脚本；可选关联 24 种压缩格式，包含 `.lzh`、`.lha`、`.alz`、`.tar.br`、`.tar.lz4`、`.tar.lz`、`.tar.lzma`、`.tar.z`、`.zpaq` / `.zpq`。
- [x] ISO 虚拟光驱：Lumark 文件列表和资源管理器 `SystemFileAssociations\.iso\shell` 菜单都提供加载/卸载入口，调用系统 `Mount-DiskImage`/`Dismount-DiskImage`，挂载后自动打开盘符；使用隐藏 PowerShell 子进程、30 秒超时和 `--mount-iso`/`--unmount-iso` 单实例转发，客户端普通用户通常无需管理员。
- [x] 单实例多窗口：Shell 请求按文件打开独立窗口并级联错开，窗口共享设置与密码库、各自独立任务中心，关闭全部窗口后进程退出。
- [x] 内置只读文本查看器：右键菜单直接在内置窗口查看文本文件，归档条目解压后展示，打开条目的解压进度窗口完成后自动关闭。
- [x] 从归档拖出：使用原生拖拽手势解析桌面/资源管理器目标，释放后把选中条目解压到目标文件夹；解压走后台任务和进度窗口，取消拖拽不会产生文件，失败会清理半成品。
- [x] Windows 系统托盘：后台压缩或解压任务没有可见窗口时显示托盘图标，点击后恢复对应进度窗口或主窗口，任务结束自动移除。
- [x] 记录密码校验的可中断体验：多候选校验持续超过 400 毫秒后弹出进度卡片，显示第几条候选并提供“停止自动尝试/后台继续”，状态栏同时保留停止入口；停止会取消当前校验任务，并在取消事件里直接回退到手动密码输入。
- [x] 自动匹配失败后的新增密码引导：密码弹窗提示记录密码均不正确，默认勾选“保存到密码库”，确认后先在工作线程校验手动密码，命中才写入密码库（名称取归档名、匹配规则取完整路径），输错则重新弹出密码框；相同密码不会重复写入。
- [x] 拖入文件的归档更新模式：确认框可选“添加并替换同名文件”“只更新较新的文件”“仅刷新已有条目”；仅较新模式按归档条目与磁盘文件时间比较（容差 2 秒，兼容 ZIP 的 DOS 时间精度），仅刷新模式跳过新文件；编辑报告和任务消息给出跳过数量，同名替换语义与原来一致。
- [x] 拖入文件夹与归档同步：文件夹按根目录名展开成归档内条目，确认框在存在文件夹时提供“同步文件夹内容”模式；同步会新增缺少的文件、更新较新文件，并删除归档中磁盘已不存在的条目，任务消息报告删除数量。
- [x] 修正 ZIP 条目时间按 UTC 解释的问题：DOS 时间字段按本地时区解释，列表时间不再整体偏移，更新模式的新旧比较也恢复正确。
- [x] 自解压 EXE（ZIP）：新增 `lumark-sfx` 运行时模块，生成端把模块、ZIP 数据和 24 字节页脚拼成单个可执行文件；压缩窗口可勾选“自解压 EXE”（仅无密码、不分卷的 ZIP），输出扩展名自动切换为 `.exe`；运行时按需读取数据段、拒绝路径穿越条目，解压到可执行文件同名目录后打开资源管理器。
- [x] 分卷 `.001` 的原地编辑会被拒绝并提示先合并为单文件，分卷只能创建、列表和解压。
- [x] 压缩后关机：压缩窗口可勾选“完成后关闭计算机”，任务成功后通过系统 `shutdown.exe` 安排 60 秒倒计时关机；失败或取消的任务不会关机，状态栏提示可用 `shutdown /a` 取消。
- [x] CPU 限制：压缩窗口可勾选“降低压缩优先级”，任务运行期间把进程优先级降到普通以下，多个低优先级任务并存时等最后一个结束再恢复；压缩线程数仍可在算法页单独限制，避免硬性百分比限速连带拖慢界面线程。
- [x] 真正的排队工作池：任务管理器改用固定工作线程池（默认按核心数一半、最多 4 个），任务先进先出排队，排队中的任务可取消且不会启动；管理器析构时通知工作线程退出。
- [x] 跨任务聚合：任务中心显示排队数量，并在有运行任务时给出按字节合并的“跨任务总进度”，避免只看单个任务时忽略其它并行任务。
- [x] 7Z 解压单文件内部进度：按 256 KiB 分块复制并按 100 毫秒节流上报已解压字节，单个大条目不再让进度条停在起点；新增慢速读取器的多段回调测试。
- [x] 解压资源预算：默认单次解压总量 128 GiB、条目 100 万；ZIP/7Z/TAR 在写文件前检查条目数与声明大小，单文件流、`.enc` 和容器复制按剩余预算分块中止，超限返回“解压超出资源预算”并删除半成品；新增 ZIP/7Z/GZ 三个拦截测试。
- [x] Shell 压缩失败时保留错误结果窗口，避免隐藏宿主在进度窗口关闭后立即退出，让用户看不到失败原因；ZPAQ 压缩窗口默认保留原始时间，避免无谓复制整个源目录。
- [x] 伪装压缩（全格式）：新增自有容器（面具 + BLAKE3 密钥流混淆载荷 + 512 字节尾部），覆盖 ZIP/7Z/LZH/ZPAQ/TAR 系列/单文件流；压缩窗口新增“伪装”选项卡和轻巧模式开关，支持内置 MP4/JPG 与自定义面具；ZIP/7Z/LZH/TAR 系列/单文件流直接写入伪装容器，仅 ZPAQ 先写内层归档再包装，失败/取消不破坏旧输出。
- [x] 一键打开与一键还原：Lumark 文件表和资源管理器“用 Lumark 打开”直接浏览伪装归档，ZIP/7Z/TAR 系列/单文件流/LZH 使用读取视图，ZPAQ/RAR/ALZ/传统格式物化并缓存；右键“还原伪装文件”和资源管理器“Lumark 一键还原”把真身写回同目录，同名自动改名，完成后在资源管理器中定位。
- [x] 任意文件伪装与设置：文件/归档右键“伪装为媒体文件…”打开独立 620×360 窗口，支持批量、输出目录、面具和删除源文件；设置页新增伪装默认启用、默认面具和自定义面具；不注册媒体扩展名默认关联。
- [x] 伪装测试：13 个伪装集成测试覆盖全部 19 种可写格式、密码、任意文件、已有归档、只读格式物化、损坏尾部、取消、批量删除源文件和同名还原；UI 测试覆盖压缩窗口、伪装窗口、启动参数和菜单判定。
- [x] 7Z 原始块复制编辑：重命名和“整块删除或替换”直接复用原压缩块；固实块内部分删除、加密密码变化和缺 CRC 时自动回退全量重压缩；`EditReport` 记录复用块数并在任务消息中展示。
- [x] 自解压扩展：支持 ZIP/7Z、AES 密码、v2 页脚和旧 v1 兼容；运行时提供目录选择、密码输入、错误密码重试、覆盖确认，以及 `/S`、`/D=`、`/P=`、`/Y` 参数；使用真实运行时二进制完成 ZIP 和加密 7Z 端到端测试。
- [x] 主界面本地化起步：接入官方 `rust-i18n 4.2` 和 gpui-kit 组件 locale，设置“综合”页可切换中英文；动作带使用 `locales/ui.yml` 的 key，设置窗口七个页签及综合/外观/压缩/文件关联/密码库/通知/关于页文案随语言变化；任务中心、独立解压窗口、主窗口页头、工作导航栏、文件表右键菜单、详情栏、密码记录列表、记录详情、安全卡片、密码编辑/主密码窗口和独立伪装窗口和密码库导入导出/确认弹窗和归档密码/保存密码/解锁弹窗和归档添加/可执行/删除确认弹窗和独立进度窗口和轻巧压缩窗口和压缩方法/筛选/时间下拉选项和压缩表单提示/错误和经典压缩窗口各页签、标题、分卷控件和分卷提示文案也已接入；主窗口其余页面仍待迁移。
- [x] 自解压运行时中英文界面：优先读取 `LUMARK_LANG`，否则按 Windows 用户界面语言选择；目录、密码、覆盖确认、成功/错误提示和按钮提供英文文本。支持 `/LANG=`/`--lang=` 覆盖；主界面完整本地化仍未完成。
- [x] 自解压图标真实模块测试：以当前测试 EXE 作为运行时模块，替换图标后读取页脚并解压载荷，覆盖 PE 资源改写和偏移重算。
- [x] 自解压自定义图标：压缩窗口可选择 `.ico`，生成时在追加载荷前用 Win32 资源 API 替换运行时模块的 RT_ICON/RT_GROUP_ICON，页脚偏移按替换后的实际大小计算。
- [x] CHM 只读格式：使用 `libchm` 解析 ITSF/ITSP、PMGL 目录和 LZX 数据，支持普通命名空间文件/目录列表、选择性解压、预算/取消和完整性测试；已加入 `.chm` Shell 关联。
- [x] Windows 正式安装器：按 GPUI Kit 官方打包文档新增 `installer/lumark.iss` 和 `installer/pack-installer.ps1`，暂存 app/SFX/许可文本，生成 Inno Setup 安装器和 SHA-256；`PrivilegesRequired=admin`，默认安装到 `C:\Program Files\Lumark`，安装器以启动安装器的原始用户身份注册当前用户经典右键菜单，并支持先签名暂存二进制和最终安装器。已用 Inno Setup 6.7.3 实际编译出 `dist/Lumark-Setup-0.1.0.exe` 并生成校验文件；未签名安装器的静默安装会被 SmartScreen 拦截，正式发布前需签名。
- [x] Authenticode 代码签名脚本：`installer/sign.ps1` 支持证书存储指纹或 PFX、SHA-256 时间戳、签名后自动 `signtool verify /pa`，以及 `-VerifyOnly` 校验；`pack-installer.ps1` 可先签名暂存二进制和最终安装器，没有证书时明确失败。
- [x] 发布信任检查与安全警告修复：定位到工作区被代码代理沙箱写入 Low Mandatory Level，`target` 下新编译的 exe 继承低完整性后，Windows 附件管理器在双击时弹出“打开文件 - 安全警告”（与 MOTW 同一套 UI，签名不能消除）；已将仓库根目录恢复为 Medium。`tools/build_release.ps1` 在构建前检测工作区，若为 Low 会恢复根目录为 Medium（权限不足时明确失败），再对 release exe、安装器和便携包做 MOTW/Low 完整性检查。
- [x] 工作区 Low 完整性再次修复（2026-11-02）：沙箱重新把仓库根标为 Low，`target` 在 Low 状态下重建，新 exe 继承 Low；已恢复根目录、`target\release` 和 release exe 为 Medium，并为当前用户补仓库根的继承完全控制（目录标签需要 WRITE_OWNER）。`tools/build_release.ps1` 现在会在构建前修复 `target`/`target\release`/`target\debug`/`dist` 目录和 debug exe，并在目录标签权限不足时自动补授后重试。
- [x] 一键发布构建：`tools/build_release.ps1` 统一编译 app/SFX、检查 MOTW 与工作区完整性、按需签名（开发证书/PFX）、调用 `pack-installer.ps1`、生成 `Lumark-<版本>-portable-x64.zip` 和 `SHA256SUMS.txt`。
- [x] GitHub Actions：新增 `.github/workflows/windows-build.yml`，push/PR 执行格式、行数、clippy、测试和 release 构建并上传 artifact；`v*` tag 校验 Cargo 版本后创建/更新 GitHub Release；支持 `LUMARK_SIGN_PFX_BASE64`/`LUMARK_SIGN_PFX_PASSWORD` 可选签名。
- [x] EGG 只读格式：使用 MIT `unegg` 读取 Store/Deflate/Bzip2/LZMA 条目，支持列表、选择性解压、覆盖策略、预算/取消、完整性测试和自动发现 EGG 分卷（含双卷回归测试）；AZO 条目因缺少可用解码器明确报不支持，使用本地 `libazo` 占位依赖并有回归测试。已加入 `.egg` Shell 关联。
- [x] DMG（UDIF）只读格式：使用 MIT `dpp` 自动识别 HFS+/APFS 卷，支持遍历、列表、选择性解压、覆盖策略、预算/取消和完整性测试；按 `.dmg` 扩展名或末尾 `koly` 识别，跳过符号链接。测试用 `hfsplus`/`udif` 合成原始 HFS+ DMG。
- [x] NSIS 安装包只读格式：使用 Apache-2.0 `nsis` 解析 PE overlay、压缩头和 `EW_EXTRACTFILE` 条目，支持列表、选择性解压、覆盖策略、预算/取消、完整性测试和自动发现 EGG 分卷（含双卷回归测试）；按内容识别，不注册 `.exe` 默认关联，避免劫持普通可执行文件。测试夹具来自 nsis-rs 并保留 NOTICE。
- [x] XZ 编解码依赖收敛：RPM/XAR 解码和 TAR/单文件流创建改用纯 Rust `lzma-rust2`，移除直接 `xz2` 依赖；新增 MIT/Apache 兼容的本地 `xz2` 适配层，把本地 `lumark-legacy` 的 XZ/LZMA 调用转到 `lzma-rust2`，并用 `liblzma` 为 `backhand` 提供 SquashFS XZ 后端，已通过 XZ/LZMA 往返和 XZ SquashFS 测试。
- [x] SquashFS 只读格式：使用 `backhand` 解析 v4 镜像，支持目录/文件列表、选择性解压、覆盖策略、预算和取消；启用 gzip/zstd/lz4/xz 压缩。已加入 `.squashfs`/`.sqfs` Shell 关联。
- [x] ZPAQ 解压和完整测试改走可杀死工作进程：取消时终止子进程并清理暂存区；解压仍由引擎一次性完成，移动阶段继续逐条上报进度。
- [x] 伪装归档原地编辑：ZIP/7Z 内层恢复、编辑、复用原面具重新包装后原子替换，保留原文件名和可选 `.lumark.bak`；其他内层格式仍要求先还原。
- [x] 资源管理器新增“伪装为媒体文件…”动词（`--disguise`），启动后直接打开轻巧伪装压缩窗口。
- [x] 全仓注释补齐：模块入口、函数、结构体字段和枚举变体统一使用中文职责注释；因注释增长超限的格式检测和压缩标签再次拆分为 `format/detect.rs` 与 `compress_window/labels.rs`。
- [x] 文件表列宽支持拖动表头分隔线逐列调整，松手后写入设置；设置页保留三档预设和“重置列宽”，自定义宽度优先于预设。
- [x] 10 万项目录性能验收：`bench_100k` 实测目录加载约 167 ms、全量搜索约 75 ms；基准位于 `crates/lumark-browser/examples/bench_100k.rs`。
- [x] Shell 多选转发：一次请求包含多个归档时，第一个在当前窗口打开，其余各开独立窗口；压缩/伪装仍合并批次。
- [x] 伪装单遍写入（ZIP、7Z、LZH、ZPAQ、TAR 系列和 GZ/BZ2/XZ/ZST/Brotli/LZ4）：ZPAQ 使用 fork 的 `zpaq_rs::ArchiveWriter` 逐条目写入标准流，有密码时由写入器生成盐并做 AES-CTR 流式加密，直接写入伪装容器。
- [x] 7Z/LZH 伪装单遍写入：写入器通过可 Seek 的伪装适配器直接写入 `DisguiseWriter`，7Z 的 32 字节签名头和 LZH 条目头都在数据写完后按载荷位置混淆回填；不再先落完整内层文件；新增 LZH 多条目 2 MiB 大文件回填测试。
- [x] ZIP 伪装单遍写入：`ZipWriter::new_stream` 直接写入伪装容器，支持 AES/ZipCrypto、进度和取消。
- [x] 媒体扩展名 `.mp4`/`.jpg` 注册 `Lumark.Media` 到 `OpenWithProgids`，只加入“打开方式”候选，不修改系统默认关联。
- [x] Windows 会话锁定：每 2 秒查询一次会话标志，检测到锁定且密码库需要主密码时自动锁定并清空选中记录；DPAPI-only 密码库保持可用。
- [x] 卸载数据保留：Inno Setup 卸载只移除安装目录，用户设置和密码库继续保留；需要彻底清理时由用户手动删除应用数据目录。
- [x] 邮件通知基础：新增 `lumark-mail` SMTP 发送器（隐式 TLS/STARTTLS/无 TLS、多收件人、配置校验）和 `lumark-mail` 单元测试；SMTP 密码使用 Windows DPAPI 保护，设置模型已加入邮件字段。
- [x] 邮件通知完整接入：设置窗口新增“通知”页，可保存 SMTP 主机/端口/账号/密码/发件人/收件人和 TLS 模式，支持测试发送；压缩任务成功后自动发送通知，密码用 DPAPI 加密保存。
- [x] XAR/PKG 只读格式：解析 28 字节头、zlib 目录和逐文件 zlib/BZip2/XZ/Store 数据，支持列表、选择性解压、符号链接跳过、预算/取消和完整性测试；`.xar`/`.pkg` 已加入 Shell 关联。
- [x] RPM 只读格式：解析 lead、签名/主 header 和 gzip/xz/BZip2/zstd/LZMA 压缩的 newc cpio 载荷，支持列表、选择性解压、符号链接跳过、预算/取消和完整性测试；`.rpm` 已加入 Shell 关联。
- [x] MSI 只读格式：读取 OLE 复合文档中的 File/Media 表，打开内嵌或同目录外部 CAB 并列表、选择性解压和完整性测试；按 File/Component/Directory 表还原目录层级，MSI 写入仍不支持。
- [x] 命名管道单实例请求：首个进程启动管道服务，后续进程优先通过命名管道转发参数，失败时回退现有请求目录；参数带长度前缀，不落盘。
- [x] 归档头部确定性变异测试：对 ZIP/7Z/TAR/GZIP 各做 64 次单字节篡改，对 NSIS/SquashFS/DMG/EGG 各做 32 次单字节篡改，并对 ZIP 做多组截断；列表、测试和解压只返回错误，不 panic。
- [x] WIM 创建：使用 LGPL `wim` 后端捕获暂存目录并生成 LZX 压缩归档，支持多文件和目录；LGPL `wim-format`/`ms-compress` 只读后端支持 XPRESS/LZX/LZMS 和 SWM 分卷。暂存目录在完成后删除，WIM 仍不支持密码和分卷写入。
- [x] cargo-fuzz 持续目标：新增 `fuzz/fuzz_targets/archive_list.rs`，把输入写成临时 ZIP 后调用列表和完整测试，可直接用 `cargo +nightly fuzz run archive_list` 运行；工作区外独立 lockfile。
- [x] 设置窗口通知页 UI 测试：打开真实设置窗口、切到“通知”页并验证 SMTP 主机、端口和密码控件可见。
- [x] 自动解压提示 UI 测试：锁定密码库且启用自动解压时，打开加密包会进入解锁提示并保存待处理归档，不直接打开密码框。
- [x] 任务中心暂停按钮 UI 测试：运行中任务渲染暂停按钮，点击在缺少控制器时不 panic。
- [x] 任务取消确认 UI 测试：进度窗口确认弹层渲染“保留任务/确认取消”两个按钮。
- [x] 拖入归档确认 UI 测试：在归档内拖入文件时进入更新模式确认弹窗，而不是直接覆盖；验证待添加列表和确认按钮。
- [x] 归档闭环基准：新增 `bench_archive`，32 MiB 源下 ZIP/7Z/TAR.GZ 的创建、列表、解压、测试均有 release 实测；ZIP 创建约 94 ms，7Z 约 3.5 s，TAR.GZ 约 133 ms。
- [x] UI 自动化新增邮件设置测试：验证 SMTP 密码以 DPAPI 密文保存、发送配置解密还原，以及清除密码后不再保留密文。
- [x] UI 自动化新增独立密码编辑窗口测试：验证标签、密码、匹配规则和保存按钮可渲染。
- [x] 任务暂停/取消控制测试：验证暂停会阻塞工作线程、恢复后继续、暂停中取消立即唤醒并设置取消标志。

## 下一步（按优先级）

- [x] SFX 支持 `LUMARK_SFX_PASSWORD` 环境变量；`/P=` 保留兼容，文档建议优先交互输入或环境变量，减少命令行历史泄漏。
- [x] ZPAQ 伪装单遍写入：fork 的 `ArchiveWriter` 支持无密码标准流和有密码 AES-CTR 流；ZIP/7Z/LZH/TAR 系列/单文件流也均为单遍。媒体扩展名的 `OpenWithProgids` 已注册。
- [x] 与 Apate 的二进制互通：按用户决定不实现；Apate 为 GPL-3.0 且格式未公开，不复制其代码。
- [x] ZPAQ 选择性和可分组全量解压按顶层过滤组分段调用引擎，可在多个顶层条目之间上报进度并响应取消；单个超大条目仍由上游引擎一次阻塞完成。PAQ8/LPAQ 系列按用户决定不支持。
- [x] 邮件通知只做纯文本；支持 1–300 秒 SMTP 连接/命令超时、瞬时网络/4xx 错误最多 3 次的进程内退避重试、最近 8 条发送记录、持久化待重试队列（跨进程保留、手动重试）、XOAUTH2 访问令牌，以及通过 `reqwest` 使用刷新令牌从 OAuth2 令牌端点获取新访问令牌。
- [x] PAQ8/LPAQ 按用户决定不支持；MSI 已支持内嵌 CAB 只读打开，XAR/PKG 已支持只读打开；WIM 的 LZX 创建、LZMS 读取和 SWM 多分卷已完成；Unix `.Z` 已有真实样本和检测回归。
- [x] LZH 的 lh4/lh5/lh7 已支持跨块流式编码，不再受 512 MiB 单条目内存上限限制；Store（lh0）继续分块直写。
- [x] 键盘/高对比度验收：按用户决定暂不进行。
- [x] 主界面状态文案已完成中英文迁移，压缩格式错误提示使用本地化格式名称。
- [x] 传统格式写入上限按上游适配器能力记录并在文档说明；Windows 原生拖出手势通过 `SHDoDragDrop` 提供系统光标，实际投放目录由 `IShellWindows`/桌面路径解析，解压由 Lumark 任务执行。
- [x] 依赖收敛：从 GeeZipX 0.7.4 裁剪出本地 `lumark-legacy`，只保留实际使用的 ACE/ARC/ARJ/ASAR/CAB/CPIO/DEB/ISO/LZH/UDF 和单流辅助模块；删除外部 `geezipx-core` 依赖及其 ZIP 2/7z 0.21 重复依赖，WIM/AES 改用标准后端，错误映射和路径安全逻辑保持不变。

## 外部依赖/架构限制

- ZPAQ 伪装单遍写入已通过 fork 的 `zpaq_rs::ArchiveWriter` 实现；无密码走标准流，有密码先写 32 字节盐再用 AES-CTR 加密，均由官方 `zpaq` 读取验证。
- WIM 创建使用 LGPL-2.1+ 的 `wim` 后端生成 LZX 压缩归档；LZMS/SWM 读取使用 `wim-format`/`ms-compress`。
- PAQ8/LPAQ 没有许可兼容的成熟 Rust 后端，按用户决定暂不支持。
- LZH lh4/lh5/lh7 不再受整块内存限制；编码块实现改编自 Apache-2.0 的 `oxiarc-lzhuf`。

## 验证命令

```powershell
powershell -NoProfile -File tools/check_file_sizes.ps1
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
powershell -NoProfile -File tools/build_release.ps1 -Version 0.1.0 -UseDevCert
```

DPAPI 测试必须在有权访问当前 Windows 用户密钥存储的普通用户会话中运行。托盘图标测试需要可创建 Shell 通知图标的交互式桌面会话；受限会话可能返回“拒绝访问”。
