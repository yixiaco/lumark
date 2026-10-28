---
name: lumark-code-style
description: 维护 Lumark 项目的注释、模块边界和界面资源约定；在修改该仓库的 Rust、PowerShell、IconPark SVG 或项目文档时使用。
---

# Lumark 项目规范

## 注释规范

- 所有代码注释、模块文档和函数文档统一使用简体中文。
- 标识符、协议名、命令、API 名称和必要的英文技术名词保持原样，不为了翻译而改代码。
- 注释说明设计意图、关键约束、失败策略和安全边界，不复述代码表面行为。
- 每个函数必须有 `///` 职责注释；模块入口使用 `//!` 说明职责范围。
- 修改注释时只改变说明文字，不改变代码行为；完成后运行格式化和测试。

## 模块边界

- `crates/lumark-ui/src/app.rs` 只保留应用状态、启动参数、构造与窗口生命周期，以及子模块入口。
- 文件、归档、密码、设置和后台任务动作放在 `app/actions/`；任务事件桥、Shell 压缩批次和后台宿主托盘归入 `app/actions/tasks.rs`。
- 主框架、文件表、页面、弹窗和通用组件放在 `app/view/`。
- 主题模式、强调色派生和全局主题 token 放在 `app/theme.rs`，由 `app.rs` 统一再导出；页面不要硬编码颜色。
- 通用路径、大小、日期和格式化辅助函数放在 `app/util.rs`。
- 启动参数和真实窗口 UI 测试放在 `app/tests.rs`，`app.rs` 只保留 `#[cfg(test)] mod tests;` 入口。
- 页面文件按页面和组件拆分：`app/view/pages.rs` 只保留 `mod` 声明，密码库、设置、任务中心分别放在 `app/view/pages/` 下，跨页面复用的统计卡片等放入 `pages/shared.rs`。
- 单个页面文件接近 600 行，或开始容纳第二个页面、第二组独立组件时，必须先拆分再继续添加功能，禁止把多个页面堆在同一文件。
- `app.rs` 超过约 800 行通常意味着动作或视图回流：新增逻辑优先放入现有职责模块，确需新模块时保持单向依赖。
- 子模块通过 `use super::super::*` 复用 `app.rs` 的共享导入；只有测试直接调用的动作才提升为 `pub(crate)`。

## 界面资源

- UI 固定使用 `gpui-kit 0.8.1`，压缩核心固定使用 `zip 9.0.5` 和 `sevenz-rust2 0.24.1`。
- 图标使用 IconPark 官方 SVG，存放在 `crates/lumark-ui/assets/icons/iconpark/`，运行时不访问网络。
- SVG 使用 `currentColor` 跟随主题；新增图标时保留上游许可和来源说明。
- 界面必须同时检查宽窗口单行命令栏和窄窗口双行命令栏，避免路径、搜索和主操作被裁切。

## 视觉样式

- 矩形控件、标签、徽标和容器的圆角使用 gpui-kit 主题 token：控件优先 `theme.radius`，卡片和浮层使用 `theme.radius_lg`；手写固定值沿用现有尺度（约 6–14px），不得随元素高度放大。
- 禁止用 `rounded_full()`、`rounded_full_style()` 或接近短边一半的圆角把矩形元素做成胶囊或椭圆；矩形始终保持直角边配统一的小圆角。
- 只有本身为圆形的元素可以使用全圆角，例如状态圆点、头像和圆形图标底座；进度条按 gpui-kit Progress 规范使用高度一半的胶囊圆角。
- 新增或重构界面前先查看 gpui-kit 对应组件的默认圆角，沿用组件样式，不自行放大。

## 提交

- 提交消息保持简短，一句中文概括改动即可，不写长段落或逐项清单。
- 提交前确认工作区只包含本次改动的文件，并确保格式化和测试通过。

## 验证

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p lumark-app
```

DPAPI 测试需要在可访问当前 Windows 用户密钥存储的普通用户会话中运行。
