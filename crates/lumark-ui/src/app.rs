//! 应用状态、启动参数和窗口生命周期。
//!
//! 文件与归档动作、界面视图、主题应用和自动化测试分别位于 `actions`、
//! `view`、`theme` 和 `tests` 子模块；这里只保留跨模块共享的状态定义、
//! 构造流程和窗口入口。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 只有进程内的一个窗口消费单实例请求；压缩批次因此不会被多个窗口拆散。
static IPC_POLL_OWNER: AtomicBool = AtomicBool::new(false);

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, TitleBar, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClipboardItem, Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Window, div,
    hsla, px,
};
use uuid::Uuid;

use lumark_archive::{
    ArchiveFormat, ArchiveService, ArchiveTimeSource, CompressionOptions, DisguiseMask,
    DisguiseOptions, EditOptions, EditRoot, ExtractionOptions, FileTimeFilter,
    OverwritePolicy as ArchiveOverwritePolicy, PathMode, SevenZCompression, UpdateMode,
    ZipCompression, ZipEncryption, ZpaqCompression,
};
use lumark_browser::{BrowserEntry, BrowserLocation, BrowserModel, SearchMode};
use lumark_platform_windows as platform;
use lumark_settings::{
    ColumnKind, ColumnWidths, CompressionPreset as SettingsPreset, DisguiseMaskPreference,
    Settings, SettingsStore, ThemePreference,
};
use lumark_tasks::{TaskEvent, TaskKind, TaskManager, TaskProgress, TaskRequest, TaskState};
use lumark_vault::{PasswordEntry, VaultState, VaultStore};

mod startup;
pub(crate) use startup::*;
mod stores;
pub(crate) use stores::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 主窗口当前页面。
pub(crate) enum Screen {
    /// 文件浏览页。
    Browser,
    /// 密码库页。
    Passwords,
    /// 任务中心页。
    Tasks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 打开归档后的初始动作。
pub(crate) enum ArchiveOpenIntent {
    /// 打开后浏览。
    Browse,
    /// 打开后解压。
    Extract,
}

#[derive(Debug, Clone)]
/// 密码验证成功后要执行的动作。
pub(crate) enum PendingPasswordAction {
    /// 密码通过后打开归档。
    OpenArchive {
        archive: PathBuf,
        intent: ArchiveOpenIntent,
    },
    /// 密码通过后打开解压窗口。
    ExtractDialog { archive: PathBuf },
    /// 密码通过后打开条目。
    OpenEntry {
        archive: PathBuf,
        relative: String,
        mode: PendingOpenMode,
    },
    /// 密码通过后测试归档。
    Test { archive: PathBuf },
}

/// 等待密码验证的后台任务。
pub(crate) struct PendingPasswordJob {
    action: PendingPasswordAction,
    entry_ids: Vec<Uuid>,
    extras: Vec<lumark_vault::PasswordCandidate>,
    /// 手动输入的密码在验证成功后先写入密码库，再继续原动作。
    save_on_success: bool,
}

/// 正在后台校验记录密码的进度。
///
/// 弹窗只在验证超过可感知时长后出现，快速命中时不会闪一下；用户选择
/// “后台继续”后仍可从状态栏停止，因此这里只记录是否被手动隐藏。
struct PasswordAttemptProgress {
    task_id: u64,
    archive: PathBuf,
    index: usize,
    total: usize,
    started: std::time::Instant,
    hidden: bool,
}

impl PasswordAttemptProgress {
    /// 校验进行到 400 毫秒后才展示弹窗。
    fn is_visible(&self) -> bool {
        !self.hidden && self.started.elapsed() >= std::time::Duration::from_millis(400)
    }
}

/// 等待额外密码验证的后台任务。
struct PendingExtraPasswordJob {
    action: PendingPasswordAction,
    candidates: Vec<lumark_vault::PasswordCandidate>,
}

/// 压缩任务对应的界面动作选项。
///
/// 这些选项只影响任务结束后的应用行为，不参与归档格式编码，因此不放进
/// `CompressionOptions`。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CompressTaskOptions {
    /// 任务成功后安排系统关机。
    pub shutdown_after: bool,
    /// 任务运行期间降低进程优先级。
    pub low_priority: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 等待密码验证的归档编辑动作。
pub(crate) enum PendingEditAction {
    /// 密码通过后添加文件。
    Add,
    /// 密码通过后删除条目。
    Delete,
    /// 密码通过后重命名条目。
    Rename,
}

/// 等待密码验证的编辑任务。
struct PendingEditJob {
    archive: PathBuf,
    action: PendingEditAction,
}

/// 等待密码验证后要打开的归档条目。
struct PendingOpenEntry {
    task_id: u64,
    temporary: PathBuf,
    relative: String,
    mode: PendingOpenMode,
}

/// 归档条目解压完成后的处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingOpenMode {
    /// 交给系统默认应用打开。
    External,
    /// 在内置只读文本查看器中打开。
    Viewer,
}

