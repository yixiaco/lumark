# 游戏资产开发说明

本文件记录“游戏资产”功能的当前实现边界、验证方式和后续里程碑。功能仍在分期实现，
不应把未勾选的里程碑视为可用能力。

## 已实现（M1–M4 纹理部分）

- **容器层**：`lumark-archive` 新增 UE IoStore（`.utoc` + `.ucas`）只读读取，支持 v1–8 TOC、
  压缩块表、目录索引 AES-256-ECB 解密、None/Zlib/Gzip/LZ4/Zstd/Oodle 解压，并接入
  `ArchiveService::detect/probe_encryption/verify_password/list`。IoStore 的解压、测试和编辑
  明确返回“仅支持浏览与查看”；PAK 能力保持不变。
- **多游戏会话**：游戏库可同时打开多个游戏，每个会话保留自己的浏览位置、回退/前进历史和
  详情状态；打开和切换会更新最近打开时间，设置页按时间倒序展示游戏库，打开列表持久化到
  设置（最多 8 个）。游戏资产入口进入“游戏库列表页”，点“打开”才进入容器浏览；侧栏和
  上下文条的“返回列表”回到列表页并保留浏览位置，列表页的“返回文件浏览”再回到文件系统。
  游戏名称统一使用游戏库保存的名称（扫描出的项目名可能只是子目录名）。
- **游戏编辑窗口**：新增/编辑游戏在独立窗口中完成，复用共享设置和密码库；保存后窗口自动
  关闭，主窗口刷新游戏库列表。新增入口和编辑入口都从游戏库列表页进入。
- **AES 密钥查看**：从 Shipping.exe 提取并通过容器校验的密钥可在游戏设置页点击“查看密钥”，
  用内置文本/十六进制查看器检查；查看器直接读取内存内容，不把密钥写入临时文件。
- **UE5 Zen 包**：`lumark-unreal` 解析 Zen 摘要、名称表、导入/导出表、导出数据、全局
  ScriptObjects 脚本对象表和 UE5.4+ bulk data map；已用 BNB 真实包验证类名、导出范围和
  bulk 条目。
- **无版本属性**：新增 `.usmap` v1–v4 读取（未压缩/ZStandard/Brotli，Oodle 压缩映射返回
  可读错误）与 unversioned header/属性读取；游戏设置选择映射时会先解析并显示结构与枚举
  数量，解析失败不会保存路径。Zen 资产的详情面板会把可序列化属性追加为字段行（最多 40 项，小型
  struct/数组展开一层，长文本截断到 160 字符），纹理、StaticMesh、SkeletalMesh、Skeleton 缩略图
  和旧式包共用同一只读字段展示。
- **纹理**：UTexture2D 平台数据、内联/`.ubulk` mip、BC1/BC3/BC4/BC5/BC7 与
  BGRA8/RGBA8/G8 解码；详情面板后台生成缩略图并支持点击进入图片查看器。
- **资产元数据**：Skeleton 参考骨骼树（名称/父索引/参考姿态）、AnimSequence 的时长和
  RateScale、StaticMesh 材质槽/插槽；在详情面板展示。
- **材质外观渲染**：FF7R 旧式材质/材质实例会在详情面板渲染近似 PBR 材质球：从材质表达式提取矢量/标量参数默认值，读取父材质补齐材质实例参数，按名称识别基础色/法线/金属粗糙度贴图并从同一容器加载解码，CPU 光栅器生成 512×512 材质球（含方向光、高光、法线扰动）。详情面板中的材质球可点击打开**独立的材质查看器**（与模型/骨骼查看器共用交互式窗口）：左键拖拽绕球旋转、缩放/重置，旋转时按世界空间固定光照重绘，静态图片仍由图片查看器打开。FF7R `M_UE4Man_Body`/`M_UE4Man_ChestLogo` 已生成可辨认的皮肤色+法线细节材质球；UE5/IoStore（BNB）材质同样渲染材质球：从 `VectorParameterValues`/`ScalarParameterValues`/`TextureParameterValues` 提取参数，按同目录命名约定定位贴图（`MAT-ChickenA` 验证羽毛贴图+法线细节）；完整 shader 图不复原。

