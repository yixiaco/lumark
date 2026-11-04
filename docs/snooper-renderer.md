# Snooper 渲染对齐设计

## 状态

- [x] P0 质量切换/空白模型/预览缓存修复
- [x] S-1 Snooper 默认环境、设置结构与共享预设
- [ ] S-2 GPU G-buffer + 延迟光照
- [x] S-3 级联阴影（两级联太阳阴影贴图 + 3×3 PCF）
- [x] S-4 SSAO（深度重建屏幕空间 AO，全分辨率乘色）
- [ ] S-5 天空盒与雾（天空球蓝渐变/太阳盘已接入 GPU 背景，大气 LUT/雾待完成）
- [ ] S-6 无限地面/网格（深色地面已接入查看器场景，阴影/SSAO/雾待完成）
- [x] S-7 半透明/FXAA/性能（Snooper 模式 GPU 回读后 FXAA-lite）
- [x] S-8 预览/UI（工具栏“效果”开关；CPU 完整回退待做）
- [ ] S-9 验收

## 默认环境

参考 Snooper `SunActor`/`EnvironmentActor`：

- 太阳强度 10、颜色 `(1.0, 0.96, 0.90)`、太阳角 0.5357°
- 天空光上 `(0.6, 0.7, 0.8)`、下 `(0.4, 0.35, 0.3)`、强度 1.5
- 共享预算 1.25，按 `SceneLighting.cs` 解析为约 2.94 的太阳强度和
  `(0.235, 0.275, 0.314)` / `(0.157, 0.137, 0.118)` 天空色
- 默认太阳方向（UE Z-up，指向太阳）`(0.766, 0.643, 0.0)`

实现位置：`lumark-render::SnooperEnvironmentSettings::resolve`；
查看器与详情缩略图统一使用 `RenderSettings::snooper_viewer`。

## 目标管线

```
阴影级联 → G-buffer（position/normal/albedo/spec/depth）→ 无限地面
→ SSAO 半分辨率 + 双边模糊 → 延迟光照（PCF/SSAO/雾/天空）
→ 半透明前向 → FXAA → 回读
```

- Snooper 模式禁用 SS/MSAA，使用 FXAA；查看器默认 1.5× 输出分辨率，可切 1.0×/1.5×/2.0×。
- 拖动 0.75×、2 级联 1024²；静止 1×、4 级联 2048²。
- 网格 LOD 固定，不随交互切换；`fit_bounds` 取全部 LOD 并集，pan 使用逻辑高度。
- 天空/地面为全屏渲染，不进入 `Scene` 包围盒。

## 当前实现约束

- 全部效果关闭时，Snooper 前向路径必须保持可用。
- Studio/Headlight 路径不进入新管线。
- 详情缩略图与查看器共用 `SNOOPER_PRESET_VERSION`，版本变化使缓存失效。