/// 任务中心展示的一条任务记录。
///
/// 记录在任务整个生命周期内保留，成功后也不会立即删除，因此用户可以从
/// 任务中心重新打开后台任务的进度窗口或查看最终结果。
#[derive(Clone)]
pub(crate) struct TaskRecord {
    /// 任务 ID。
    pub(crate) id: u64,
    /// 任务类型。
    pub(crate) kind: TaskKind,
    /// 任务展示名称。
    pub(crate) label: String,
    /// 任务状态。
    pub(crate) state: TaskState,
    /// 最近进度。
    pub(crate) progress: TaskProgress,
    /// 完成或失败消息。
    pub(crate) message: Option<String>,
}

/// 拖出或复制归档条目时的临时解压进度弹窗。
#[derive(Clone)]
pub(crate) struct TempExtractDialog {
    /// 弹窗标题。
    pub(crate) title: String,
    /// 说明文本。
    pub(crate) message: String,
    /// 临时解压进度。
    pub(crate) progress: f32,
}

/// 侧栏目录树的扁平行。
pub(crate) struct TreeRow {
    path: PathBuf,
    label: String,
    depth: usize,
    expanded: bool,
}

/// 正在拖动调整的文件表列。
#[derive(Clone, Copy)]
pub(crate) struct ColumnResize {
    /// 正在调整的列。
    pub(crate) column: ColumnKind,
    /// 按下鼠标时的窗口横坐标。
    pub(crate) start_x: f32,
    /// 按下鼠标时的列宽。
    pub(crate) start_width: f32,
}