- **StaticMesh 预览**：首个 LOD 的顶点/索引/section 解析，并识别资产声明的 LOD 数量；
  CPU 光栅器生成 3D 缩略图显示在详情面板；独立模型查看器窗口支持左键轨道旋转、缩放和
  重置，并可按 section/材质槽显示或隐藏几何。StaticMesh 的全部 LOD 已解析，查看器可
  循环切换 LOD；SkeletalMesh 也会解析并切换 LOD0/后续 LOD（后续 LOD 为尽力探测）。
- **骨骼与动画**：Skeleton 参考姿态累积为世界空间线条图，可在同一查看器中旋转/缩放；
  SkeletalMesh 读取内置参考骨架和各 LOD 的流式索引/顶点缓冲，详情面板与查看器显示
  rest pose，并在模型上叠加参考骨骼线条、按 section/材质切换可见性；同时解析
  `FSkinWeightVertexBuffer` 的骨骼索引/权重数组（最多 12 影响/顶点），并按 RequiredBones
  映射到骨架；新增 linear blend skinning（骨骼局部旋转增量 → 变形网格），查看器工具栏
  提供“蒙皮演示”按钮，详情副标题显示顶点数和最大影响数；
  AnimSequence 详情面板显示时长、RateScale、轨道数、压缩字节、压缩 codec 和只读时间轴；
  `AnimCompress_PerTrackCompression_6` 关键帧解码与帧采样已实现；先选中 AnimSequence，
  再打开 SkeletalMesh 查看器，即可用“播放动画”按钮连续播放/暂停；查看器工具栏显示只读
  时间轴、当前帧/总帧数与秒数，并提供回到开头、上一帧、下一帧按钮。UE4 旧式包（FF7R）的
  `AnimEncoding_ConstantKeyLerp` 压缩数据也已解码（轨道偏移表 + Float96NoW/Fixed48NoW/
  IntervalFixed32NoW），`ThirdPersonIdle` 验证 68 轨道/80 帧/2.63s 并可播放。
- **PAK 资产**：游戏 `.pak` 中的 Zen `.uasset/.ubulk` 通过归档服务提取后复用同一套
  纹理/模型/骨骼/元数据预览；加密 PAK 的目录列表会带上加密标志，打开条目、解压、复制和
  拖出都自动复用游戏会话的 AES 密钥，资产不会再显示“需要密码”。
- **资产打开**：游戏容器内双击资产和右键“使用内置查看器打开”走同一入口：已解析的纹理/
  模型/骨骼立即打开对应查看器，仍在后台解析时先排队，解析完成后自动打开；没有可用查看器
  的资产会在状态栏说明原因。
- **游戏容器列表口径**：游戏资产按包浏览，`.uexp/.ubulk/.uptnl` 包伴随文件默认隐藏，
  其大小并入所属 `.uasset/.umap`；文件夹同时汇总解压后与压缩后大小，所以上级文件夹的
  “大小/压缩后大小”与进入后条目总和一致，压缩后总计也能和 PAK 的磁盘大小对照。
- **模组容器**：除 `Content/Paks` 根目录外，递归扫描 `~mods` 等子目录中的 `.pak/.utoc`，
  游戏归档列表与 FModel 一致。
- **按类型搜索**：搜索栏新增“类型”下拉，可按图片/纹理、模型、骨骼、动画、音频、文本、压缩包、其他筛选搜索结果；空搜索文本 + 类型即可列出全部同类资产。分类依据扩展名与 UE 目录/前缀启发式（`Model`、`Texture`、`Skeleton`、`SK_`、`AS_` 等），不读取文件内容。
- **平铺视图**：平铺列数随窗口宽度实时重排；图片文件自动生成缩略图（文件系统直接解码，归档条目解压后缩放），未就绪或失败时保留类型图标。- **查看器复制**：文本行和十六进制行都可以用鼠标拖选，Ctrl+C 复制所选内容
  （十六进制行包含偏移/字节/ASCII）；工具栏“复制”仍可整体复制当前视图。
  内存中的 AES 密钥查看器同样支持。
