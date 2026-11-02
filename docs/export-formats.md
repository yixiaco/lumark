# 导出格式说明（E-17）

> 更新：2026-11-02。对应 `lumark-export` 公共 API 与批量导出。

## 模型格式

| 格式 | 写出函数 | 能力 | 限制 |
|---|---|---|---|
| ActorX PSK | `write_psk` | 静态/骨骼网格、section 材质、顶点/法线/切线、UV0..n、顶点色、蒙皮权重、参考骨架；Y 镜像 | 单 LOD、不写材质贴图、无 morph target/插槽 |
| ActorX PSA | `write_psa` | 逐帧骨骼关键值（position/quat/scale），版本 20100422，含 `ANIMKEYS`/`SCALEKEYS` | 参考缩放固定 1（`ExportBone` 无缩放字段）；单动画序列 |
| glTF 2.0 GLB | `write_glb` | 网格/section、PBR 材质、基础色/法线贴图内嵌 PNG、skin + inverse bind matrices、节点层级；Z-up→Y-up、单位缩放可配 | 不写动画/相机/灯；alpha 仅裁剪；贴图内嵌不外部引用 |
| UEFormat UEMODEL | `write_ueformat_model` | v10、LOD 属性集、材质、UV0..n、顶点色、蒙皮权重、骨架；TANGENTS 自动估算 | 单 LOD、不写动画/morph/碰撞 |
| USDA | `write_usda` / `write_usda_with_animation` | Mesh、法线/UV/顶点色、GeomSubset 多材质、`UsdPreviewSurface` 自动材质、基础色贴图侧车；骨骼资产写 `SkelRoot/Skeleton`（joints/bindTransforms/restTransforms）与 `SkelBindingAPI` 蒙皮 primvars；可选 `SkelAnimation`（translations/rotations/scales timeSamples、`skel:animationSource`） | 动画需调用方按帧采样；法线贴图未接线 |

### 坐标与单位

- PSK/PSA/UEMODEL 保持 UE 坐标（Z-up、厘米），PSK/PSA 按约定镜像 Y；
- glTF/USDA 转为 Y-up（`(x,y,z)→(x,z,-y)`）；glTF 由 `ExportOptions::scale` 控制单位（建议 0.01 厘米→米），USDA 由缩放推导 `metersPerUnit`。

### 公共输入

`ExportMesh` + `ExportBone` + `ExportOptions{scale, vertex_colors, extra_uvs}`；所有格式共用 `actorx::validate` 检查索引、section、蒙皮长度和骨骼父索引。

## 贴图格式（E-3）

`encode_texture(&ExportTexture, &TextureExportOptions{format, quality})`：

| 格式 | 扩展名 | 特性 |
|---|---|---|
| PNG | `.png` | 无损、保留 alpha |
| JPEG | `.jpg` | 有损、质量 1–100、忽略 alpha |
| TGA | `.tga` | 未压缩 32 位 BGRA、top-left 原点 |
| WebP | `.webp` | 无损 |
| HDR | `.hdr` | Radiance RGBE（32-bit_rle_rgbe 头、非 RLE 扫描线），忽略 alpha |

输入统一为 RGBA8；尺寸/像素长度不一致返回 `InvalidTexture`。HDR 按线性 RGB→RGBE 编码，适用于环境/光照类贴图；普通 PBR 基础色建议用 PNG/TGA。BNB `MI_Props` 实机 HDR 侧车：512×512、1,048,625 字节（两张贴图 + materials.json）。

## 材质旁挂（E-4）

`material_sidecars(&[ExportMaterial], &MaterialSidecarOptions{format, quality})` 返回资源列表：

```
textures/<材质>_BaseColor.<ext>
textures/<材质>_Normal.<ext>
materials.json
```

`materials.json` 结构：

```json
{
  "version": 1,
  "generator": "Lumark",
  "textureFormat": "png",
  "materials": [
    {
      "name": "MI_Props",
      "baseColor": [1.0, 1.0, 1.0, 1.0],
      "metallic": 0.0,
      "roughness": 0.55,
      "emissive": [0.0, 0.0, 0.0],
      "twoSided": false,
      "textures": { "baseColor": "textures/MI_Props_BaseColor.png", "normal": "textures/MI_Props_Normal.png" }
    }
  ]
}
```

`write_resources(base, resources)` 安全落盘，拒绝绝对路径与 `..`。

## 批量导出（E-12/E-13 部分）

```rust
let report = export_archive_cancellable(
    &mut source, &mappings, &config,
    Some(cancel.as_ref()),       // Arc<AtomicBool>，任务面板/UI 使用
    |progress| { /* Analyzing/Exported/Skipped/Failed/Cancelled */ },
)?;
```

- `BatchExportConfig`：输出目录、格式集合、缩放、材质/贴图上限、过滤、limit、overwrite；
- `BatchExportReport`：成功资产/文件、跳过、失败、`cancelled`、`elapsed_seconds`，可用 `to_json_pretty()` 写 `batch-report.json`；
- 每资产失败不中断整批；取消在下一资产开始前生效；
- 仍未接入 `TaskManager`/任务面板 UI（E-13 剩余、E-15）。

## 验证记录

- `lumark-export` 21 个测试：五种模型格式 + 五种贴图格式 + 材质旁挂 + PSA；
- BNB 实机：`SK_Chicken.glb/.psk/.uemodel/.usda/.psa`、`SM_Dry_Goods_04g` 四格式、`SM_Box_02` GLB/USDA、`MI_Props` 四格式贴图侧车；
- GLB 通过 `gltf::Gltf::from_slice` 校验；PSK/PSA/UEMODEL/USDA 有独立 chunk/结构解析示例。
