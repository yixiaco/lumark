---
name: lumark-code-style
description: 维护 Lumark 项目的注释、模块边界、界面资源和工作区完整性约定；在修改该仓库的 Rust、PowerShell、IconPark SVG 或项目文档时使用。
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
- 主题模式、强调色派生和全局主题 token 放在 `crates/lumark-ui-shared/src/theme.rs`，由 `app.rs` 统一再导出；页面不要硬编码颜色。
- 通用路径、大小、日期和格式化辅助函数放在 `app/util.rs`。
- 启动参数和真实窗口 UI 测试放在 `app/tests.rs`，`app.rs` 只保留 `#[cfg(test)] mod tests;` 入口。
- 页面文件按页面和组件拆分：`app/view/pages.rs` 只保留 `mod` 声明，密码库、设置、任务中心分别放在 `app/view/pages/` 下，跨页面复用的统计卡片等放入 `pages/shared.rs`。
- 单个页面文件接近 600 行，或开始容纳第二个页面、第二组独立组件时，必须先拆分再继续添加功能，禁止把多个页面堆在同一文件。
- `app.rs` 超过约 800 行通常意味着动作或视图回流：新增逻辑优先放入现有职责模块，确需新模块时保持单向依赖。
- 子模块通过 `use super::super::*` 复用 `app.rs` 的共享导入；只有测试直接调用的动作才提升为 `pub(crate)`。

## 界面资源

- UI 固定使用 `gpui-kit 0.8.1`，压缩核心固定使用 `zip 9.0.5` 和 `sevenz-rust2 0.24.1`。
- 图标使用 IconPark 官方 SVG，存放在 `crates/lumark-ui-shared/assets/icons/iconpark/`，运行时不访问网络。
- 主程序共用的主题、i18n、IconPark/品牌资源、通用控件、格式化函数和进度面板放在 `crates/lumark-ui-shared/`。
- 自解压运行时使用**原生 Win32 界面**（`main.rs` + `setup.rs` + `progress.rs` + `win.rs`），参考 360 压缩设置/进度窗口的布局；2026-11-03 实机确认 GPUI/DirectX stub 会被 360 QVM 判为木马，原生 stub 不触发。不要重新引入 GPUI，也不要让 `lumark-sfx` 依赖 `lumark-ui` 或 `lumark-ui-shared`。
- 自解压设置窗口按客户区高度布局，底部按钮锚定客户区底部；Windows 默认 DPI 缩放坐标和字体，禁止再把窗口外框高度当作控件坐标。进度窗口必须在后台线程解压，通过定时器刷新，取消标记在读写块之间检查。
- SVG 使用 `currentColor` 跟随主题；新增图标时保留上游许可和来源说明。
- 界面必须同时检查宽窗口单行命令栏和窄窗口双行命令栏，避免路径、搜索和主操作被裁切。

## 视觉样式

- 矩形控件、标签、徽标和容器的圆角使用 gpui-kit 主题 token：控件优先 `theme.radius`，卡片和浮层使用 `theme.radius_lg`；手写固定值沿用现有尺度（约 6–14px），不得随元素高度放大。
- 禁止用 `rounded_full()`、`rounded_full_style()` 或接近短边一半的圆角把矩形元素做成胶囊或椭圆；矩形始终保持直角边配统一的小圆角。
- 只有本身为圆形的元素可以使用全圆角，例如状态圆点、头像和圆形图标底座；进度条按 gpui-kit Progress 规范使用高度一半的胶囊圆角。
- 新增或重构界面前先查看 gpui-kit 对应组件的默认圆角，沿用组件样式，不自行放大。

## 按钮尺寸

- 常规操作按钮一律使用 gpui-kit 默认尺寸（`Button` 默认 Medium，约 32px 高），**不要**调用 `.xsmall()` 或 `.small()` 缩小；需要更高密度时优先调整容器布局、内边距或换行，而不是缩小按钮。
- 批量选择、格式切换、数量限制、设置页操作、导出/确认/取消、详情面板操作都属于常规按钮，必须使用标准尺寸。
- 例外仅限固定高度的标题栏、状态栏、工具栏中的纯图标按钮，或确实无法容纳标准按钮的单行密集区域；使用例外时必须在代码注释说明原因。
- `Tag`、徽标、状态标签不属于按钮，保留其组件默认的小尺寸语义，不要为了“统一”放大。

## 交互反馈

- 按钮必须有指针手 `.cursor_pointer()`：本项目按产品决定执行，有意覆盖官方「按钮使用默认箭头」的默认值。
- 链接，以及行为等同链接的内容（打开 URL、外部文档、邮件地址）同样使用手型。
- 复选框、菜单项、标签页等其余原生控件仍按官方约定使用默认箭头；要改判先确认，不要顺手统一。
- 官方 `Button` 在 `render` 里强制 `.cursor_default()`，会覆盖调用处的 `.cursor_pointer()`，只在调用点加链式方法无效；落地方式见下一条。
- 实现按钮手型只有两条路：给 `vendor/gpui-component` 打一行补丁，让可交互按钮走手型（推荐）；或改用在 `link` / `text` 变体。禁止为此复制组件的视觉实现。
- 需要其它光标时按语义选择：可选中文字 `.cursor_text()`、拖拽 `.cursor_grab()` / `.cursor_grabbing()`、不可用 `.cursor_not_allowed()`、缩放 `.cursor_ew_resize()` / `.cursor_ns_resize()`。
- 出处：`gpui-kit-design-guides` 的「Pointer conventions」；按钮一项是本仓库有意偏离官方默认。

## 工作区完整性

- **禁止将工作区根目录标记为 Low**：不得对仓库根目录执行 `icacls /setintegritylevel Low`，也不得让沙箱、构建脚本、测试或临时命令把仓库根目录设置为低完整性；根目录必须保持 Medium。
- 根目录的 Low 标签会传播到 `target` 下新生成的 exe，触发 Windows“打开文件 - 安全警告”。发现根目录是 Low 时，应恢复为 Medium（`icacls <仓库根目录> /setintegritylevel "(OI)(CI)Medium"`），并用 `icacls .\target\release\lumark-app.exe` 确认产物也是 Medium；不要用签名或 MOTW 清理掩盖。
- 低完整性/沙箱实验只能在临时目录进行，禁止用仓库根目录测试；发布构建使用 `tools/build_release.ps1`，它会恢复根目录和 `target`/`dist` 子目录的完整性标签。

## 测试代码

- 一次性测试代码（临时验证脚本、调试用例、临时 `#[test]`、`print!/println!`、`dbg!` 输出、临时改写的调用点）只服务于当前这一次验证，验证完成后必须删除，不得留在工作区，也不得提交。
- 删除后重新跑一遍 `cargo test --workspace`，确认正式用例仍然通过；不要靠“反正没人跑”把临时测试留在仓库里。
- 需要长期保留的回归覆盖，迁入 `crates/lumark-ui/src/app/tests.rs` 或对应 crate 的正式测试位置，随 `cargo test --workspace` 一起运行；不要用 `#[ignore]` 的临时用例冒充正式测试。
- 提交前用 `git status` 和 `git diff` 复核，确认没有残留的临时测试文件、调试输出或只为验证而改的代码。

## 提交

- 提交消息保持简短，一句中文概括改动即可，不写长段落或逐项清单。
- 提交前确认工作区只包含本次改动的文件，并确保格式化和测试通过。

## 验证

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p lumark-app
cargo build --release -p lumark-sfx
```

DPAPI 测试需要在可访问当前 Windows 用户密钥存储的普通用户会话中运行。