/// Lumark 主应用状态。
pub struct LumarkApp {
    browser: BrowserModel,
    /// 当前文件表列宽，拖动时实时更新并在松开后持久化。
    column_widths: ColumnWidths,
    /// 正在拖动的列；`None` 表示没有列宽拖动。
    column_resize: Option<ColumnResize>,
    settings: Arc<SettingsStore>,
    vault: Arc<VaultStore>,
    tasks: Arc<TaskManager>,
    screen: Screen,
    status: String,
    show_compress: bool,
    compress_output: gpui_kit::Entity<InputState>,
    compress_password: gpui_kit::Entity<InputState>,
    compress_comment: gpui_kit::Entity<InputState>,
    compress_exclude: gpui_kit::Entity<InputState>,
    compress_store: gpui_kit::Entity<InputState>,
    compress_split: gpui_kit::Entity<InputState>,
    /// 分卷大小输入框上次使用的单位，重新打开压缩窗口时沿用。
    compress_split_unit: SizeUnit,
    compress_dictionary: gpui_kit::Entity<InputState>,
    compress_threads: gpui_kit::Entity<InputState>,
    compress_after: gpui_kit::Entity<InputState>,
    compress_before: gpui_kit::Entity<InputState>,
    compress_format: ArchiveFormat,
    compress_preset: SettingsPreset,
    compress_solid: bool,
    compress_encrypt_header: bool,
    compress_zipcrypto: bool,
    compress_test_after: bool,
    compress_delete_source: bool,
    /// 下一次打开压缩窗口时是否默认创建自解压文件。
    compress_sfx: bool,
    /// 下一次打开压缩窗口时是否默认启用伪装输出。
    compress_disguise: bool,
    /// 下一次打开压缩窗口时使用的面具类型。
    compress_disguise_mask: DisguiseMaskPreference,
    /// 自定义面具文件路径；仅面具类型为 Custom 时使用。
    compress_disguise_custom: Option<PathBuf>,
    /// 等待完成后关机的压缩任务。
    pending_shutdown_task: Option<u64>,
    /// 正在以低优先级运行的压缩任务；最后一个结束时恢复普通优先级。
    low_priority_tasks: HashSet<u64>,
    compress_path_mode: PathMode,
    compress_tab: u8,
    #[allow(dead_code)]
    compress_classic: bool,
    compress_password_visible: bool,
    sidebar_visible: bool,
    extract_output: gpui_kit::Entity<InputState>,
    extract_password: gpui_kit::Entity<InputState>,
    password_label: gpui_kit::Entity<InputState>,
    password_value: gpui_kit::Entity<InputState>,
    password_pattern: gpui_kit::Entity<InputState>,
    master_unlock: gpui_kit::Entity<InputState>,
    master_extract_unlock: gpui_kit::Entity<InputState>,
    master_remove: gpui_kit::Entity<InputState>,
    selected_password: Option<Uuid>,
    password_editor_window: Option<gpui_kit::AnyWindowHandle>,
    master_password_window: Option<gpui_kit::AnyWindowHandle>,
    active_task: Option<u64>,
    /// 上次检查 Windows 会话锁定状态的时间。
    last_session_check: std::time::Instant,
    task_progress: Option<TaskProgress>,
    active_task_kind: Option<TaskKind>,
    active_task_label: Option<String>,
    progress_windows: HashMap<u64, view::windows::ProgressWindowEntry>,
    task_records: Vec<TaskRecord>,
    compress_window: Option<gpui_kit::AnyWindowHandle>,
    extract_window: Option<gpui_kit::AnyWindowHandle>,
    extract_view: Option<gpui_kit::Entity<view::extract_window::ExtractWindowView>>,
    /// 独立伪装窗口句柄和视图。
    disguise_window: Option<gpui_kit::AnyWindowHandle>,
    disguise_view: Option<gpui_kit::Entity<view::disguise_window::DisguiseWindowView>>,
    /// 伪装窗口使用的输出目录输入框，和主应用共享。
    disguise_output: gpui_kit::Entity<InputState>,
    pending_extract_saves: HashMap<u64, (PathBuf, zeroize::Zeroizing<String>)>,
    pending_extract_open_dirs: HashMap<u64, PathBuf>,
    show_save_prompt: bool,
    pending_unlock_archive: Option<PathBuf>,
    show_unlock_for_extract: bool,
    unlock_for_browse: bool,
    show_archive_password_prompt: bool,
    archive_password_input: gpui_kit::Entity<InputState>,
    archive_password_visible: bool,
    archive_password_use_vault: bool,
    archive_password_action: Option<PendingPasswordAction>,
    /// 密码弹窗由自动尝试失败后打开，用于展示“一并保存”的引导。
    archive_password_after_failure: bool,
    /// 手动输入的密码是否在验证成功后写入密码库。
    archive_password_save_to_vault: bool,
    temp_extract: Option<TempExtractDialog>,
    temp_root: PathBuf,
    settings_error: Option<String>,
    vault_error: Option<String>,
    location_input: gpui_kit::Entity<InputState>,
    search_input: gpui_kit::Entity<InputState>,
    search_results: Option<Vec<BrowserEntry>>,
    search_mode: SearchMode,
    details_visible: bool,
    column_layout: u8,
    tile_view: bool,
    browser_focus: FocusHandle,
    dropped_sources: Option<Vec<PathBuf>>,
    show_delete_confirm: bool,
    pending_delete_paths: Vec<PathBuf>,
    show_new_folder: bool,
    new_folder_name: gpui_kit::Entity<InputState>,
    show_rename: bool,
    rename_name: gpui_kit::Entity<InputState>,
    rename_target: Option<PathBuf>,
    /// 拖入归档后等待确认更新模式的文件列表。
    pending_archive_additions: Vec<(String, PathBuf)>,
    /// 拖入归档的目录根，同步模式据此新增、更新和删除条目。
    pending_archive_roots: Vec<EditRoot>,
    show_archive_add_dialog: bool,
    /// 本次添加到归档时同名条目的处理方式。
    archive_add_mode: UpdateMode,
    password_search: gpui_kit::Entity<InputState>,
    password_note: gpui_kit::Entity<InputState>,
    password_page_scroll: gpui_kit::ScrollHandle,
    password_list_scroll: gpui_kit::ScrollHandle,
    revealed_password: Option<Uuid>,
    show_vault_transfer: bool,
    /// 导出文件夹；加密备份和 CSV 都写入该目录。
    vault_export_dir: gpui_kit::Entity<InputState>,
    /// 待导入的加密密码库文件。
    vault_import_path: gpui_kit::Entity<InputState>,
    vault_transfer_passphrase: gpui_kit::Entity<InputState>,
    /// 密码库锁定时在导入/导出弹窗内输入的主密码。
    vault_transfer_unlock: gpui_kit::Entity<InputState>,
    vault_transfer_replace: bool,
    show_confirm_remove_master: bool,
    show_confirm_clear_vault: bool,
    archive_password: Option<zeroize::Zeroizing<String>>,
    show_confirm_delete_password: bool,
    last_window_size: (f32, f32),
    last_size_save: std::time::Instant,
    folder_sizes: HashMap<PathBuf, u64>,
    folder_size_pending: HashSet<PathBuf>,
    pending_password_jobs: HashMap<u64, PendingPasswordJob>,
    /// 正在校验记录密码的任务进度，用于展示停止入口。
    password_attempt: Option<PasswordAttemptProgress>,
    pending_extra_password: Option<PendingExtraPasswordJob>,
    show_extra_password_prompt: bool,
    pending_edit_jobs: HashMap<u64, PendingEditJob>,
    pending_archive_loads: HashMap<u64, PendingPasswordAction>,
    pending_open_entry: Option<PendingOpenEntry>,
    pending_open_target: Option<PathBuf>,
    show_open_executable_confirm: bool,
    tree_expanded: HashSet<PathBuf>,
    tree_children: HashMap<PathBuf, Vec<PathBuf>>,
    file_list_scroll: gpui_kit::base::VirtualListScrollHandle,
    system_icon_cache: std::cell::RefCell<HashMap<String, Option<Arc<gpui_kit::Image>>>>,
    file_association_count: usize,
    main_window: gpui_kit::AnyWindowHandle,
    /// 后台压缩任务没有可见窗口时显示的系统托盘图标。
    tray_icon: Option<platform::tray::TrayIcon>,
    /// 由 Shell 压缩或还原请求启动的隐藏主窗口，在任务结束后自动关闭。
    headless_shell_host: bool,
    /// 当前窗口是否负责消费单实例请求。
    ipc_poll_owner: bool,
    /// Shell 多选压缩请求按批次合并，避免每个文件各开一个窗口。
    pending_shell_compress: Vec<PathBuf>,
    shell_compress_deadline: Option<std::time::Instant>,
    settings_window: Option<gpui_kit::AnyWindowHandle>,
}

