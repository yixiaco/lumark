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

- `crates/lumark-ui/src/app.rs` 只保留应用状态、启动参数、生命周期和测试入口。
- 文件和归档操作放在 `app/actions/`，密码和设置动作放在对应 action 模块。
- 主框架、文件表、页面、弹窗和通用组件放在 `app/view/`。
- 通用路径、大小、日期和格式化辅助函数放在 `app/util.rs`。
- 新增功能优先放入现有职责模块；确需新模块时保持单向依赖，避免把界面逻辑重新堆回 `app.rs`。

## 界面资源

- UI 固定使用 `gpui-kit 0.7.3`，压缩核心固定使用 `zip 9.0.5` 和 `sevenz-rust2 0.24.1`。
- 图标使用 IconPark 官方 SVG，存放在 `crates/lumark-ui/assets/icons/iconpark/`，运行时不访问网络。
- SVG 使用 `currentColor` 跟随主题；新增图标时保留上游许可和来源说明。
- 界面必须同时检查宽窗口单行命令栏和窄窗口双行命令栏，避免路径、搜索和主操作被裁切。

## 验证

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p lumark-app
```

DPAPI 测试需要在可访问当前 Windows 用户密钥存储的普通用户会话中运行。