- **验证**：CUE4Parse UE5.8 unversioned fixture 的 T_BC1/T_BC3/T_BC4/T_BC5/T_BC7/
  T_BGRA8/T_G8/T_Mips/T_Streaming 均成功读取；T_BC1/T_BC7/streaming 解码结果与 fixture
  源图一致；`SKEL_Fixture` 解析出 Root、Joint_1、Joint_2 三根骨骼；`AS_Fixture` 解析出
  AnimSequence 元数据。BNB 的 `T_ChickenA_BaseColor` 已解析出 Texture2D 与 12 个 bulk
  条目；属性解析仍需 BNB 的 `.usmap`。诊断命令：
  - `cargo run -p lumark-unreal --example inspect_asset -- <utoc> <key|-> <global.utoc> <entry>`
  - `cargo run -p lumark-unreal --example mappings_check -- <file.usmap> [StructName]`
  - `cargo run -p lumark-unreal --example inspect_texture -- <utoc> <key|-> <global.utoc> <entry> <usmap> [ubulk|-] [out.png]`
  - `cargo run -p lumark-unreal --example inspect_skeleton -- <utoc> <key|-> <global.utoc> <entry> <usmap> [export]`
  - `cargo run -p lumark-unreal --example inspect_mesh -- <utoc> <key|-> <global.utoc> <entry> <usmap> [export] [out.png]`

## 已知限制与后续

- **UE4 旧式包**：已支持 FF7R（UE4.18）`uasset/uexp` 的 unversioned 属性、纹理、Skeleton、SkeletalMesh 与 AnimSequence 播放；
  其它自定义引擎版本的 native struct 布局仍按结构特征尽力探测。旧式包头部与自定义版本表已解析；FF7R 包自定义版本为 0，按 UE4.18
  默认版本推断 SkeletalMesh 相关布局。
- **可交互 3D 查看器**：独立查看器窗口已支持 StaticMesh/SkeletalMesh 轨道旋转/缩放、
  多 LOD 切换和 Skeleton 骨骼线条图；SkeletalMesh 几何/蒙皮/动画播放已接入。当前详情面板
  显示 StaticMesh 静态缩略图、Skeleton 线条图或 AnimSequence 时间轴，点击缩略图或双击
  `.uasset` 可打开查看器。使用步骤见“使用说明”。
- **文档**：README/用户使用说明在功能完整后补充。

## 映射文件（`.usmap`）

UE5 cooked 资产通常是无版本属性，属性名和类型不在包内，需要游戏对应的 `.usmap`：

1. 打开【游戏资产】→ 编辑游戏（或添加游戏）。
2. 选择游戏目录后，Lumark 会先在游戏根目录、`Mappings/`、`Content/Paks/` 中自动寻找
   `.usmap`；也可以点击“选择映射…”手动指定。
3. Lumark 会在后台解析文件；成功时显示结构/枚举数量，失败不会保存路径。
4. 保存后，纹理、StaticMesh、Skeleton、AnimSequence、SkeletalMesh 元数据会使用该映射。

BNB 的映射文件已由用户提供，位于游戏根目录下的构建目录
`5.6.1-.../Mappings/*.usmap`；Lumark 扫描 BNB 时会自动发现并预填。映射解析结果为
12749 个结构、2386 个枚举。没有映射时，详情面板仍会显示包名、导出数量、主类等包级
元数据；纹理/模型/骨骼/动画的字段级解析会明确提示需要映射。

## 阶段状态

- **阶段 1（只读浏览与静态查看）已完成**：游戏库/多游戏会话、目录扫描、AES 密钥提取与校验、
  Oodle/映射配置、IoStore/PAK 文件夹式浏览、纹理预览、StaticMesh 首个 LOD、Skeleton 参考
  骨骼、AnimSequence 元数据/时间轴、SkeletalMesh rest pose/多 LOD 与模型/骨骼查看器。
- **BNB 实机验收已完成**：容器/密钥/路径浏览通过；使用 BNB 映射后，纹理
  `T_ChickenA_BaseColor` 解码为 2048×2048 PF_DXT5（12 mip），StaticMesh `Cube`
  （24 顶点/12 三角形）、SkeletalMesh `SK_Chicken`（42 骨骼、2860 顶点、3844 三角形）、
  Skeleton `SK_Chicken_Skeleton`（42 骨骼）和 AnimSequence `ANIM_Chicken_Idle1`（4.000s）
  均通过端到端验收；`tools/verify_game_assets.ps1` 已跑通真实 BNB 纹理链路。
