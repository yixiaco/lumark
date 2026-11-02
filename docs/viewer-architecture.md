# Lumark 查看器渲染架构（V-9）

> 更新：2026-11-02（目标模式第 22 轮）。对应计划 V-0…V-6 的实现说明与验证基线。

## 分层

```
lumark-unreal（资产读取/材质解析）
        │  Scene（引擎无关数据）
        ▼
lumark-scene：Scene / SceneNode / MeshData（含 MeshSkin）/ MaterialData / TextureData
        │
        ▼
lumark-render：CPU 光栅（raster） + wgpu 30 离屏（gpu）
        │  RenderTarget（RGBA8，to_bgra）
        ▼
lumark-ui/render_service：RenderImage（缩略图）；完整查看器帧泵见 V-16
```

- 引擎无关层不引用 Unreal/GPU/UI；未来 Unity Provider 复用同一 `Scene`。
- 坐标系：UE Z-up（`scene.up_axis`），渲染层直接使用场景坐标；`unit_scale` 仅作记录。

## 公共 API

- `RenderSettings`：宽高、背景、相机轨道参数（yaw/pitch/distance/target）、双灯颜色与强度、环境光、超采样（1–4）；
- V-4 增补：`shading: ShadingMode { Lit, Unlit, Normals }`；
- V-6 增补：`show_grid`、`show_bones`、`hidden_material_slots`；
- `RenderTarget`：RGBA8 + `to_bgra()`（GPUI `RenderImage` 用）；
- `Renderer`（CPU）/ `GpuRenderer`（wgpu）均输出 `RenderTarget`，上层可统一回退。

## 渲染管线

1. **相机**：根据场景包围盒自动拟合轨道相机（`Camera::fit`），透视投影深度 0..1。
2. **批次/实例化**：按「网格 + section + 材质 + 蒙皮节点」合批；静态网格共享本地空间顶点缓冲，每个节点只上传实例矩阵（GPU 实例化）；蒙皮网格保留 bind 空间顶点并独占批次。
3. **蒙皮**：`MeshData.skin`（关节/inverse bind/≤4 影响/姿态）；CPU 逐顶点 LBS；GPU 顶点含 joints/weights，批次上传关节矩阵 storage buffer，WGSL 内 top-4 归一化混合。
4. **着色**：基础色/法线/ORM（R=AO/G=粗糙度/B=金属度）/自发光贴图，金属度参与高光强度；alpha < 0.5 裁剪；单面材质剔除背面、双面材质保留。
5. **叠加层**：地面网格与骨骼线段（LineList 管线，深度测试不写深度）；CPU 用 DDA 3D 线段光栅。
6. **输出**：颜色目标 `Rgba8Unorm`，着色器手动 tone mapping + gamma；超采样后盒式降采样回目标尺寸。

## 快捷键

| 按键 | 行为 |
|---|---|
| `F` | 重置相机（yaw 35°/pitch 25°/zoom 1×） |
| `Space` | 播放/暂停动画 |
| `←` / `→` | 动画逐帧后退/前进 |
| `L` | 着色模式循环（光照→无光→法线） |
| `G` | 地面网格开关 |
| `B` | 骨骼线开关 |
| `N` | 世界法线着色 |

正式查看器窗口创建时获取焦点；Ctrl/Alt 组合不拦截。动画时间轴使用官方 `Slider`，拖动/点击即跳转并暂停播放，播放/步进/重置后自动同步滑块位置。

## 着色模式

| 模式 | CPU | GPU |
|---|---|---|
| `Lit` | 双灯 + 环境光 + PBR 近似 | 同 CPU（WGSL） |
| `Unlit` | 基础色（含贴图/顶点色） | 同左 |
| `Normals` | 世界法线映射到 0–1 | 同左 |

## 测试与基线

- `lumark-render` 单元测试：**32 个**（CPU：相机裁剪/z-buffer/PBR/ORM/自发光/alpha/双面/蒙皮/着色模式/网格与隐藏槽/场景统计；GPU：三角形/贴图/ORM/自发光/双面/蒙皮/着色模式/网格/两节点实例化，无 GPU 自动跳过）。
- 本机基线（NVIDIA RTX 5070 Laptop，Vulkan，480×480，2×SS，含回读同步）：
  - `SM_Box_02 + MI_Props`：约 72 ms；
  - `SK_Chicken` 静态：约 83 ms；
  - `SK_Chicken + ANIM_Chicken_Idle2` 动画帧：约 87–93 ms。
- CPU 同资产约 200 ms，GPU 用于交互与缩略图，CPU 作为无 GPU 回退。

## 已知限制（对应后续步骤）

- 渲染线程/异步双 staging/device lost 重建：待 V-2/V-8；GPU 渲染错误已自动回退 CPU；
- MSAA、纹理 LRU、sRGB 视图切换：待 V-8；
- wireframe 模式（DX12 兼容方案）与 section 逐项 UI：待 V-6 后续/V-17；
- 透明仅 alpha-test，无排序混合；
- 世界 Actor 实例化：静态网格已支持场景级 GPU 实例化（共享顶点 + 实例矩阵，V-7 完成）；世界 Provider 仍待 V-14；
- 完整查看器仍按需生成 PNG，RenderImage 帧泵待 V-16。
