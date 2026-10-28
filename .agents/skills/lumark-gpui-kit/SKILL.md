---
name: lumark-gpui-kit
description: gpui-kit 0.8.1 的功能与组件清单，以及「文档里有的必须优先用」的实施规则；在 Lumark 中新增或修改任何界面、窗口、表单、列表、表格、浮层、通知、图标、主题、i18n 或 UI 测试时使用。
---

# 优先使用 gpui-kit 的能力

`crates/lumark-ui` 的界面层固定使用 `gpui-kit = "=0.8.1"`（工作区 `Cargo.toml`，`Cargo.lock` 锁在同一版本）。
官方文档已为下面的每一项提供实现、测试过的用法和默认视觉：**文档中存在的功能和组件，必须优先使用现成实现，不得手写替代品。**

## 文档入口

| 用途 | 地址 |
| --- | --- |
| 功能文档 | <https://gpui-kit.com/zh-CN/docs/> |
| 组件文档 | <https://gpui-kit.com/zh-CN/component/> |
| Base 层（无样式行为原语） | <https://gpui-kit.com/zh-CN/base/primitives/> |
| 全部页面纯文本（可离线检索） | <https://gpui-kit.com/llms-full.txt> |
| 页面地址规律 | 功能 `/zh-CN/docs/<slug>`，组件 `/zh-CN/component/<slug>`，原语 `/zh-CN/base/primitives/<slug>`，Base 专题 `/zh-CN/base/<slug>` |
| 离线源码（最权威） | `%USERPROFILE%\.cargo\registry\src\*\gpui-component-0.8.1\src\`、`gpui-base-0.8.1\src\` |

站点默认版本就是 v0.8.1（页面上标为「最新」），与本仓库依赖一致。历史版本在 `/versions/vX.Y.Z/` 下，**不要用旧版本页面推断当前 API**。

## 强制规则

1. **先查清单，再动手。** 写任何界面代码前，先用下面的功能表和组件表确认是否已有实现；有就打开对应页面，照抄它的 `## 导入` 路径和标记为 `Tested consumer recipe` 的完整示例。
2. **禁止手写重复实现。** 下面这些「看起来简单于是自己拼」的部件必须用组件库版本：标签/徽标（`Tag`、`Badge`）、空状态（`Empty`）、加载态（`Spinner`、`Skeleton`、`ShimmerText`）、快捷键（`Kbd`）、对话框与确认（`WindowExt::open_dialog` / `open_alert_dialog`）、通知（`WindowExt::push_notification`、`Notification`）、提示（`Tooltip`）、浮层（`Popover`、`HoverCard`）、表单行与校验（`Form`、`Field`、`Input`）、设置页（`Settings`）、分组卡片（`GroupBox`）、键值展示（`DescriptionList`）、分页（`Pagination`）、步骤（`Stepper`）、进度（`Progress`）、开关/滑杆/单选（`Switch`、`Slider`、`Radio`）、选中态按钮（`Toggle`）、侧栏/标题栏/状态栏/工具栏（`Sidebar`、`TitleBar`、`StatusBar`、`Toolbar`）、树与列表（`Tree`、`List`、`VirtualList`）、表格（`Table`、`DataTable`）、复制按钮（`Clipboard`）、头像（`Avatar`）、折叠区（`Accordion`、`Collapsible`）、可拖分栏（`Resizable`）、可滚动容器（`Scrollable`）、图表（`Chart`、`plot`）、Markdown/HTML（`TextView`）、diff（`Diff`）、代码编辑（`Editor`）。
3. **不许发明 API。** 「方法名听起来合理」不是证据：每个方法都要能在文档页或上面 `cargo registry` 源码里找到。查不到就说明还没这个 API，改用现有组合或先问。
4. **只有文档没覆盖的能力才自建**，并在 `///` 注释里写清现有组件为什么不适用。本仓库的 IconPark 图标集（`AppIcon` + `Icon::data`）就是既定例外，不要再引入 gpui-kit 的内置图标集当业务图标。
5. **缺 feature 就开 feature，不要绕开组件自己写。** `speech`、`tree-sitter`、`syntect`、`decimal` 等能力在 `gpui-kit` 默认 feature 之外；需要时在 `crates/lumark-ui/Cargo.toml` 打开对应 feature，而不是重写一套。
6. **弹窗、面板、通知统一走 `WindowExt`。** 它们在窗口 `Root` 之上合成，自带焦点圈定、遮罩、键盘关闭和进出动效；自建覆盖层会丢掉这些行为。
7. **所有交互必须能键盘操作且有语义。** 见功能表中的 `无障碍` 与 `焦点` 两行：交互元素给 `ElementId`、`FocusHandle` 和 `accessibility_label`，`<div>` 加 `on_click` 不算按钮。
8. **视觉沿用主题与 `lumark-code-style`。** 颜色取 `cx.theme()`，圆角用 `theme.radius` / `theme.radius_lg`，禁止把矩形做成胶囊。
9. **改完就跑验证。** `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、相关 UI 测试（`#[gpui_kit::test]`）。

