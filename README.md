# Lumark

Lumark 是一个使用 Rust + [gpui-kit](https://github.com/longbridge/gpui-kit) 构建的 Windows 优先桌面压缩管理器。当前实现以可运行的文件浏览、ZIP/7Z/TAR 压缩解压、归档原子编辑和加密密码库为第一条完整链路。

## 当前功能

- 参考经典归档器的界面布局：自绘标题栏与工作区切换、渐变动作带、全宽路径与搜索导航行、常用位置和目录树合并的文件夹侧栏、底部详细信息检查器、全宽状态栏和统一模态层；宽窗口显示品牌标语，窄窗口自动收缩搜索框和模式标签。
- 界面优先使用 gpui-kit 官方组件：动作带和标题栏导航使用官方 Button 变体，格式、压缩方法、字典、单词、固实块、线程和时间筛选统一使用官方 Select 下拉框，文件右键菜单使用官方 ContextMenu/PopupMenu，文件表使用官方 Scrollbar，经典压缩窗口使用官方 Tabs、GroupBox、Radio 和 Checkbox，注释使用官方多行 Textarea。
- 设置使用独立选项卡窗口，参考经典归档器分为综合、外观、压缩、文件关联、密码库和关于；主题色可在外观页通过官方 ColorPicker 自定义，默认蓝色，主按钮、动作带、选中态、进度条和状态栏会同步换色，并持久化到设置文件。
- 独立压缩窗口：轻巧模式为 600×330 的紧凑表单，经典模式为 600×390 的六选项卡布局；窗口独立于主界面，不置顶，轻巧/经典切换时保留全部已填参数。多文件或文件夹源会隐藏 GZ/BZ2/XZ/ZST/Brotli/LZ4 等单文件流格式；若默认格式是其中之一，会自动改用对应的 TAR 变体。
- 独立任务进度窗口：压缩、解压和测试任务启动后自动弹出，显示目标路径、当前文件进度、总进度、已用/剩余时间、总大小、已压缩、压缩后、压缩率、文件数、处理速度和暂停、继续、取消、后台；取消需要二次确认，确认后立即停止并清理临时文件，关闭窗口不中断任务。
- 压缩后端使用 zlib-ng 和新版 ZIP/7Z 编码器，ZIP 按 1 MiB 分块读写并按块检查取消；7Z 多线程 LZMA2 默认按 16 MiB 编码块完成量报告真实进度（字典更大时随库钳制），两次块事件之间按最近速度平滑显示并在块完成时校正；单线程/其他算法按已写入编码器的字节更新，加密归档保留读取进度，7Z 解压按 256 KiB 分块、100 毫秒节流上报单文件内部进度。
- 标题栏工作区按钮和模态遮罩使用阻塞命中区域，右上角密码库/设置按钮不再被拖拽区域吞掉，压缩/解压设置对话框也不会把鼠标事件穿透到下层文件表。
- IconPark 官方 SVG 图标已本地打包并按主题着色，运行时不依赖网络；来源与许可保存在 `crates/lumark-ui/assets/icons/iconpark/`。
- ZIP、7Z、LZH、TAR、TAR.GZ/BZ2/XZ/ZST/BR/LZ4/LZ/LZMA；GZ、BZ2、XZ、ZSTD、Brotli、LZ4；ZIP/7Z AES 密码、可选的 ZipCrypto 兼容写入和 7Z 固实压缩；ZIPX/旧 ZIP 的 Deflate64、Shrink、Reduce、Implode 只读解压。LZH 创建底层支持 lh0、lh4–lh7，界面预设使用 lh4/lh5/lh7；lh0 条目已分块流式写入，lh4/lh5/lh7 仍需要整块输入，超过 512 MiB 会提前拒绝。
- ZPAQ 读写（内嵌 libzpaq/JIDAC 引擎）：创建标准归档、AES-256 密码、列表、完整测试、预算解压、分卷和密码库联动；压缩级别 1–5 可选，轻巧模式的“体积小”对应 5 级，默认保留源文件原始时间，≥32 MiB 的单文件批次交给可杀死的隐藏工作进程执行，压缩中可直接取消；解压和完整测试也通过该工作进程执行，任务取消时终止子进程并清理暂存区；普通小文件批次按 32 MiB / 64 文件分批上报进度。创建时会跳过符号链接和 Windows 目录联接，避免归档条目与源计划不一致；输出同名旧压缩包在任务成功后原子替换。PAQ8/LPAQ 系列暂无许可兼容的成熟后端，不在支持范围。
- RAR 只读列表、密码验证、解压和完整性测试（UnRAR 后端，保留许可声明）；Unix `.Z`、AES `.enc`、ALZ（Store/Deflate/ALZ 改造 Bzip2，支持 ZipCrypto 和 `.a00` 分卷）只读解压。
- ISO/UDF、CAB、ARJ、ACE、ARC/PAK、CPIO、WIM（仅第一映像，XPRESS/LZX）、ASAR、DEB、ISZ、IMG/BIN、UU/XXE、LZ/LZMA、TAR.Z、ZOO、HA、HYP、SQ、SQZ、UC2、ICE、PACKICE 等 40+ 只读格式适配；WIM 的 LZMS 压缩和 SWM 分卷暂不支持。
- ZIP/7Z 归档内浏览、选择性解压、删除/重命名/添加文件，7Z 重命名和整块删除直接复用原始压缩块，无法复用时才全量重建，最后原子替换；把文件或文件夹拖入归档时可选择“替换同名文件”“只更新较新的文件”“仅刷新已有条目”或“同步文件夹内容”，同步会删除归档中磁盘已不存在的条目，结果消息会给出跳过和删除数量；可选 `.lumark.bak`。
- `.001` 原始分卷创建、列表和解压；失败时保留旧卷并清理半成品。
- 密码管理：添加/编辑/删除即时写入、名称/备注/匹配规则、记录时间、独立编辑窗口、掩码与 10 秒显示、Windows DPAPI（计算机绑定）、可选主密码、Windows 会话锁定时自动锁定、加密导入导出和明文 CSV 导出；导出位置通过文件夹选择器指定，锁定时需先解锁，操作结果通过官方 Notification 提示；独立设置窗口可查看密码库位置并迁移到自定义文件夹。
- 打开加密压缩包时显示独立密码输入窗口，支持显示密码、自动匹配密码库记录和跳转密码管理；自动匹配失败后可直接勾选“保存到密码库”，密码校验通过才写入记录。
- 文件平铺视图按大图标加名称排列，优先使用 Windows Shell 文件类型图标，读取失败时回退 IconPark；文件表列宽可拖动表头分隔线逐列调整，松手后持久化，设置页保留三档预设和“重置列宽”。
- 支持把归档内文件直接拖到资源管理器解压，或用 Ctrl+C 复制文件后粘贴；临时文件按归档窗口隔离，关闭窗口后清理。
- 解压加密 ZIP/7Z/RAR 时从密码库生成候选密码并在后台逐个验证；主密码锁定时提供解锁入口，校验较慢时可从进度卡片或状态栏停止并改为手动输入。
- 后台任务：固定工作线程池按提交顺序排队执行归档加载、压缩、解压、测试、编辑和记录密码验证，排队中的任务可直接取消；支持进度、暂停/继续、取消、状态栏和独立进度窗口，任务中心同时显示运行中/排队中/已完成数量和跨任务合并进度。
- 压缩后可测试归档并在成功时删除源文件。
- 压缩完成邮件通知：设置窗口“通知”页配置 SMTP 主机、端口、账号、DPAPI 加密密码、发件人和收件人，支持隐式 TLS/STARTTLS/无 TLS 及测试发送；压缩任务成功后自动发送纯文本通知。
- 自解压 EXE：压缩窗口可勾选“自解压 EXE”（ZIP 或 7Z，可带密码，不支持分卷），生成的可执行文件内置 `lumark-sfx` 运行时；双击后可在窗口中选择解压目录、输入密码，密码错误可重试，也支持 `/S`、`/D=`、`/P=`、`/Y` 静默参数；成功后打开资源管理器，模块随安装脚本分发。
- 伪装压缩（全格式）：压缩窗口可勾选“伪装为媒体文件”，把 ZIP、7Z、LZH、ZPAQ、TAR 系列、GZ/BZ2/XZ/ZST/Brotli/LZ4 的输出包装成可正常播放/查看的 MP4/JPG 外观；TAR 系列和单文件流边生成边写入伪装容器，不再额外保留完整内层文件；内层归档格式保持不变，载荷按 128 KiB 分块用 BLAKE3 密钥流混淆，尾部 512 字节记录载荷偏移/长度、原文件名、混淆盐和载荷类型。内置视频/图片面具，也可选择自定义媒体文件；伪装不提供加密。
- 一键打开与一键还原：Lumark 文件表双击伪装归档、或资源管理器“用 Lumark 打开”，直接浏览/选择性解压/测试内层归档；ZIP/7Z/TAR 系列/单文件流/LZH 使用读取视图不复制载荷，ZPAQ/RAR/ALZ/传统只读格式物化到临时目录并做进程内缓存。右键“还原伪装文件”或资源管理器“Lumark 一键还原”把真身写回伪装文件旁，同名文件自动追加序号；伪装 ZIP/7Z 还支持原地重命名、删除和添加，自动复用原面具重新包装。
- 任意文件伪装：文件或归档右键“伪装为媒体文件…”打开独立伪装窗口，资源管理器右键菜单也提供“伪装为媒体文件…”入口；可选择输出目录、面具和“伪装后删除源文件”；已有 ZIP/7Z 等归档伪装后仍可直接浏览，普通文件伪装只能还原。
- 设置的“压缩”页可配置新压缩默认启用伪装、默认面具类型和自定义面具文件；`.mp4`/`.jpg` 只注册 `OpenWithProgids` 作为“打开方式”候选，不修改系统默认关联。
- 解压资源预算：默认单次解压总量 128 GiB、条目 100 万，超出时中止并删除半成品；ZIP/7Z/TAR 在写文件前检查，单文件流、加密容器和容器复制按剩余预算分块中止；RAR、ALZ 和传统只读格式按条目声明大小做预算检查，ALZ 的写入器还会按剩余预算兜底。
- 压缩后关机：压缩窗口可勾选“完成后关闭计算机”，压缩、测试和删除源文件都成功后由系统 `shutdown.exe` 安排 60 秒倒计时关机，期间可用 `shutdown /a` 取消；失败或取消的任务不会关机。
- CPU 占用：压缩窗口可勾选“降低压缩优先级”，任务运行期间进程以普通以下优先级调度，全部低优先级任务结束后恢复；压缩线程数仍可在算法页单独限制。
- Windows 当前用户文件关联：可勾选 ZIP、7Z、RAR 等 24 种压缩格式，注册后直接打开 Lumark 的 Windows 默认应用页，由系统确认默认值；同时提供经典右键菜单、Send To、回收站删除和安装/卸载脚本。
- ISO 虚拟光驱：Lumark 文件列表和资源管理器右键菜单都提供“加载至虚拟光驱/卸载虚拟光驱”；挂载成功后自动打开盘符，使用 Windows 8+ 自带的 `Mount-DiskImage`/`Dismount-DiskImage`。客户端普通用户通常无需管理员，Server 或收紧的安全策略可能要求管理员。资源管理器菜单位于 `HKCU\Software\Classes\SystemFileAssociations\.iso\shell`，升级后需要重新运行安装脚本或 `Lumark.exe --register-shell`。
- 单实例多窗口：双击或右键打开压缩包时每个文件生成独立窗口，窗口之间共享设置和密码库、各自保留浏览状态，关闭全部窗口后退出进程。
- 内置只读文本查看器：右键菜单可在独立窗口中查看压缩包内外的文本文件，归档条目自动解压到临时目录后显示，超过 4 MB 或二进制内容会给出提示。
- 密码库 XChaCha20-Poly1305、Argon2id、原子写入、损坏 `.bak` 恢复、外部修改锁定和密码零化。

### 格式支持矩阵

- **读写**：ZIP（AES/传统加密、Deflate/Bzip2/LZMA 写入）、7Z、LZH（lh0/lh4–lh7）、ZPAQ、TAR、TAR.GZ、TAR.BZ2、TAR.XZ、TAR.ZST、TAR.BR、TAR.LZ4、TAR.LZ、TAR.LZMA、GZ、BZ2、XZ、ZST、Brotli、LZ4。
- **只读解压**：RAR、ISO/UDF、CAB、ARJ、ACE、ARC/PAK、CPIO、WIM（仅第一映像和 XPRESS/LZX）、ASAR、DEB、ISZ、IMG/BIN、UU/XXE、LZ/LZMA、ALZ、TAR.Z、ZOO、HA、HYP、SQ/SQ2/QQQ、SQZ、UC2、ICE、PACKICE；ZIPX/旧 ZIP 的 Deflate64、Shrink、Reduce、Implode。
- **伪装**：所有可写格式都能输出为媒体外观并直接浏览/解压/测试/还原；只读归档可通过“伪装为媒体文件…”包装，打开时物化到临时目录后复用现有读取逻辑；原始文件载荷只能还原，不能浏览。
- **暂不支持**：RPM、XAR/PKG、MSI、CHM、SquashFS、DMG、EGG、NSIS 安装包、PAQ8/LPAQ；WIM 创建、WIM 的 LZMS/SWM、RAR/CAB/ISO/UDF/ALZ 创建；ZIP Deflate64 等旧方法和 `.tar.Z` 的写入。

## 运行

```powershell
cargo run -p lumark-app
```

要求 Rust stable（当前环境为 1.99，工作区 MSRV 为 1.96），Windows x64，MSVC 工具链。首次构建会下载 GPUI Kit 及其依赖。

生成安装包或自解压功能需要同时构建运行时模块：

```powershell
cargo build --release -p lumark-app -p lumark-sfx
./installer/install.ps1
```

## 测试

```powershell
cargo test --workspace
```

提交前额外运行：

```powershell
powershell -NoProfile -File tools/check_file_sizes.ps1
```

DPAPI 测试需要普通用户密钥存储；托盘图标测试需要可创建 Shell 通知图标的交互式桌面会话，受限会话可能返回“拒绝访问”。

10 万项目录基准：`cargo run -p lumark-browser --release --example bench_100k`，当前实测加载约 167 ms、搜索约 75 ms。

测试覆盖：

- ZIP/7Z/TAR 系列/流式格式往返、AES 密码、固实压缩、原子编辑、分卷和路径穿越拒绝。
- 密码库 XChaCha20-Poly1305、Argon2id、DPAPI、导出导入、主密码、自动锁定、损坏恢复和密码匹配。
- 任务队列压缩/解压/测试和后台记录密码验证。
- 工作池排队顺序、排队任务取消和跨任务进度聚合。
- 归档更新模式（只更新较新文件、仅刷新已有条目、同步文件夹并删除多余条目）和 ZIP 条目时间戳的本地时区解释。
- 自解压包拼接与运行时解析：生成端校验页脚，运行时模块读取数据段并拒绝路径穿越条目。
- 解压资源预算拦截 ZIP、7Z 和流式压缩炸弹，超限时清理半成品。
- 压缩后关机参数（倒计时下限、原因文本）和任务成功/失败/取消时的关机计划清理。
- 进程优先级开关的读写往返和多任务并存时的恢复时机。
- 密码库候选密码自动解压加密 ZIP/7Z/RAR。
- 全格式伪装矩阵（ZIP/7Z/LZH/ZPAQ/TAR 系列/单文件流）的创建、浏览、解压、测试与还原；任意文件伪装、已有归档伪装、只读格式物化打开、尾部损坏、取消和同名还原。
- 多选、范围选择、历史导航和高级搜索过滤。
- `gpui-kit::test` 真实窗口测试覆盖虚拟文件行、平铺视图切换、密码校验弹窗与归档更新模式确认框。
- 真实 Windows 窗口已完成宽窗口、最小窗口、轻巧/经典压缩窗口、独立解压窗口、任务中心、独立进度窗口、密码库、独立密码编辑窗口、主密码窗口、独立设置窗口与文件关联的视觉验收，截图位于 `artifacts/`。

## 界面结构

`app.rs` 只保留应用状态、启动参数和生命周期，其余职责按功能拆分：

| 文件 | 职责 |
|---|---|
| `app/actions/files.rs` | 导航、选择、搜索、目录树和文件操作；进度辅助在 `app/actions/files/util.rs` |
| `app/actions/archive.rs` | 归档打开、压缩、解压、测试、输出目录选择和密码验证流程；按 `open/compress/extract/password/test/util` 拆分 |
| `app/actions/disguise.rs` | 伪装窗口、批量伪装、一键还原和 Shell 错误提示 |
| `app/actions/passwords.rs` | 密码即时写入、解锁、主密码和导入导出 |
| `app/actions/settings.rs` | 主题、默认压缩参数、密码库位置、文件关联和 Shell 集成 |
| `app/actions/tasks.rs` | 任务记录、事件桥、Shell 压缩批次、后台宿主托盘和任务中心数据维护 |
| `app/view/chrome.rs` | 标题栏、动作带、导航行、文件夹侧栏、状态栏和模态层；按布局区域拆分为子模块 |
| `app/view/browser.rs` | 全宽文件表、搜索结果条和侧栏详细信息检查器；列、行、平铺、菜单、详情和拖放分文件 |
| `app/view/pages.rs` | 页面模块入口，只保留子模块声明 |
| `app/view/pages/passwords.rs` | 密码库页面组合 |
| `app/view/pages/password_list.rs` | 记录列表、搜索、锁定面板和记录行 |
| `app/view/pages/password_details.rs` | 记录详情和主密码卡片 |
| `app/view/pages/password_security.rs` | 自动解压与安全操作卡片 |
| `app/view/pages/tasks.rs` | 任务中心页面、任务行和状态样式 |
| `app/view/pages/shared.rs` | 跨页面复用的统计卡片 |
| `app/view/settings_window.rs` | 独立设置窗口、选项卡框架和窗口生命周期 |
| `app/view/settings_window_pages.rs` | 综合、外观和关于选项卡 |
| `app/view/settings_window_tabs.rs` | 压缩、文件关联和密码库选项卡 |
| `app/view/password_windows.rs` | 独立密码编辑窗口和主密码设置窗口 |
| `app/view/compress_window.rs` | 独立压缩窗口、轻巧/经典排版、官方 Select 算法与时间选项、伪装选项卡；按表单、布局、分卷、标签和选项卡拆分 |
| `app/view/disguise_window.rs` | 独立伪装窗口、输出目录、面具选择和删除源文件选项 |
| `app/view/extract_window.rs` | 独立解压窗口、目标目录、密码显示/隐藏和仅解压选中条目 |
| `app/view/dialogs.rs` | 确认对话框、密码提示和导入导出表单；按归档密码、归档编辑、文件编辑、密码库和临时解压拆分 |
| `app/view/windows.rs` | 独立任务进度窗口、任务控制和进度指标 |
| `app/view/components.rs` | IconPark 图标、渐变横幅、经典分组框、单复选控件和通用按钮 |
| `app/util.rs` | 路径、大小、日期、格式和界面格式化辅助函数 |
| `app/theme.rs` | 主题模式、强调色派生和全局主题 token |
| `app/startup.rs` | 启动模式和单实例路径解析 |
| `app/stores.rs` | 多窗口共享的设置和密码库存储 |
| `app/tests.rs` | 启动参数测试入口；真实窗口 UI 测试按浏览器、压缩、密码、任务和查看器拆分到 `app/tests/` |

![Lumark 主界面](artifacts/ui-main-final.png)

![文件夹大小按需计算](artifacts/ui-folder-size.png)

![窄窗口布局](artifacts/ui-narrow.png)

![高级搜索匹配菜单](artifacts/ui-search-menu.png)

![轻巧压缩窗口](artifacts/ui-compress-light.png)

![经典压缩窗口](artifacts/ui-compress-dialog.png)

![官方 Select 下拉菜单](artifacts/ui-compress-select.png)

![文件右键菜单](artifacts/ui-context-menu.png)

![删除确认](artifacts/ui-delete-confirm.png)

![删除密码记录](artifacts/ui-delete-password.png)

![独立解压窗口](artifacts/ui-extract-window.png)

![任务中心](artifacts/ui-task-center.png)

![任务进度窗口](artifacts/ui-progress-window.png)

![取消任务二次确认](artifacts/ui-progress-cancel.png)

![密码管理](artifacts/ui-passwords-final.png)

![独立密码编辑窗口](artifacts/ui-password-editor.png)

![主密码设置窗口](artifacts/ui-master-window.png)

![密码库位置设置](artifacts/ui-vault-location.png)

![更改密码库位置对话框](artifacts/ui-vault-location-modal.png)

![独立设置窗口](artifacts/ui-settings-window.png)

![设置与主题色选择](artifacts/ui-theme-color.png)

![文件关联与 Windows 集成](artifacts/ui-file-association.png)

## 工作区

| Crate | 职责 |
|---|---|
| `lumark-archive` | 格式模型与检测、归档读写、路径安全、选择性解压 |
| `lumark-vault` | 加密密码库、DPAPI、主密码、密码匹配 |
| `lumark-tasks` | 后台任务、进度、暂停和取消 |
| `lumark-mail` | SMTP 邮件通知发送器 |
| `lumark-browser` | 文件系统与归档浏览、多选、搜索 |
| `lumark-settings` | 非敏感设置持久化 |
| `lumark-ui` | 基于 GPUI Kit 的桌面界面 |
| `lumark-sfx` | 自解压运行时模块：解析页脚、按需解压并拒绝不安全路径 |
| `lumark-app` | 应用入口 |
| `lumark-platform-windows` | 回收站、经典 Shell 菜单、单实例和 Windows 平台适配 |

## 设计取舍

- 模块按职责拆分并保持单文件不超过 1000 行：`service.rs` 拆为探测、列表、解压、创建、伪装、传统格式、工具、进度和分卷模块；压缩窗口、文件表、标题栏、弹窗、归档动作、应用测试和 roundtrip 测试同样按功能拆分。`tools/check_file_sizes.ps1` 在验证阶段拦截超限文件，避免功能继续堆回单个文件。

- 密码库默认由与当前计算机绑定的 Windows DPAPI 保护；设置主密码后改用 Argon2id + XChaCha20-Poly1305，并移除 DPAPI 包裹。主密码无法找回。
- 密码列表默认掩码，单条显示 10 秒；复制后 30 秒只在剪贴板内容仍匹配时清空。
- 当前原生读写格式为 ZIP、7Z、LZH、ZPAQ、TAR 系列（含 BR/LZ4/LZ/LZMA）、常用单文件流、Brotli/LZ4；其余 40+ 扩展名已纳入格式识别和界面类型判断，具体只读解码仍按 `IMPLEMENTATION_STATUS.md` 推进。
- ALZ 使用 MIT 的 `unalz` 库做只读解压，支持 Store、Deflate、ALZ 改造 Bzip2、ZipCrypto 和 `.a00` 分卷；ALZ 路径接口是 UTF-8 字符串，非 UTF-8 路径会明确报错，`.egg` 属于另一种格式，不在支持范围。
- ZOO/HA/HYP/SQ/SQZ/UC2/ICE/PACKICE/TAR.Z 通过现有 `unarc-rs` 统一读取接口接入；这些格式按整条目缓冲读取，因此没有条目内逐块进度和中途取消，预算以“读取前声明大小 + 读取后实际长度”双重检查为准，不把它们描述为对大归档零风险。
- ZIP 旧方法（Deflate64/Shrink/Reduce/Implode）只读：启用 `zip` 的 `deflate64` 与 `legacy-zip` 特性，不提供这些方法的写入入口。
- ZPAQ 使用 `zpaq_rs` 内嵌的 libzpaq/JIDAC 参考实现，而不是只读适配层或整包缓冲的写入器：压缩写入标准 JIDAC 格式，条目大小和时间可被官方 zpaq 读取；解压先落到输出目录内的暂存区，再按覆盖策略移动，确保跳过/重命名和资源预算可用。引擎调用是阻塞的，因此压缩按 32 MiB / 64 文件分批提供进度和取消点，解压解码阶段没有逐文件进度；PAQ8/LPAQ 没有许可兼容的成熟 crate（paq8px 为 GPL-2 且归档跨版本不兼容），暂不支持。
- 自解压包使用自有运行时模块，只解压无密码、不分卷的 ZIP：格式固定使模块保持很小，也不需要在解压端处理密码对话框；解压路径的包含性、符号链接和覆盖行为复用 `zip` 自带的解压器，不自研第二套校验。之所以没有引入 `zesven::sfx` 或 7-Zip SFX 模块：前者只负责 stub+配置+7z 拼接且仍需要外部 stub，后者是 LGPL 二进制并需额外合规；7Z 自解压和带密码的自解压仍未实现。
- 伪装容器使用自有格式，不与 Apate 做二进制互通：Apate 为 GPL-3.0，读取或生成其格式需要研究并受其许可约束；Lumark 只借鉴“伪装/一键还原”的功能形态，容器、实现和内置面具均为独立实现。容器固定为“面具 + 混淆载荷 + 512 字节尾部”，尾部保存载荷边界、原文件名、混淆盐和载荷类型，因此还原不依赖面具内容。
- 伪装创建先写内层归档到同目录临时文件，再流式包装成媒体外观；峰值磁盘约等于压缩包的两倍，换取所有可写格式共用同一套包装逻辑。后续可对 ZIP/7Z/TAR/单文件流的泛型写入器做单遍写入优化。
- 伪装归档的打开策略分两类：ZIP/7Z/TAR 系列/单文件流/LZH 直接读取解密视图，不复制载荷；ZPAQ、RAR、ALZ 和 `geezipx-core`/`unarc-rs` 传统格式只接受路径，物化到 `%TEMP%\Lumark\disguise-*` 并按（路径、长度、修改时间、盐）缓存，最多 4 项 / 16 GiB，启动时清理 24 小时前的残留。
- 伪装只混淆载荷和文件名，不提供加密：盐随文件保存，能读取文件的人都能还原；真正的机密性仍依赖 ZIP/7Z 密码。尾部信息是还原的唯一入口，文件被截断或追加后无法还原，原文件名上限为 240 字节。
- CPU 限制采用进程优先级而不是百分比硬限速：硬限速会把界面线程一起限流，暂停、取消和进度刷新都会变迟钝；需要更精细控制时使用压缩线程数选项。
- 7Z 后端继续使用 `sevenz-rust2`：`zesven` 的解压进度、流式和编辑 API 更全，但更年轻（2026 年首版、下载量约为前者 1/27、七个月跨了三个大版本），整体迁移要重写 list/extract/create/edit/probe/verify 并重跑 7-Zip 互操作矩阵；现阶段用读取器包装和编码块完成量补进度（加密归档退回读取进度），等需要“7Z 编辑避免全量重压缩”等能力时再做 spike 评估。
- LZH 创建复用 `geezipx-core` 的 `LzhWriter`，按预设映射到 lh4/lh5/lh7；该写入器需要把单个文件整块读入内存，因此项目对单条目限制 512 MiB，并忽略“仅存储通配符”、注释和归档时间选项。
- ISO 挂载不引入第三方驱动，直接调用系统 `Mount-DiskImage`/`Dismount-DiskImage`；挂载状态在 Lumark 退出后仍由 Windows 保留，卸载入口在 Lumark 文件列表和资源管理器右键菜单。资源管理器入口注册在 `SystemFileAssociations\.iso\shell`，不依赖 `.iso` 当前绑定的 ProgID，因此第三方软件占用文件关联也不影响。
- Windows 11 稀疏 MSIX/新版菜单、会话锁定和 UI 自动化测试仍在后续阶段。
- 视频、图片、PDF 压缩按规划暂不实现。

## 许可与来源

本项目使用 MIT OR Apache-2.0。压缩核心 API 和格式思路参考 MIT 许可的 [GeeZipX](https://github.com/GEELINX-LTD/geezipx)；伪装压缩和“一键还原”的功能形态参考 GPL-3.0 的 [rippod/apate](https://github.com/rippod/apate)，未复用其代码或文件格式。GPUI Kit 使用 Apache-2.0；ZIP 使用 MIT；sevenz-rust2 使用 Apache-2.0；ZPAQ 后端使用 CC0-1.0 的 [zpaq_rs](https://github.com/turtle261/zpaq-rs)，其内置的 libzpaq/zpaq.cpp 为公有领域软件；ALZ 后端使用 MIT 的 [unalz](https://github.com/alkegi/unalz)；LZIP/LZMA 编码使用 Apache-2.0 的 [lzma-rust2](https://github.com/hasenbanck/lzma-rust2)；传统格式适配使用 MIT OR Apache-2.0 的 [unarc-rs](https://github.com/mkrueger/unarc-rs)；界面图标来自 Apache-2.0 许可的 [IconPark](https://iconpark.oceanengine.com/official)。发布前请阅读 `THIRD_PARTY_NOTICES.md`。