impl Drop for LumarkApp {
    /// 关闭单个归档窗口时清理该窗口产生的复制和拖出临时文件。
    fn drop(&mut self) {
        if self.ipc_poll_owner {
            IPC_POLL_OWNER.store(false, Ordering::Release);
        }
        let _ = std::fs::remove_dir_all(&self.temp_root);
    }
}

impl LumarkApp {
    /// 使用默认密码库和设置路径创建生产应用。
    ///
    /// 设置中保存了自定义密码库位置时优先使用该路径；读取设置失败时
    /// 仍然回退到默认位置，保证应用可以启动。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_shared(
            window,
            cx,
            SharedStores::open(SettingsStore::default_path()),
            None,
            StartupRequest::from_env(),
        )
    }

    /// 使用自定义密码库路径创建应用，主要用于集成测试。
    pub fn new_with_paths(
        window: &mut Window,
        cx: &mut Context<Self>,
        vault_path: PathBuf,
        start_dir: Option<PathBuf>,
    ) -> Self {
        Self::new_shared(
            window,
            cx,
            SharedStores::open_with(vault_path, SettingsStore::default_path()),
            start_dir,
            StartupRequest::from_env(),
        )
    }

    /// 使用指定存储路径创建应用，主要用于集成测试。
    pub fn new_with_stores(
        window: &mut Window,
        cx: &mut Context<Self>,
        vault_path: PathBuf,
        settings_path: PathBuf,
        start_dir: Option<PathBuf>,
    ) -> Self {
        Self::new_shared(
            window,
            cx,
            SharedStores::open_with(vault_path, settings_path),
            start_dir,
            StartupRequest::from_env(),
        )
    }

    /// 构建应用状态，应用持久化偏好，并启动后台任务轮询。
    ///
    /// `stores` 在多个窗口之间共享，保证设置和密码库只有一份状态；
    /// `request` 描述这个窗口要直接打开的压缩包，为空时显示起始目录。
    pub(crate) fn new_shared(
        window: &mut Window,
        cx: &mut Context<Self>,
        stores: SharedStores,
        start_dir: Option<PathBuf>,
        request: Option<StartupRequest>,
    ) -> Self {
        let settings = stores.settings.clone();
        let settings_error = stores.settings_error.clone();
        let settings_snapshot = settings.get();
        crate::i18n::set_language(&settings_snapshot.language);
        let headless_shell_host = request.as_ref().is_some_and(|request| {
            matches!(
                request.mode,
                StartupMode::Compress | StartupMode::Disguise | StartupMode::Restore
            )
        });
        apply_theme(
            settings_snapshot.theme,
            &settings_snapshot.accent_color,
            window,
            cx,
        );
        let remembered_dir = settings_snapshot
            .last_directory
            .clone()
            .filter(|path| path.is_dir());
        let browser = BrowserModel::new(
            start_dir
                .or(remembered_dir)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        )
        .unwrap_or_else(|_| BrowserModel::new(".").expect("current directory"));
        let vault = stores.vault.clone();
        let vault_error = stores.vault_error.clone();

        let compress_output = new_input(window, cx, "压缩包输出路径", false);
        let compress_password = new_input(window, cx, "压缩密码（可选）", true);
        let compress_comment = new_input(window, cx, "压缩包注释（可选）", false);
        let compress_exclude = new_input(window, cx, "排除通配符，用 ; 分隔", false);
        let compress_store = new_input(window, cx, "仅存储通配符，用 ; 分隔", false);
        let compress_split = new_input(window, cx, "留空表示不分卷", false);
        let compress_dictionary = new_input(window, cx, "7Z 字典大小，例如 64M", false);
        let compress_threads = new_input(window, cx, "线程数，例如 4", false);
        let compress_after = new_input(window, cx, "起始日期 YYYY-MM-DD", false);
        let compress_before = new_input(window, cx, "结束日期 YYYY-MM-DD", false);
        let extract_output = new_input(window, cx, "解压目录", false);
        let extract_password = new_input(window, cx, "解压密码", true);
        let disguise_output = new_input(window, cx, "伪装输出目录", false);
        let password_label = new_input(window, cx, "名称，例如：工作压缩包", false);
        let password_value = new_input(window, cx, "密码", true);
        let password_pattern = new_input(window, cx, "适用路径或通配符（可选）", false);
        let master_unlock = new_input(window, cx, "主密码", true);
        let master_extract_unlock = new_input(window, cx, "主密码", true);
        let master_remove = new_input(window, cx, "当前主密码", true);
        let archive_password_input = new_input(window, cx, "压缩包密码", true);
        let location_input = new_input(window, cx, "输入路径后按“转到”", false);
        let search_input = new_input(window, cx, "搜索名称或通配符", false);
        let new_folder_name = new_input(window, cx, "新文件夹名称", false);
        let rename_name = new_input(window, cx, "新名称", false);
        let password_search = new_input(window, cx, "搜索名称、备注或匹配规则", false);
        let password_note = new_input(window, cx, "备注（可选）", false);
        let vault_export_dir = new_input(window, cx, "导出文件夹", false);
        let vault_import_path = new_input(window, cx, "导入文件路径", false);
        let vault_transfer_passphrase = new_input(window, cx, "独立导出口令", true);
        let vault_transfer_unlock = new_input(window, cx, "主密码", true);
        let browser_focus = cx.focus_handle();
        let details_visible = settings_snapshot.details_visible;

        let initial_format = format_from_settings(&settings_snapshot);
        let initial_preset = settings_snapshot.default_preset;
        let mut app = Self {
            browser,
            column_widths: settings_snapshot
                .column_widths
                .unwrap_or_else(|| ColumnWidths::from_layout(settings_snapshot.column_layout)),
            column_resize: None,
            settings,
            vault,
            tasks: Arc::new(TaskManager::new()),
            screen: Screen::Browser,
            status: "就绪".to_string(),
            show_compress: false,
            compress_output,
            compress_password,
            compress_comment,
            compress_exclude,
            compress_store,
            compress_split,
            compress_split_unit: SizeUnit::default(),
            compress_dictionary,
            compress_threads,
            compress_after,
            compress_before,
            compress_format: initial_format,
            compress_preset: initial_preset,
            compress_solid: false,
            compress_encrypt_header: true,
            compress_zipcrypto: false,
            compress_test_after: false,
            compress_delete_source: false,
            compress_sfx: false,
            compress_disguise: settings_snapshot.disguise_enabled,
            compress_disguise_mask: settings_snapshot.disguise_mask,
            compress_disguise_custom: settings_snapshot.disguise_custom_mask.clone(),
            pending_shutdown_task: None,
            low_priority_tasks: HashSet::new(),
            compress_path_mode: PathMode::Relative,
            compress_tab: 0,
            compress_classic: false,
            compress_password_visible: false,
            sidebar_visible: true,
            extract_output,
            extract_password,
            password_label,
            password_value,
            password_pattern,
            master_unlock,
            master_extract_unlock,
            master_remove,
            selected_password: None,
            password_editor_window: None,
            master_password_window: None,
            active_task: None,
            last_session_check: std::time::Instant::now(),
            task_progress: None,
            active_task_kind: None,
            active_task_label: None,
            progress_windows: HashMap::new(),
            task_records: Vec::new(),
            compress_window: None,
            extract_window: None,
            extract_view: None,
            disguise_window: None,
            disguise_view: None,
            disguise_output,
            pending_extract_saves: HashMap::new(),
            pending_extract_open_dirs: HashMap::new(),
            show_save_prompt: false,
            pending_unlock_archive: None,
            show_unlock_for_extract: false,
            unlock_for_browse: false,
            show_archive_password_prompt: false,
            archive_password_input,
            archive_password_visible: false,
            archive_password_use_vault: false,
            archive_password_action: None,
            archive_password_after_failure: false,
            archive_password_save_to_vault: false,
            temp_extract: None,
            temp_root: std::env::temp_dir()
                .join("Lumark")
                .join("clipboard")
                .join(Uuid::new_v4().to_string()),
            settings_error,
            vault_error,
            location_input,
            search_input,
            search_results: None,
            search_mode: SearchMode::Wildcard,
            details_visible,
            column_layout: settings_snapshot.column_layout.min(2),
            tile_view: false,
            browser_focus,
            dropped_sources: None,
            show_delete_confirm: false,
            pending_delete_paths: Vec::new(),
            show_new_folder: false,
            new_folder_name,
            show_rename: false,
            rename_name,
            rename_target: None,
            pending_archive_additions: Vec::new(),
            pending_archive_roots: Vec::new(),
            show_archive_add_dialog: false,
            archive_add_mode: UpdateMode::Replace,
            password_search,
            password_note,
            password_page_scroll: gpui_kit::ScrollHandle::new(),
            password_list_scroll: gpui_kit::ScrollHandle::new(),
            revealed_password: None,
            show_vault_transfer: false,
            vault_export_dir,
            vault_import_path,
            vault_transfer_passphrase,
            vault_transfer_unlock,
            vault_transfer_replace: false,
            show_confirm_remove_master: false,
            show_confirm_clear_vault: false,
            archive_password: None,
            show_confirm_delete_password: false,
            last_window_size: (
                settings_snapshot.window_width,
                settings_snapshot.window_height,
            ),
            last_size_save: std::time::Instant::now(),
            folder_sizes: HashMap::new(),
            folder_size_pending: HashSet::new(),
            pending_password_jobs: HashMap::new(),
            password_attempt: None,
            pending_extra_password: None,
            show_extra_password_prompt: false,
            pending_edit_jobs: HashMap::new(),
            pending_archive_loads: HashMap::new(),
            pending_open_entry: None,
            pending_open_target: None,
            show_open_executable_confirm: false,
            tree_expanded: HashSet::new(),
            tree_children: HashMap::new(),
            file_list_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            system_icon_cache: std::cell::RefCell::new(HashMap::new()),
            file_association_count: platform::shell::registered_extension_count(),
            main_window: window.window_handle(),
            tray_icon: None,
            headless_shell_host,
            ipc_poll_owner: false,
            pending_shell_compress: Vec::new(),
            shell_compress_deadline: None,
            settings_window: None,
        };
        set_input(
            &app.location_input,
            app.browser.current_display().as_str(),
            window,
            cx,
        );
        if let Some(error) = &app.settings_error {
            app.status = format!("设置文件不可用：{error}");
        }
        if let Some(error) = &app.vault_error {
            app.status = format!("密码库不可用：{error}");
        }
        cleanup_stale_temp_dirs();
        if let Some(request) = request {
            app.apply_startup_request(request, window, cx);
        }
        app.start_task_polling(cx);
        app
    }

    /// 把启动请求应用到当前窗口，决定浏览、解压还是压缩。
    ///
    /// 请求指向的文件不存在或不是压缩包时保持普通浏览界面，不会让窗口
    /// 停在半加载状态。
    pub(crate) fn apply_startup_request(
        &mut self,
        request: StartupRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = request
            .targets()
            .into_iter()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();
        let Some(target) = targets.first().cloned() else {
            return;
        };
        // Shell 一次转发多个路径时，当前窗口处理第一个，其余各开独立窗口，
        // 避免多选打开只显示第一个归档。
        if matches!(
            request.mode,
            StartupMode::Open | StartupMode::Extract | StartupMode::Restore
        ) && targets.len() > 1
        {
            for path in targets.iter().skip(1) {
                self.open_request_window(
                    StartupRequest {
                        mode: request.mode,
                        paths: vec![path.clone()],
                    },
                    cx,
                );
            }
        }
        match request.mode {
            StartupMode::Compress => {
                // 资源管理器会为每个选中项各启动一次进程；先短暂收集再统一打开
                // 一个轻巧压缩窗口，避免多选时弹出多个窗口。
                self.queue_shell_compress(request.paths);
            }
            StartupMode::Disguise => {
                self.compress_disguise = true;
                self.open_compress_dialog_with_sources(request.paths, window, cx);
            }
            StartupMode::Extract => {
                self.open_archive_with_auto_password(
                    target,
                    ArchiveOpenIntent::Extract,
                    window,
                    cx,
                );
            }
            StartupMode::Open => {
                self.open_archive_with_auto_password(target, ArchiveOpenIntent::Browse, window, cx);
            }
            StartupMode::Restore => {
                self.start_restore_disguise(target, window, cx);
            }
            StartupMode::MountIso => {
                self.mount_iso_path(target, window, cx);
            }
            StartupMode::UnmountIso => {
                self.unmount_iso_path(target, window, cx);
            }
        }
    }

    /// 为 Shell 请求打开独立窗口，让每个压缩包拥有自己的浏览状态。
    ///
    /// 新窗口复用当前窗口的设置和密码库，并在第一帧之后把主窗口带到前台：
    /// 资源管理器转发请求时本进程没有前台权限，窗口如果停在后台，用户
    /// 会以为双击没有反应。
    pub(crate) fn open_request_window(&self, request: StartupRequest, cx: &mut Context<Self>) {
        self.spawn_window(None, Some(request), cx);
    }

    /// 打开一个复用共享存储的新窗口。
    ///
    /// `start_dir` 用于在新窗口中直接浏览文件系统目录，`request` 用于直接
    /// 打开压缩包；目录场景没有 Shell 模式，因此请求留空。
    pub(crate) fn spawn_window(
        &self,
        start_dir: Option<PathBuf>,
        request: Option<StartupRequest>,
        cx: &mut Context<Self>,
    ) {
        let stores = self.shared_stores();
        let options = window_options(&stores);
        if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
            window.on_next_frame(|window, _| window.activate_window());
            cx.new(|cx| Self::new_shared(window, cx, stores, start_dir, request))
        }) {
            eprintln!("无法打开 Lumark 窗口：{error}");
        }
    }

    /// 收集当前窗口使用的共享服务，供新窗口复用。
    fn shared_stores(&self) -> SharedStores {
        SharedStores {
            settings: self.settings.clone(),
            vault: self.vault.clone(),
            settings_error: self.settings_error.clone(),
            vault_error: self.vault_error.clone(),
        }
    }

    /// 为集成测试暴露密码库存储，同时不泄漏密钥材料。
    pub fn vault(&self) -> &VaultStore {
        &self.vault
    }
}

