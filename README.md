# Lumark

Lumark 是一个使用 Rust + [gpui-kit](https://github.com/longbridge/gpui-kit) 构建的 Windows 优先桌面压缩管理器。当前实现以可运行的文件浏览、ZIP/7Z/TAR 压缩解压、归档原子编辑和加密密码库为第一条完整链路。

## 当前功能

- 参考经典归档器的界面布局：自绘标题栏与工作区切换、渐变动作带、全宽路径与搜索导航行、常用位置和目录树合并的文件夹侧栏、底部详细信息检查器、全宽状态栏和统一模态层；宽窗口显示品牌标语，窄窗口自动收缩搜索框和模式标签。
- 界面优先使用 gpui-kit 官方组件：动作带和标题栏导航使用官方 Button 变体，格式、压缩方法、字典、单词、固实块、线程和时间筛选统一使用官方 Select 下拉框，文件右键菜单使用官方 ContextMenu/PopupMenu，文件表使用官方 Scrollbar，经典压缩窗口使用官方 Tabs、GroupBox、Radio 和 Checkbox，注释使用官方多行 Textarea。
- 主题色可在设置页通过官方 ColorPicker 自定义，默认蓝色；主按钮、动作带、选中态、进度条和状态栏会同步换色，并持久化到设置文件。
- 独立压缩窗口：轻巧模式为 600×390 的紧凑表单，经典模式为 780×650 的六选项卡布局；窗口独立于主界面，不置顶，轻巧/经典切换时保留全部已填参数。
- 独立任务进度窗口：压缩、解压和测试任务启动后自动弹出，显示目标路径、当前文件进度、总进度、已用/剩余时间、总大小、已压缩、压缩后、压缩率、文件数、处理速度和暂停、继续、取消、后台；取消需要二次确认，确认后立即停止并清理临时文件，关闭窗口不中断任务。
- 压缩后端使用 zlib-ng 和新版 ZIP/7Z 编码器，ZIP 按 1 MiB 分块读写并按块检查取消；7Z 通过进度读取器报告当前文件进度。
- 标题栏工作区按钮和模态遮罩使用阻塞命中区域，右上角密码库/设置按钮不再被拖拽区域吞掉，压缩/解压设置对话框也不会把鼠标事件穿透到下层文件表。
- IconPark 官方 SVG 图标已本地打包并按主题着色，运行时不依赖网络；来源与许可保存在 `crates/lumark-ui/assets/icons/iconpark/`。
- ZIP、7Z、TAR、TAR.GZ/BZ2/XZ/ZST；GZ、BZ2、XZ、ZSTD、Brotli、LZ4；ZIP/7Z AES 密码、可选的 ZipCrypto 兼容写入和 7Z 固实压缩。
- RAR 只读列表、密码验证和解压（UnRAR 后端，保留许可声明）；Unix `.Z` 与 AES `.enc` 只读解压。
- ISO/UDF、CAB、ARJ、ACE、ARC/PAK、CPIO、WIM、ASAR、ZPAQ、LZH/LHA、DEB、ISZ、IMG/BIN、UU/XXE、LZ/LZMA 等 30+ 只读格式适配。
- ZIP/7Z 归档内浏览、选择性解压、删除/重命名/添加文件，同名添加按替换处理，临时重建后原子替换；可选 `.lumark.bak`。
- `.001` 原始分卷创建、列表和解压；失败时保留旧卷并清理半成品。
- 密码管理：草稿式添加/编辑/删除、名称/备注/匹配规则、掩码与 10 秒显示、Windows DPAPI、可选主密码、加密导入导出。
- 解压加密 ZIP/7Z/RAR 时从密码库生成候选密码并在后台逐个验证；主密码锁定时提供解锁入口。
- 后台任务：归档加载、压缩、解压、测试、编辑、记录密码验证、进度、暂停/继续、取消、状态栏和独立进度窗口。
- 压缩后可测试归档并在成功时删除源文件。
- Windows 当前用户经典右键菜单、Send To、单实例、回收站删除、默认应用设置入口和安装/卸载脚本。
- 密码库 XChaCha20-Poly1305、Argon2id、原子写入、损坏 `.bak` 恢复、外部修改锁定和密码零化。

## 运行

```powershell
cargo run -p lumark-app
```

要求 Rust stable（当前环境为 1.99，工作区 MSRV 为 1.96），Windows x64，MSVC 工具链。首次构建会下载 GPUI Kit 及其依赖。

## 测试

```powershell
cargo test --workspace
```

测试覆盖：

- ZIP/7Z/TAR 系列/流式格式往返、AES 密码、固实压缩、原子编辑、分卷和路径穿越拒绝。
- 密码库 XChaCha20-Poly1305、Argon2id、DPAPI、导出导入、主密码、自动锁定、损坏恢复和密码匹配。
- 任务队列压缩/解压/测试和后台记录密码验证。
- 密码库候选密码自动解压加密 ZIP/7Z/RAR。
- 多选、范围选择、历史导航和高级搜索过滤。
- `gpui-kit::test` 真实窗口测试覆盖虚拟文件行和平铺视图切换。
- 真实 Windows 窗口已完成宽窗口、最小窗口、轻巧/经典压缩窗口、独立解压窗口、任务中心、独立进度窗口、密码库和设置的视觉验收，截图位于 `artifacts/`。

## 界面结构

`app.rs` 只保留应用状态、启动参数和生命周期，其余职责按功能拆分：

| 文件 | 职责 |
|---|---|
| `app/actions/files.rs` | 导航、选择、搜索、目录树和文件操作 |
| `app/actions/archive.rs` | 归档打开、压缩、解压、测试、输出目录选择和密码验证流程 |
| `app/actions/passwords.rs` | 密码草稿、解锁、主密码和导入导出 |
| `app/actions/settings.rs` | 主题、默认压缩参数和 Shell 集成 |
| `app/actions/tasks.rs` | 任务记录、状态文本和任务中心数据维护 |
| `app/view/chrome.rs` | 标题栏、渐变动作带、导航行、文件夹侧栏、状态栏和模态层 |
| `app/view/browser.rs` | 全宽文件表、搜索结果条和侧栏详细信息检查器 |
| `app/view/pages.rs` | 密码库、设置和任务中心 |
| `app/view/compress_window.rs` | 独立压缩窗口、轻巧/经典排版、官方 Select 算法与时间选项 |
| `app/view/extract_window.rs` | 独立解压窗口、目标目录、密码显示/隐藏和仅解压选中条目 |
| `app/view/dialogs.rs` | 确认对话框、密码提示和导入导出表单 |
| `app/view/windows.rs` | 独立任务进度窗口、任务控制和进度指标 |
| `app/view/components.rs` | IconPark 图标、渐变横幅、经典分组框、单复选控件和通用按钮 |
| `app/util.rs` | 路径、大小、日期、格式和界面格式化辅助函数 |

![Lumark 主界面](artifacts/ui-main-final.png)

![文件夹大小按需计算](artifacts/ui-folder-size.png)

![窄窗口布局](artifacts/ui-narrow.png)

![高级搜索匹配菜单](artifacts/ui-search-menu.png)

![轻巧压缩窗口](artifacts/ui-compress-light.png)

![经典压缩窗口](artifacts/ui-compress-dialog.png)

![官方 Select 下拉菜单](artifacts/ui-compress-select.png)

![文件右键菜单](artifacts/ui-context-menu.png)

![独立解压窗口](artifacts/ui-extract-window.png)

![任务中心](artifacts/ui-task-center.png)

![任务进度窗口](artifacts/ui-progress-window.png)

![取消任务二次确认](artifacts/ui-progress-cancel.png)

![密码管理](artifacts/ui-passwords-final.png)

![设置与主题色选择](artifacts/ui-theme-color.png)

## 工作区

| Crate | 职责 |
|---|---|
| `lumark-archive` | 格式检测、归档读写、路径安全、选择性解压 |
| `lumark-vault` | 加密密码库、DPAPI、主密码、密码匹配 |
| `lumark-tasks` | 后台任务、进度、暂停和取消 |
| `lumark-browser` | 文件系统与归档浏览、多选、搜索 |
| `lumark-settings` | 非敏感设置持久化 |
| `lumark-ui` | 基于 GPUI Kit 的桌面界面 |
| `lumark-app` | 应用入口 |
| `lumark-platform-windows` | 回收站、经典 Shell 菜单、单实例和 Windows 平台适配 |

## 设计取舍

- 密码库默认由 Windows DPAPI 保护；设置主密码后改用 Argon2id + XChaCha20-Poly1305，并移除 DPAPI 包裹。主密码无法找回。
- 密码列表默认掩码，单条显示 10 秒；复制后 30 秒只在剪贴板内容仍匹配时清空。
- 当前原生读写格式为 ZIP、7Z、TAR 系列、常用单文件流、Brotli/LZ4；其余 30+ 扩展名已纳入格式识别和界面类型判断，具体只读解码仍按 `IMPLEMENTATION_STATUS.md` 推进。
- SFX、Windows 11 稀疏 MSIX/新版菜单、托盘、会话锁定和 UI 自动化测试仍在后续阶段。
- 视频、图片、PDF 压缩按规划暂不实现。

## 许可与来源

本项目使用 MIT OR Apache-2.0。压缩核心 API 和格式思路参考 MIT 许可的 [GeeZipX](https://github.com/GEELINX-LTD/geezipx)。GPUI Kit 使用 Apache-2.0；ZIP 使用 MIT；sevenz-rust2 使用 Apache-2.0；界面图标来自 Apache-2.0 许可的 [IconPark](https://iconpark.oceanengine.com/official)。发布前请阅读 `THIRD_PARTY_NOTICES.md`。
