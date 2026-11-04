# 图片查看器（I-1…I-5）

> 更新：2026-11-03。模块位置：`crates/lumark-ui/src/app/view/image_viewer_window/`。

## 结构（I-1）

| 文件 | 行数 | 职责 |
|---|---|---|
| `mod.rs` | ~305 | 状态、构造、缩放/平移/旋转、画布度量、Render 组合 |
| `title.rs` | ~47 | 标题栏与文件信息 |
| `toolbar.rs` | ~91 | 适应/100%/缩放/旋转等操作 |
| `body.rs` | ~146 | 画布、绝对定位布局、滚轮/拖拽输入、空状态 |
| `status.rs` | ~90 | 尺寸/格式/缩放状态栏 |
| `image_ops.rs` | ~388 | 预检、EXIF 方向校正、旋转、SVG 尺寸、缩放/布局数学 |
| `app.rs` | ~76 | 打开图片/内置查看器入口 |
| `tests.rs` | ~210 | EXIF/旋转/滚轮方向/绝对布局/拖拽/锚点测试 |

全部源码模块 <600 行。

## 布局

- 画布 `div#image-canvas`（`flex_1`、`overflow_hidden`）内只有一个全尺寸
  `relative` 舞台；画布在 `on_children_prepainted` 中把舞台 bounds 写入
  `Rc<Cell<Option<Bounds<Pixels>>>>`，即画布的精确窗口坐标与尺寸。
- 适应窗口（`fit`）：舞台内的 `img` 用 `size_full` + `ObjectFit::Contain`，
  保持比例并在窗口内留边。
- 手动缩放/平移：`img` 绝对定位，尺寸 = `原图尺寸 × zoom`，位置
  `left = 画布中心 + pan.x - 图片宽 / 2`、`top` 同理；图片 `flex_shrink_0` +
  `ObjectFit::Fill`。宽高与 `pan` 无关，因此拖拽和缩放到窗口边缘时都不会被
  flex 收缩成非等比压扁。
- 画布 bounds 变化（窗口缩放）时，prepaint 回调追加一帧
  （`Window::request_animation_frame`），让手动缩放模式用新的画布中心重新布局。

## 交互

| 操作 | 行为 |
|---|---|
| 滚轮 | GPUI/Windows 语义下向上 `delta.y > 0` **放大**、向下缩小；指数缩放 `zoom × exp(delta.y × 0.0025)`，**以光标为锚点**（`anchored_pan`） |
| 左键拖拽 | 手动缩放模式下按绝对坐标 1:1 平移：`pan = 按下时的 pan + (光标 - 按下点)`，每次都从按下点重算，不累积布局回流误差；适应窗口模式禁止拖拽 |
| 适应窗口 | `fit = true`、清空 `pan`，按 Contain 显示 |
| 100% | `zoom = 1` 并清空 `pan` |
| `−` / `+` | 以画布中心为锚点按 1.25 倍缩放，可从适应窗口比例连续放大/缩小 |
| 左转/右转 | 生成旋转后的临时文件（不修改原图） |

缩放范围：常规 0.05–16 倍；小图在适应窗口被放大到 16 倍以上、或全景图缩小到
5% 以下时，范围扩展到适应窗口比例，避免离开适应模式时比例跳变或滚轮方向反转。

缩放锚点公式（`cursor`、`center` 均为窗口坐标）：

```
pan' = pan + (1 - new_zoom / old_zoom) × (cursor - canvas_center - pan)
canvas_center = canvas_bounds.origin + canvas_bounds.size / 2
```

## 图片处理

- 静态 JPEG/PNG 读取 EXIF Orientation，必要时生成校正后的临时 PNG；
- 动图/SVG 不做方向校正，避免破坏动画/矢量渲染；
- SVG 通过 `width/height` 或 `viewBox` 预检尺寸；
- 所有临时文件在窗口关闭时删除；
- 图片只读，不写回原文件或归档。

## 测试

- EXIF 方向 6 宽高互换；
- 无 EXIF 保持原文件；
- 左右旋转临时文件宽高；
- 显示名优先资产名；
- 滚轮方向（向上放大）与缩放范围；
- 绝对布局矩形：2 倍缩放居中、平移只改位置不改宽高、缩小后仍居中；
- 画布中心与适应窗口比例（精确 bounds）；
- 拖拽按绝对坐标 1:1 且图片尺寸不变；
- 适应窗口 → 手动缩放的锚点连续性；
- 光标锚点 3 组断言；
- UI 集成（`app/tests/ui/viewer.rs`）：PNG、EXIF 旋转图片打开，旋转/重置。

## 支持范围

- 单图查看：适应窗口、滚轮锚点缩放、拖拽平移、左右旋转、EXIF 方向校正、SVG 尺寸识别；
  多图/缩略图列表不在该只读窗口的范围内。
- 超大图由 GPUI 解码，并在 1 亿像素 / 512 MiB 前拒绝，避免解码占用失控。
- 指针在画布外松开左键时，拖拽状态会在指针回到画布后的首次移动中清理，不影响随后操作。