## 使用流程

1. 在下面的表里定位能力：功能表回答"这个能力有没有"，组件表回答"这个控件有没有"。
2. 打开对应文档页，依次读 `## 导入`、`## 用法`、带 `Tested consumer recipe` 标记的完整示例，以及页尾的注意事项。
3. 照抄导入路径。文档要求 `as _` 的 trait（`Sizable as _`、`ButtonVariants as _`、`ActiveTheme as _`、`StyledExt as _`、`Disableable as _`、`Colorize as _`）必须一起导入，否则链式方法不可见。
4. 需要跨帧状态时用文档给出的 `*State`：`cx.new(|cx| XxxState::new(window, cx))` 先建好再传进视图或弹层。**不要在 `render` 或 `open_dialog` 的 builder 里 `cx.new(...)`**，重建会丢编辑态和选中态。
5. 复制粘贴前先确认类型名和模块路径与下表一致（表里的模块路径来自各页 `## 导入`，可直接使用）。

## 功能清单

### 应用骨架与平台

| 能力 | 关键入口 | slug |
| --- | --- | --- |
| 依赖安装与平台工具链 | `gpui-kit` 单依赖；`gpui-pre` 是 GPUI 快照名 | `installation` |
| 应用启动与窗口 | `application().with_assets(..).run` → `init(cx)` → `open_window(options, cx, build)` | `getting-started` |
| 窗口 Root | `open_window` 自动挂载 `Root`，承载对话框/面板/通知层 | `component/root` |
| 多窗口 | 每窗口独立 `Window`/`Root`，共享 `Entity` 数据 | `multi-window` |
| 窗口 API | `Window` 上下文、`WindowOptions`、`.window_handle()` | `window` |
| 主题与 token | `ActiveTheme::theme()`；`theme.radius`、`radius_lg`、颜色 token | `component/theme` |
| 字体 | 应用需自行提供的字体清单与回退 | `fonts` |
| 国际化 | `rust-i18n 4.2`；组件文案在 `gpui_component` namespace 覆盖 | `i18n` |
| 系统通知中心 | `Notification::info(..).system()`、`.in_app_and_system()`、`cx.show_system_notification(..)` | `system-notification` |
| 性能 HUD | `gpui_fps::fps_monitor(window, cx)`（独立 crate，需自行添加依赖） | `fps` |
| 原生扩展 | `NativeMenu`（系统菜单）、`gpui-webview`（实验性） | `native-extension` |
| WebView | `gpui-webview` 仅实验性；渲染文档用 `TextView`，开浏览器用 `cx.open_url` | `webview` |
| 打包与分发 | 各平台产物、签名与安装格式 | `packaging` |
| 自动更新 | 框架不提供更新服务，策略与验证属应用 | `auto-update` |
| WebAssembly | 组件 showcase 可用，不作为交付路径 | `webassembly` |
| 移动端 | iOS/Android 平台接入 | `mobile` |

### 交互与状态模型