mod actions;
mod diagnostics;
mod theme;
mod util;
mod view;

/// 创建带点击手势的官方按钮。
///
/// 组件库默认给普通按钮使用箭头光标；这里统一补上指针，保证悬停时
/// 所有可点击操作都显示明确的可点击反馈。
pub(crate) fn button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

impl LumarkApp {
    /// 通过官方 Notification 组件提示操作结果，并同步状态栏文本。
    ///
    /// 集成测试窗口没有 Root 通知层；这种情况下只更新状态栏，避免测试环境
    /// 因缺少插件而失败。生产窗口由 `gpui_kit::open_window` 挂载 Root。
    pub(crate) fn notify(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        kind: NotificationType,
        message: impl Into<String>,
    ) {
        let message = message.into();
        self.status = message.clone();
        if window
            .root::<gpui_kit::component::Root>()
            .flatten()
            .is_some()
        {
            window.push_notification(Notification::new().with_type(kind).message(message), cx);
        }
    }
}

pub(crate) use actions::tasks::{
    aggregate_task_progress, is_terminal_task_state, task_state_label,
};
pub(crate) use theme::{apply_theme, apply_theme_accent, hsla_to_hex, theme_accent_presets};
pub(crate) use util::*;

/// 生成主窗口的初始选项，新窗口按级联序号错开避免完全重叠。
///
/// 尺寸来自共享设置，最小尺寸与启动时保持一致；窗口位置只做小范围偏移，
/// 连续打开多个压缩包时每个窗口都能露出标题栏。
fn window_options(stores: &SharedStores) -> gpui_kit::WindowOptions {
    /// 新窗口级联偏移计数。
    static CASCADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let settings = stores.settings.get();
    let cascade = CASCADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 6;
    let offset = cascade as f32 * 24.;
    gpui_kit::WindowOptions {
        window_min_size: Some(gpui_kit::size(px(960.), px(640.))),
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
            origin: gpui_kit::point(px(80. + offset), px(60. + offset)),
            size: gpui_kit::size(
                px(settings.window_width.clamp(720., 4096.)),
                px(settings.window_height.clamp(520., 2160.)),
            ),
        })),
        ..TitleBar::window_options()
    }
}