- **FF7R 实机**：PAK 列表/提取与旧式包摘要已通过；加载 FF7R `.usmap` 后，Texture2D 可显示 `AddressX`/`VirtualTextureStreaming`/`bApplyResidentLODBias`/`CachedCombinedLODBias`/`NeverStream`，SkeletalMesh 可显示 `Skeleton`/`MinLOD`/`bUseHighPrecisionTangentBasis`，Blueprint/EndTextResource 等也可显示字段。旧式 `Skeleton` 读取已实现：`UE4_Mannequin_Skeleton` 解析出 68 根参考骨骼并在骨骼查看器显示，参考姿态使用 UE4 的 f32 `FTransform`；`SK_Mannequin` 的 SkeletalMesh 原生数据会探测内嵌参考骨架（同样 68 根骨骼），并解析首个 LOD 的 section/索引/顶点几何（实测 23297 顶点、41052 三角形、2 section）及逐顶点蒙皮权重（4 影响），可在查看器中显示 rest pose 网格、骨骼叠加和做蒙皮演示；FF7R 旧式 AnimSequence 已完整解码 ConstantKeyLerp 压缩数据：`ThirdPersonIdle` 读出 80 帧、68 条骨骼映射和约 2.63 秒时长，可在查看器中播放并用时间轴/帧按钮查看；`SK_Mannequin` 的 23297 顶点/41052 三角形网格可由该动画驱动完成蒙皮变形。
- **阶段 2（蒙皮动画播放）已完成**：SkeletalMesh 顶点索引/权重数组已读取（BNB 2860
  顶点、8 影响），RequiredBones→骨架索引映射与 linear blend skinning 已完成（示例可将
  大腿骨骼旋转 15° 生成变形网格）；AnimSequence 的 PerTrackCompression 关键帧已解码，
  `anim_play` 可按时间采样（BNB 42 轨道/121 帧）并驱动 LBS 生成动画帧；查看器已接入
  连续播放/暂停、时间轴进度、帧号/秒数与逐帧按钮（先选中 AnimSequence 再打开 SkeletalMesh 查看器），FF7R 旧式 ConstantKeyLerp 动画同样可播放；StaticMesh/SkeletalMesh
  多 LOD 均可在查看器中切换。

## 实机验证

- **BNB**：扫描到 `TogetherBNB` 项目、IoStore 容器和 AES 密钥；容器可列包，映射自动发现后已完成纹理/StaticMesh/SkeletalMesh/Skeleton/AnimSequence 的字段级验收（结果见上一节）；在游戏目录不含任何 Oodle DLL 的情况下，纯 Rust Oodle 后端再次验证 `SK_Chicken` 与 `T_ChickenA_BaseColor` 可正常解压、解析并渲染。
- **FINAL FANTASY VII REMAKE**：项目目录 `End`；主程序为 `Binaries/Win64/ff7remake_.exe`（无 `-Win64-Shipping.exe` 命名，已支持最大 exe 回退）；从其提取的 AES 密钥通过 `pakchunk0-WindowsNoEditor.pak` 验证；Oodle 候选位于 `Engine/Binaries/ThirdParty/Oodle/Win64`；该 PAK 可列出 18896 个条目，并已修正旧版 PAK 压缩槽位映射，可提取 `.uasset/.uexp/.ubulk`。其包为 **旧式 unversioned 包**（包 flags `0x80002000` 含 `PKG_UnversionedProperties`，`.uexp` 开头是无版本 header），字段名解析需要专用 `.usmap`。游戏实际为 **UE4.18**（可执行文件内 `++UE4+Release-4.18`）；FModel 手动 UE 版本应设为 4.18，而不是当前配置里的 4.28，否则 FModel 自身也会按错误的版本化布局解析。游戏目录下现已提供 **FF7 Remake 专用映射**（`Mappings.usmap`，9736 structs/1939 enums，含 `End*` 游戏类），映射本身可正常加载。旧式属性读取已能解析 `BlueprintGeneratedClass`、`EndTextResource`、`SimpleConstructionScript` 等资产；关键修正是 FF7R/UE4.18 的旧式 bool 值可能是**任意非零字节**（例如 `0x81` 表示 true），不能只接受 0/1。修正后 `Texture2D` 可读出 `AddressX`、`VirtualTextureStreaming`、`bApplyResidentLODBias`、`CachedCombinedLODBias`、`NeverStream`，`SkeletalMesh` 可读出 `Skeleton`、`MinLOD`、`bUseHighPrecisionTangentBasis`，详情面板会展示这些字段。**纹理基础 mip 无映射启发式预览**保留：通过 `SizeX/SizeY/PackedData/PF_*` 定位原生平台数据并读取 `.ubulk` 基础 mip（512×512 PF_BC5 已验证）。FF7R 旧式 SkeletalMesh 的 LOD0 几何/蒙皮与 AnimSequence 压缩数据均已解析；LOD1+ 与部分材质 native 布局仍按结构特征尽力探测。

