# 导出文件结构校验（E-16）

> 更新：2026-11-02。校验工具：`crates/lumark-export/examples/validate_exports.rs`。

## 用法

```powershell
cargo run -q -p lumark-export --example validate_exports -- <file>...
```

- GLB：使用官方 `gltf` crate 解析 JSON/二进制、遍历 mesh/primitive/node/skin；
- PSK/PSA：按公开 ActorX 布局独立解析 chunk 头/体（不复用写出器内部状态），校验版本、chunk 顺序、尺寸/数量、索引范围、四元数归一化、骨骼父索引与关键帧数量；
- UEMODEL：校验 `UEFORMAT` 魔数、`UEMODEL` 标识、版本 10、对象名/路径、根属性集（`LODS`/`SKELETON`）及字节完全消费；
- USDA：校验 `#usda 1.0`、单位、`Root`/`Mesh`/`Material` 基元、花括号/引号配对与独立 NaN/Inf 数值。

任一失败返回非零，并逐项打印原因。

## BNB 实机结果（2026-11-02）

| 文件 | 结果 | 关键信息 |
|---|---|---|
| `target/BNB_SK_Chicken.glb` | PASS | meshes=1 primitives=1 nodes=43 skins=1，generator=Lumark |
| `target/BNB_SK_Chicken.psk` | PASS | 顶点 2860 / 三角形 3844 / 权重 7699 / 骨骼 42，chunk 顺序完整 |
| `target/BNB_ANIM_Chicken_Idle2.psa` | PASS | 骨骼 42 / 帧 141 / fps 30.21 / 关键帧 5922，chunk 顺序完整 |
| `target/BNB_SK_Chicken.uemodel` | PASS | version=10，roots=`LODS,SKELETON` |
| `target/BNB_SK_Chicken.usda` | PASS | 363,660 字节 / 49 行 / 结构配对 |
| `target/asset-export/SM_Box_02.glb` | PASS | meshes=1 nodes=1 |
| `target/asset-export/SM_Box_02.psk` | PASS | 顶点 844 / 三角形 642 / 静态无蒙皮 |
| `target/asset-export/SM_Box_02.uemodel` | PASS | roots=`LODS`，静态网格 |
| `target/asset-export/SM_Box_02.usda` | PASS | 98,038 字节 / 结构配对 |

汇总：**9 个文件，9 通过，0 失败**。

## 覆盖范围与限制

- 已覆盖：容器/头/计数/尺寸/索引/关键帧/骨骼/纹理路径等结构一致性；
- GLB 由官方 crate 完整解析，属于外部实现验证；
- PSK/PSA/UEMODEL/USDA 为独立布局解析器，不依赖写出代码，但仍不是第三方 DCC 导入；
- 未完成的真实验收：在 Unreal Engine / Blender / USD Composer 中实际导入并核对蒙皮动画/材质显示（本机无这些工具或未自动化）；
- USDA 目前只做结构/数值检查，未验证 USD Skel/动画（E-10 未实现部分）。