/// 启动 GPUI 应用；Shell 压缩请求只显示轻巧压缩窗口，其余请求再开新窗口。
pub fn run() -> anyhow::Result<()> {
    // 先装 panic 日志：GUI 进程没有控制台，崩溃信息只能落盘。
    diagnostics::install_panic_log();
    // 隐藏的 ZPAQ 工作进程入口必须在单实例转发之前处理，否则子进程会把
    // 任务转发给正在运行的父进程。
    if std::env::args().any(|argument| argument == "--zpaq-worker") {
        std::process::exit(lumark_archive::run_zpaq_worker());
    }
    if std::env::args().any(|argument| argument == "--register-shell") {
        lumark_platform_windows::shell::register_current_user()?;
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--unregister-shell") {
        lumark_platform_windows::shell::unregister_current_user()?;
        return Ok(());
    }
    // 超大单文件批次通过当前可执行文件的隐藏工作进程执行，以便取消时
    // 直接终止子进程。
    if let Ok(executable) = std::env::current_exe() {
        lumark_archive::configure_zpaq_worker(executable);
    }
    let instance = lumark_platform_windows::InstanceGuard::acquire("Lumark.SingleInstance")?;
    if instance.is_none() {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        if lumark_platform_windows::pipe::send_request(&arguments).is_err() {
            let _ = lumark_platform_windows::ipc::send_request(&arguments);
        }
        return Ok(());
    }
    let _instance = instance;
    lumark_platform_windows::pipe::start_server();
    let stores = SharedStores::open(SettingsStore::default_path());
    let request = StartupRequest::from_env();
    // ISO 挂载/卸载由资源管理器菜单触发，只执行动作，不打开主窗口。
    if let Some(request) = &request
        && matches!(
            request.mode,
            StartupMode::MountIso | StartupMode::UnmountIso
        )
    {
        let Some(path) = request.target().map(Path::to_path_buf) else {
            platform::show_message("Lumark", "请选择一个有效的 ISO 文件。", true);
            return Ok(());
        };
        match request.mode {
            StartupMode::MountIso => match platform::iso::mount(&path) {
                Ok(mounted) => {
                    if let Some(letter) = mounted.drive_letter {
                        let drive = PathBuf::from(format!("{letter}:\\"));
                        let _ = platform::open_default(&drive);
                    }
                }
                Err(error) => {
                    platform::show_message("Lumark - ISO 挂载失败", &error.to_string(), true);
                }
            },
            StartupMode::UnmountIso => {
                if let Err(error) = platform::iso::unmount(&path) {
                    platform::show_message("Lumark - ISO 卸载失败", &error.to_string(), true);
                }
            }
            _ => {}
        }
        return Ok(());
    }
    // 一键还原由资源管理器菜单触发，隐藏主窗口只显示进度窗口；目标缺失时
    // 直接提示并退出，避免留下不可见的后台进程。
    if let Some(request) = &request
        && request.mode == StartupMode::Restore
        && request.target().is_none()
    {
        platform::show_message("Lumark 一键还原", "请选择一个有效文件。", true);
        return Ok(());
    }
    let mut window_options = window_options(&stores);
    // 资源管理器压缩和还原只需要进度窗口；主窗口仅作为隐藏的宿主，
    // 任务结束后再自动退出。
    if request.as_ref().is_some_and(|request| {
        matches!(
            request.mode,
            StartupMode::Compress | StartupMode::Disguise | StartupMode::Restore
        )
    }) {
        window_options.show = false;
        window_options.focus = false;
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            use gpui_kit::component as gpui_component;
            rust_i18n::extend!(gpui_component);
            gpui_kit::init(cx);
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| LumarkApp::new_shared(window, cx, stores, None, request))
            })
            .expect("open Lumark window");
        });
    Ok(())
}

#[cfg(test)]
mod tests;