## 已知依赖与限制

- 加密 IoStore 的目录索引本身使用 AES 加密；列包只需要密钥。读取资产数据时，Oodle
  压缩块默认由内置的纯 Rust 解码器（`oozextract` 0.5.7，MIT）解压，不需要用户安装 DLL；
  `LUMARK_OODLE_BACKEND=auto|rust|dll` 可切换后端（默认 `auto`：先 Rust，失败回退 DLL）。
  DLL 后端仍支持游戏自带 `oo2core*.dll`、FModel 的 `oodle-data-shared.dll` 或
  `LUMARK_OODLE_DLL`，用于特殊格式或对照验证。
- 仅支持单分区 IoStore；分片容器返回明确错误。签名只跳过，不验证。
- StaticMesh、SkeletalMesh 与旧式（UE4.18）骨骼网格现均可读取法线、切线、UV 通道和顶点颜色（旧式暂不含顶点颜色），并暴露 `StaticMaterials` 材质槽名；FF7R 的位置偏移已修正，包围盒与蒙皮动画链路复验通过。
- 材质槽解析 API：`resolve_material_slots` 从槽位名定位材质包并合并旧式父材质链，再按命名约定加载基础色/法线/ORM 贴图；`resolve_material_asset` 可直接解析单个材质或材质实例。Zen 包缺少对象引用包路径时按资产目录/文件名启发式搜索，找不到时返回 warning 而非失败。
- 动画采样 API：`sample_animation` 按秒采样（自动钳制到 `[0, length]`）并输出局部姿态与列主序蒙皮矩阵，`skin_mesh_animation` 提供 CPU LBS 预览（位置/法线/切线同步蒙皮），可供后续 GPU 蒙皮与 glTF/PSA 导出复用。
- World 摘要 API：`read_world_summary` 解析 `.umap` 的 PersistentLevel、StreamingLevels 与 Actors 数组，输出 Actor 清单和类别统计；World Partition 子关卡在属性流无法解析时用全数据扫描回退，SoftObjectPath 等未支持属性会记录 warning。
- 引擎无关场景层：`lumark-scene` 提供 `Scene`/`SceneNode`/`MeshData`/`MaterialData`/`Transform`/`InspectorModel` 等类型，`lumark-unreal` 的 `static_mesh_scene`/`skeletal_mesh_scene` 把 UE 资产转换为该场景（Z-up、0.01 单位标记），渲染层与未来 Unity 适配共用这一入口。
- 渲染层：`lumark-render` 提供 CPU 参考光栅化器与 wgpu 30 离屏后端（优先独显，失败回退软件适配器；无 GPU 时上层可回退 CPU）。两者输出同一 `RenderTarget`（RGBA8 / `to_bgra`），支持材质基础色/法线贴图、双灯+环境光与超采样，供查看器以 `RenderImage` 显示或离屏截图。
- 导出层：`lumark-export` 已实现 ActorX PSK（静态/骨骼、蒙皮权重、骨骼、多 UV、顶点色、Y 镜像）、glTF 2.0 binary（PBR 材质、内嵌 PNG 贴图、skin/inverse bind matrices、Z-up→Y-up）、UEFormat UEMODEL v10（LOD 属性集、材质、蒙皮权重、骨架）与 USDA（UsdPreviewSurface 自动材质、基础色贴图旁挂、GeomSubset 多材质）；USD Skel 蒙皮与整包批量导出将在后续轮次补齐。
- 游戏资产入口在非 Windows 平台隐藏；Oodle 的纯 Rust 后端本身跨平台，当前入口仍按 Windows 门控。
- SkeletalMesh 的参考骨架（骨骼名/父索引/参考姿态）和各 LOD 的 rest pose 几何已可读取并
  在查看器中显示（section 材质分色 + 骨骼线条叠加）；副标题给出材质槽与其名称、LOD、section/
  顶点/三角形以及蒙皮顶点数/最大影响数。LBS 蒙皮、PerTrack 动画解码、帧采样、查看器连续
  播放与 SkeletalMesh 多 LOD 切换均已实现（后续 LOD 使用可选块之后的头部探测，并为每个
  LOD 保存独立蒙皮权重，切到 LOD1 播放动画时使用 LOD1 的权重）。
- 详情面板 UE 加载器拆分为 `browser/preview.rs`（图片预览）与 `browser/preview_ue.rs`
  （UE IoStore/PAK 资产预览），均在 600 行以内。