| 能力 | 关键入口 | slug |
| --- | --- | --- |
| 跨帧状态 | `Entity<T>`、`cx.new`、`Context` | `entity` |
| 上下文 | `&mut Window` / `&mut Context<Self>` / `&mut App` | `context` |
| 视图渲染 | `Render::render(&mut self, ..)` | `render` |
| 值类型 UI | `RenderOnce::render(self, ..)`，一次性界面描述 | `render-once` |
| 元素标识 | `ElementId`、`GlobalElementId`、a11y 节点身份 | `element_id` |
| 视图缓存 | 复用上一帧记录，跳过无变化子树的 `render` | `view-cache` |
| 焦点 | `FocusHandle`、Tab 顺序、`.track_focus()` | `focus` |
| Action | 类型化命令与派发路径 | `action` |
| 快捷键 | `KeyBinding`、`KeyContext`、`Kbd` 展示 | `keybinding` |
| 事件 | `EventEmitter`、`cx.emit` / `cx.subscribe` | `event` |
| 异步任务 | `Task<T>`；handle 被 drop 即取消 | `task` |
| 动效 | 三层 motion：typed animation、`animation` 采样、`reduced-motion` | `animation` |
| 全局单例 | `Global`、`cx.global::<T>()` | `global` |
| 共享文本 | `SharedString`（持久 UI 文本的默认选择） | `shared-string` |
| 无障碍 | AccessKit role/name/state/action、`accessibility_label` | `accessibility` |
| UI 集成测试 | `#[gpui_kit::test]`、`TestAppContext`、`gpui_kit::test::TestWindowExt` | `test` |

### 渲染、资源与指南

| 能力 | 关键入口 | slug |
| --- | --- | --- |
| Element 树 | `<div>`、`Element`、帧级 callback 生命周期 | `element` |
| 绘制 | `window.paint_quad`、`paint_path`、`canvas(prepaint, paint)` | `paint` |
| 几何与颜色 | `Bounds`、`Pixels`、`point/px/size`、`hsla` | `geometry` |
| 样式 | `Styled` 链式方法，对应 Tailwind 词汇 | `style` |
| 图片 | `img()`、`svg()`、`Icon`；`with_loading` / `with_fallback` | `image` |
| 图标与资源 | `IconName`、`Icon`、`assets::Assets`；本仓库默认 `AssetSource` | `assets` |
| 文本系统 | 字体塑形与 `TextSystem`；优先用现成文本元素 | `text-system` |
| 编码指南 | 所有权、模块划分、命名、测试与性能规则 | `coding-guides` |
| 设计指南 | 层级、间距、圆角、密度、界面用词、AI 生成界面规则 | `design-guides` |
| 框架对比 | 选型与生态能力对照（参考，不是 API） | `comparison` |

`gpui-shell`（JS 扩展层）是独立 crate，本项目未使用，不要引入。

### Base 层专题（`gpui_kit::base`）

| 能力 | 关键入口 | slug |
| --- | --- | --- |
| 原语总览 | 只提供行为、不规定视觉的组合件 | `primitives` |
| 动效 | `base::motion`，确定性采样、中断、`reduced-motion` | `motion` |
| 虚拟列表 | `v_virtual_list` / `h_virtual_list`，可变行高 | `virtual-list` |
| 富文本 | `base::text::TextView`，Markdown 与常用 HTML | `text-view` |
| 视图栈 | `NavStack`，对应 SwiftUI `NavigationStack` | `primitives/nav-stack` |
| 自绘滚动条 | `Scrollbar`、`ScrollbarState` | `primitives/scrollbar` |
| Toast | 受管理的临时消息栈 | `primitives/toast` |
| 底层浮层 | `Popup` 触发器与锚定内容宿主 | `primitives/popup` |
| 链接 | `Link`，样式由应用定义 | `primitives/link` |
| Dock 基础 | 可持久化停靠布局模型 | `dock` |
| 撤销/历史 | `History<T>`、`UndoHistory<T>` | `history` |
| 跨块文本选择 | 一次拖拽跨越多个文本块 | `text-selection` |
| 图表原语 | 比例尺、图形、坐标轴、hover | `plot` |

## 组件清单

模块路径省略 `gpui_kit::component::` 前缀；「主要类型」来自各页 `## 导入`。

### 按钮与选择控件

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Button | `button::{Button, ButtonGroup, ButtonVariants}` | 多变体/尺寸/图标/加载态按钮 | `button` |
| DropdownButton | `button::{Button, DropdownButton}` | 左主操作 + 右下拉菜单 | `dropdown_button` |
| Toggle | `button::{Toggle, ToggleGroup}` | 按钮式二元/多选切换 | `toggle` |
| Checkbox | `checkbox::Checkbox` | 二元与三态勾选 | `checkbox` |
| Switch | `switch::Switch` | 开关 | `switch` |
| Radio | `radio::{Radio, RadioGroup}` | 单选与单选组 | `radio` |
| Slider | `slider::{Slider, SliderState, SliderEvent, SliderValue}` | 单值/区间滑杆 | `slider` |
| Rating | `rating::Rating` | 星级评分 | `rating` |
| Clipboard | `clipboard::Clipboard` | 复制按钮与成功态图标 | `clipboard` |

