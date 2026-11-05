# Snooper 延迟管线迁移

## 已完成

- G-buffer 着色器：位置/法线/基础色/高光，复用蒙皮/实例与材质绑定。
- G-buffer 目标：4×Rgba16Float + 独立深度。
- G-buffer 不透明预通道：Snooper 模式先写 G-buffer，BNB 实机验证无 wgpu 错误。

## 已完成

- G-buffer → 延迟 PBR/级联阴影/雾/天空。
- 全屏 grid 写 G-buffer。
- 半透明/接触阴影/骨骼 overlay 在延迟后合成。
- GPU FXAA 最终 pass。
- CPU 回退：大气 LUT、全屏 grid、SSAO、屏幕阴影、雾、FXAA。
- 缩略图走同一 GPU 延迟路径。

## 后续可选

- 软件 CPU 延迟 G-buffer 以做到逐像素一致。
- 缩略图分辨率跟随查看器。

## 原有计划

- 延迟光照 pass：G-buffer → PBR + 4 级联阴影 + 高度雾；天空由现有背景 LUT pass 保留。
- 半透明前向叠加与 overlay：已接入延迟后的 color Load + G-buffer 深度。
- 全屏 grid：射线求交写 G-buffer，替换延迟路径中的几何地面与 overlay 网格。
- 半透明前向叠加与 combine。
- 切换 Snooper 默认路径，旧前向保留回退。

## 验收

- 延迟光照输出与 Snooper 截图/CPU 回退对比。
- 缩略图跟随查看器分辨率与效果开关。