- `tools/check_file_sizes.ps1` 的行数门禁为“超过 1000 行提醒、超过 3000 行失败”：
  `lumark-archive/src/service.rs`、`lumark-platform-windows/src/virtual_drag.rs` 这类
  既有文件只触发提醒；本次新增文件仍按职责拆分，UI 文件目标 600 行以内。

## 验收命令

基础测试：

```powershell
cargo test -p lumark-archive --test iostore
cargo test -p lumark-unreal
cargo test -p lumark-ui game_assets
```

真实游戏端到端验收（不打印 AES 密钥；未提供完整参数时只跑测试）：

```powershell
$env:LUMARK_GAME_ROOT = "D:\Games\BNB"
$env:LUMARK_GAME_MAPPING = "D:\bnb.usmap"
# 可选：仅对照 DLL 后端时设置；默认纯 Rust 后端不需要
$env:LUMARK_GAME_OODLE = "D:\Games\FModel\Output\.data\oodle-data-shared.dll"
$env:LUMARK_GAME_CONTAINER = "pakchunk0-Windows.utoc"
$env:LUMARK_GAME_ENTRY = "TogetherBNB/Content/.../T_Example.uasset"
$env:LUMARK_GAME_OUTPUT = "target\game-acceptance.png"
.\tools\verify_game_assets.ps1
```

脚本调用 `accept_game` 示例：扫描游戏、提取并校验密钥、读取映射、渲染纹理或
StaticMesh 到 PNG。

## 使用说明

1. 在设置页添加游戏目录后，进入主界面“游戏资产”；可同时打开多个游戏，侧栏标签切换，
   每个会话保留自己的目录位置、选择和前进/后退历史。
2. Oodle 默认使用内置纯 Rust 解码器；游戏设置中的 Oodle 区显示当前后端，只有需要
   特殊版本或对照验证时才选择 DLL。需要字段级属性的游戏还要选择该游戏的 `.usmap`
   （扫描目录时会自动发现根目录、`Mappings/`、`Content/Paks/` 下的映射并预填）。
   从 Shipping.exe 提取到密钥后，可点击“查看密钥”在内置文本/十六进制查看器里检查。
3. 浏览 IoStore/PAK 内的文件夹；单击资产在详情面板预览（游戏容器内的 `.uexp/.ubulk/.uptnl`
   默认隐藏并计入所属资产包，文件夹“大小/压缩后大小”与进入后条目总和一致）：
   - 纹理：后台解码基础 mip，显示格式/尺寸；无映射的 FF7R 纹理走原生启发式。
   - StaticMesh/SkeletalMesh：显示 3D 缩略图、LOD/材质/section 和字段行。
   - Skeleton：显示参考骨骼线条图。
   - AnimSequence：显示时长/轨道/压缩信息；先选中它再打开 SkeletalMesh 查看器可播放动画。
4. 双击 `.uasset` 或点击缩略图打开独立查看器：左键拖拽旋转、滚轮/工具栏缩放、LOD
   按钮切换（StaticMesh 与 SkeletalMesh）、section 按钮切换材质可见性；SkeletalMesh
   还提供“蒙皮演示”和动画播放/暂停。
5. 查看器与解析均为只读，不修改游戏目录或原始包；文本/十六进制内容可用鼠标拖选后
   Ctrl+C 复制，工具栏“复制”可整体复制当前视图。

已知限制：FF7R 的 SkeletalMesh rest pose 几何与蒙皮权重已解析；AnimSequence 已可播放（先选中动画再打开 SkeletalMesh 查看器）；材质渲染为基于参数/贴图的近似 PBR 材质球，完整 shader 图不复原；Zen 材质贴图按同目录命名约定定位；FF7R 旧式材质实例的标量/矢量/贴图参数数组在烘焙时未作为属性序列化（被写进内联 shader map），因此材质实例按父材质参数近似；完整材质图不复原；SkeletalMesh
LOD1+ 探测为尽力而为；UE4 旧式包的部分资产仍需映射才能显示字段名。

## 参考与许可

- FModel（GPL-3.0）：仅作为功能语义参考，未复制代码或界面。
- retoc（MIT）：IoStore/Zen 格式参考。
- aesdumpster-rs（MIT）与 GHFear/AESDumpster：密钥签名模式参考。
- 详细第三方声明见 `THIRD_PARTY_NOTICES.md`。