### 输入与表单

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Input | `input::{Input, InputState}` | 单行文本、掩码、校验 | `input` |
| InputGroup | `input::{InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput, InputGroupText}` | 输入框前后缀与工具栏 | `input-group` |
| Textarea | `input::{Textarea, TextareaState}` | 多行文本、自动增高 | `textarea` |
| Editor | `input::{Editor, EditorState, TabSize}` | 源码编辑、行号、折叠、诊断 | `editor` |
| NumberInput | `input::{NumberInput, NumberInputEvent, StepAction}` | 数值、步进、范围 | `number-input` |
| OtpInput | `input::{OtpInput, OtpState, OtpEvent}` | 一次性验证码 | `otp-input` |
| Select | `select::{Select, SelectState, SelectItem, SelectDelegate, SelectEvent, SelectGroup, SearchableVec}` | 下拉选择 | `select` |
| Combobox | `combobox::{Combobox, ComboboxState, ComboboxEvent}` | 可搜索单选/多选下拉 | `combobox` |
| DatePicker | `date_picker::{DatePicker, DatePickerState, DateRangePreset, DateTime}` | 单日/区间日期选择 | `date-picker` |
| TimeField | `time_field::{TimeField, TimeFieldState, HourCycle, TimePrecision}` | 分段时间输入 | `time-field` |
| Calendar | `calendar::{Calendar, CalendarState, Date, Matcher}` | 日历网格、区间、禁用规则 | `calendar` |
| ColorPicker | `color_picker::{ColorPicker, ColorPickerState, ColorSelect}` | 取色、HEX/RGB/HSL、alpha | `color-picker` |
| Form | `form::{Form, Field, field, v_form, h_form}` | 字段与操作区布局 | `form` |
| Label | `label::{Label, HighlightsMatch}` | 表单标签、次要文本、匹配高亮 | `label` |
| Questionnaire | `questionnaire::{Questionnaire, QuestionnaireState, ...}` | 多步骤问卷与校验 | `questionnaire` |
| Speech | `speech::{SpeechButton, SpeechState, SpeechWaveform, SpeechStatus, SpeechEvent}` | 语音转文本（系统识别器需 `speech` feature） | `speech` |

### 展示与数据

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Avatar | `avatar::{Avatar, AvatarGroup}` | 头像与首字母回退 | `avatar` |
| Badge | `badge::Badge` | 数字/圆点/图标徽标 | `badge` |
| Tag | `tag::Tag` | 分类、状态、优先级标签 | `tag` |
| Kbd | `kbd::Kbd` | 快捷键展示（按平台换符号） | `kbd` |
| Icon | `{Icon, IconName}` | 路径或内存字节渲染 SVG | `icon` |
| Image | `img()`、`svg()` | 图片与单色图标 | `image` |
| DescriptionList | `description_list::{DescriptionList, DescriptionItem, DescriptionText}` | 键值对元数据 | `description-list` |
| TextView | `text::{TextView, markdown}` | Markdown/HTML 只读渲染 | `text-view` |
| Diff | `diff::{Diff, DiffState, DiffFile, DiffMode, ...}` | unified/Git diff 只读展示 | `diff` |
| Table | `table::{Table, TableHeader, TableBody, TableRow, TableHead, TableCell, TableCaption}` | 声明式静态表格 | `table` |
| DataTable | `table::{DataTable, TableState, TableDelegate, Column, ColumnSort, TableEvent}` | 虚拟滚动、排序、列管理 | `data-table` |
| List | `list::{List, ListState, ListDelegate, ListItem, ListEvent}` | 虚拟化、搜索、分组列表 | `list` |
| VirtualList | `{v_virtual_list, h_virtual_list, VirtualListScrollHandle}` | 可变行高虚拟列表 | `virtual-list` |
| Tree | `tree::{tree, TreeState, TreeItem, TreeEntry}` | 层级数据与键盘导航 | `tree` |
| Chart | `chart::{LineChart, BarChart, AreaChart, PieChart, RadarChart, CandlestickChart, SankeyChart}` | 数据可视化 | `chart` |
| Plot | `plot::{scale::*, shape::*, PlotAxis, AxisText}` | 自定义图表原语 | `plot` |
| Progress | `progress::Progress` | 完成度与不确定进度 | `progress` |
| Spinner | `spinner::Spinner` | 旋转加载指示 | `spinner` |
| Skeleton | `skeleton::Skeleton` | 加载占位块 | `skeleton` |
| Shimmer | `shimmer::{ShimmerText, ShimmerStyle}` | 文字 loading 高光 | `shimmer` |
| Empty | `empty::{Empty, EmptyHeader, EmptyTitle, EmptyDescription, EmptyMedia, EmptyContent}` | 空状态、无结果、首次使用 | `empty` |

### 布局与导航

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Root | `Root`（Base 提供） | 主题/对话框/通知的窗口根 | `root` |
| Theme | `ActiveTheme` | 颜色、字体、明暗外观 | `theme` |
| GroupBox | `group_box::{GroupBox, GroupBoxVariant}` | 带标题与边框的分区容器 | `group-box` |
| Resizable | `resizable::{h_resizable, v_resizable, resizable_panel, ResizablePanelGroup, ResizableState, HandleEdge}` | 可拖拽分栏 | `resizable` |
| Scrollable | `scroll::{ScrollableElement, ScrollbarAxis, ScrollbarMode}` | 可滚动容器与自定义滚动条 | `scrollable` |
| Sidebar | `sidebar::{Sidebar, SidebarHeader, SidebarFooter, SidebarGroup, SidebarMenu, SidebarMenuItem, SidebarToggleButton}` | 应用导航侧栏 | `sidebar` |
| Tabs | `tab::{Tab, TabBar}` | 分区切换 | `tabs` |
| Stepper | `stepper::{Stepper, StepperItem}` | 流程步骤 | `stepper` |
| Pagination | `pagination::Pagination` | 分页导航 | `pagination` |
| Toolbar | `toolbar::Toolbar` | 透明水平操作容器 | `toolbar` |
| StatusBar | `status_bar::StatusBar` | 左/中/右状态栏 | `status-bar` |
| TitleBar | `TitleBar` | 自绘标题栏与窗口控制按钮 | `title-bar` |
| Menu | `menu::{PopupMenu, PopupMenuItem, DropdownMenu, ContextMenuExt}` | 弹出菜单、右键菜单 | `menu` |
| Command | `command::{Command, CommandState, CommandGroup, CommandItem, CommandEntry}` | ⌘K 风格命令面板 | `command` |
| Dock | `dock::{DockArea, DockSkin}` | 可拖标签、嵌套分割的工作区 | `dock` |
| Carousel | `carousel::{Carousel, CarouselContent, CarouselItem, CarouselNext, CarouselPrevious, CarouselPagination, CarouselState}` | 横向/纵向浏览一组内容 | `carousel` |
| Accordion | `accordion::Accordion` | 多区块折叠（FAQ、设置分组） | `accordion` |
| Collapsible | `collapsible::Collapsible` | 单个显隐区域 | `collapsible` |
| Settings | `setting::{Settings, SettingPage, SettingGroup, SettingItem, SettingField}` | 设置页结构 | `settings` |

### 浮层与反馈

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Dialog | `dialog::Dialog` + `WindowExt::open_dialog` | 普通对话框、确认框 | `dialog` |
| AlertDialog | `dialog::{AlertDialog, DialogAction, DialogClose}` + `WindowExt::open_alert_dialog` | 中断式确认，固定默认行为 | `alert-dialog` |
| Sheet | `sheet::Sheet` + `WindowExt::open_sheet` / `open_sheet_at` | 从边缘滑出的面板/抽屉 | `sheet` |
| Popover | `popover::Popover` | 锚定浮层内容 | `popover` |
| HoverCard | `hover_card::HoverCard` | 悬停富内容预览（带延迟） | `hover-card` |
| Tooltip | `tooltip::Tooltip` | 悬停/聚焦说明与快捷键 | `tooltip` |
| Notification | `notification::{Notification, NotificationType}` + `WindowExt` | 应用内 toast、系统通知 | `notification` |
| Alert | `alert::Alert` | 页面内提示条（多变体、可关闭） | `alert` |
| FocusTrap | `FocusTrapElement` | 把 Tab 焦点限制在容器内 | `focus-trap` |
| Marker | `marker::{Marker, MarkerContent, MarkerIcon, MarkerVariant}` | 全宽会话分隔行（时间、未读、状态） | `marker` |

### 会话界面

| 组件 | 模块与主要类型 | 用途 | slug |
| --- | --- | --- | --- |
| Attachment | `attachment::{Attachment, AttachmentMedia, AttachmentContent, AttachmentActions, AttachmentTitle, AttachmentStatus}` | 文件与媒体附件条目 | `attachment` |
| Bubble | `bubble::{Bubble, BubbleContent, BubbleGroup, BubbleReactions, BubbleVariant}` | 聊天气泡表面 | `bubble` |
| Message | `message::{Message, MessageHeader, MessageContent, MessageFooter, MessageAvatar, MessageGroup, MessageAlignment}` | 消息行结构与对齐 | `message` |
| MessageScroller | `message_scroller::{MessageScroller, MessageScrollerState}` | 尾部跟随的虚拟消息列表 | `message-scroller` |

### 同仓可用但文档未单列

`breadcrumb`、`separator`、`link`（原语页有文档）、`searchable_list`、`native_menu`（见 `native-extension`）、`inspector`（需 `inspector` feature 或 debug 构建）。使用前先读源码确认签名，必要时按规则 4 说明理由。

## 常用根级导入

```rust
use gpui_kit::component::{
    ActiveTheme as _,              // cx.theme()
    ButtonVariants as _,           // .primary()/.ghost()/.danger()
    Colorize as _,                 // .opacity()/.lighten() 等颜色运算
    Disableable as _,              // .disabled(bool)
    Sizable as _,                  // .small()/.large()/.with_size()
    StyledExt as _,                // .refine_style()/.corner_radii()/.h_flex()
    Selectable,                    // .selected(bool)
    Icon, IconName,
    IndexPath,
    Placement,                     // open_sheet_at / 定位
    TitleBar,
    WindowExt as _,                // open_dialog/open_sheet/push_notification
    h_flex, v_flex,
};
```

- 弹层与通知（全部由 `Root` 合成，不要自建覆盖层）：
  `window.open_dialog(cx, |dialog, _, _| ...)`、`window.open_alert_dialog(cx, |alert, _, _| ...)`、
  `window.open_sheet(cx, |sheet, _, _| ...)`、`window.open_sheet_at(Placement::Right, cx, ..)`、
  `window.close_dialog(cx)`、`window.has_active_dialog(cx)`、
  `window.push_notification(Notification::info(".."), cx)`、`window.clear_notifications(cx)`。
- 图标：业务图标统一走 `crates/lumark-ui/src/app/view/components.rs` 的 `AppIcon`（IconPark SVG + `Icon::data`）；`IconName` 只用于 gpui-kit 组件自带的图标槽位。

启动骨架（`Root` 由 `open_window` 自动挂载，不要自己包一层）：

```rust
gpui_kit::application()
    .with_assets(gpui_kit::assets::Assets)
    .run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| MyView)
        })
        .expect("failed to open window");
    });
```

## 与本仓库既有约定的衔接

- `lumark-code-style` 负责注释、模块边界、圆角与提交规范；本 skill 负责「用哪个组件/哪个功能」。两者冲突时以 `lumark-code-style` 的视觉与模块规则为准，能力实现仍优先用 gpui-kit。
- 主题：`crates/lumark-ui/src/app/theme.rs` 统一改写 gpui-kit 的 `Theme` token，页面里禁止硬编码颜色。
- i18n：`crates/lumark-ui/locales/ui.yml` 用 `rust-i18n`，组件自带的按钮/占位文案可在 `gpui_component` namespace 下覆盖，不要 fork 组件。
- 测试：`crates/lumark-ui/src/app/tests/ui/` 下已有 `#[gpui_kit::test]` 真实窗口测试，新界面照此补同层测试，轻量断言用 `TestAppContext`。

## 完成前检查表

- [ ] 每个新控件都能在组件表里找到来源，或已按规则 4 写明自建理由。
- [ ] import 路径与文档一致，`as _` trait 没有漏。
- [ ] 浮层/通知走 `WindowExt`，没有自建遮罩或绝对定位层。
- [ ] 跨帧状态是 `Entity`，`InputState`/`SelectState` 等 `*State` 不在 `render` 里创建。
- [ ] 交互元素有 `ElementId`、焦点与 `accessibility_label`，键盘可达。
- [ ] 颜色与圆角来自 `cx.theme()`，没有新增硬编码色值或胶囊圆角。
- [ ] `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 与相关测试通过。
