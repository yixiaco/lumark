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
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClipboardItem, Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Window, div,
    hsla, px,
};
use uuid::Uuid;

use lumark_archive::{
    ArchiveFormat, ArchiveService, ArchiveTimeSource, CompressionOptions, EditOptions, EditRoot,
    ExtractionOptions, FileTimeFilter, OverwritePolicy as ArchiveOverwritePolicy, PathMode,
    SevenZCompression, UpdateMode, ZipCompression, ZipEncryption,
};
use lumark_browser::{BrowserEntry, BrowserLocation, BrowserModel, SearchMode};
use lumark_platform_windows as platform;
use lumark_settings::{
    CompressionPreset as SettingsPreset, Settings, SettingsStore, ThemePreference,
};
use lumark_tasks::{TaskEvent, TaskKind, TaskManager, TaskProgress, TaskRequest, TaskState};
use lumark_vault::{PasswordEntry, VaultState, VaultStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Browser,
    Passwords,
    Tasks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveOpenIntent {
    Browse,
    Extract,
}

#[derive(Debug, Clone)]
pub(crate) enum PendingPasswordAction {
    OpenArchive {
        archive: PathBuf,
        intent: ArchiveOpenIntent,
    },
    ExtractDialog {
        archive: PathBuf,
    },
    OpenEntry {
        archive: PathBuf,
        relative: String,
        mode: PendingOpenMode,
    },
    Test {
        archive: PathBuf,
    },
}

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
pub(crate) enum PendingEditAction {
    Add,
    Delete,
    Rename,
}

struct PendingEditJob {
    archive: PathBuf,
    action: PendingEditAction,
}

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
    pub(crate) id: u64,
    pub(crate) kind: TaskKind,
    pub(crate) label: String,
    pub(crate) state: TaskState,
    pub(crate) progress: TaskProgress,
    pub(crate) message: Option<String>,
}

/// 拖出或复制归档条目时的临时解压进度弹窗。
#[derive(Clone)]
pub(crate) struct TempExtractDialog {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) progress: f32,
}

pub(crate) struct TreeRow {
    path: PathBuf,
    label: String,
    depth: usize,
    expanded: bool,
}

/// Shell 请求的打开方式，对应 `--open`、`--extract` 和 `--compress`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupMode {
    Open,
    Extract,
    Compress,
}

/// 一个由独立窗口处理的启动请求。
///
/// 首次启动来自命令行参数，之后由单实例转发的请求队列产生；两种来源
/// 都只描述要处理哪些路径，窗口和界面状态由接收它的窗口自己创建。
#[derive(Debug, Clone)]
pub(crate) struct StartupRequest {
    pub(crate) mode: StartupMode,
    pub(crate) paths: Vec<PathBuf>,
}

impl StartupRequest {
    /// 解析命令行参数并保留非 UTF-8 路径。
    pub(crate) fn from_env() -> Option<Self> {
        Self::parse_os(std::env::args_os().skip(1))
    }

    /// 解析单实例转发请求，参数已经由发送方转换成 UTF-8 文本。
    pub(crate) fn parse(arguments: Vec<String>) -> Option<Self> {
        Self::parse_os(arguments.into_iter().map(std::ffi::OsString::from))
    }

    /// 识别三种模式；未知开关忽略，其余参数按路径处理。
    fn parse_os(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Option<Self> {
        let mut mode = StartupMode::Open;
        let mut paths = Vec::new();
        for argument in arguments {
            let text = argument.to_string_lossy();
            match text.as_ref() {
                "--open" => mode = StartupMode::Open,
                "--extract" => mode = StartupMode::Extract,
                "--compress" => mode = StartupMode::Compress,
                value if value.starts_with('-') => {}
                _ => paths.push(PathBuf::from(argument)),
            }
        }
        if paths.is_empty() {
            return None;
        }
        Some(Self { mode, paths })
    }

    /// 返回请求真正要处理的第一个路径。
    ///
    /// 打开和解压只接受压缩包文件，压缩还接受文件夹；不匹配时返回 `None`，
    /// 避免为无效请求打开一个空窗口。
    pub(crate) fn target(&self) -> Option<&Path> {
        self.paths
            .iter()
            .map(PathBuf::as_path)
            .find(|path| match self.mode {
                StartupMode::Compress => path.exists(),
                StartupMode::Open | StartupMode::Extract => path.is_file() && is_archive_path(path),
            })
    }
}

/// 多个窗口共享的持久化服务。
///
/// 设置和密码库必须进程内唯一：每个窗口各自打开会互相覆盖设置文件，
/// 并让密码库出现多份互不可见的解锁状态。
#[derive(Clone)]
pub(crate) struct SharedStores {
    settings: Arc<SettingsStore>,
    vault: Arc<VaultStore>,
    settings_error: Option<String>,
    vault_error: Option<String>,
}

impl SharedStores {
    /// 读取设置，并按设置中记录的密码库位置打开共享存储。
    pub(crate) fn open(settings_path: PathBuf) -> Self {
        let (settings, settings_error) = open_settings(settings_path);
        let vault_path = settings
            .get()
            .vault_path
            .unwrap_or_else(VaultStore::default_path);
        Self::with_vault(settings, settings_error, vault_path)
    }

    /// 使用调用方指定的密码库位置打开，供自定义路径和集成测试使用。
    pub(crate) fn open_with(vault_path: PathBuf, settings_path: PathBuf) -> Self {
        let (settings, settings_error) = open_settings(settings_path);
        Self::with_vault(settings, settings_error, vault_path)
    }

    /// 打开密码库；失败时退回临时目录，保证窗口仍能启动并提示错误。
    fn with_vault(
        settings: Arc<SettingsStore>,
        settings_error: Option<String>,
        vault_path: PathBuf,
    ) -> Self {
        let (vault, vault_error) = match VaultStore::open(vault_path) {
            Ok(vault) => (Arc::new(vault), None),
            Err(error) => {
                eprintln!("vault open failed: {error}");
                (
                    Arc::new(
                        VaultStore::open(
                            std::env::temp_dir().join("lumark-vault").join("vault.dat"),
                        )
                        .expect("temporary vault"),
                    ),
                    Some(error.to_string()),
                )
            }
        };
        Self {
            settings,
            vault,
            settings_error,
            vault_error,
        }
    }
}

/// 读取设置文件；损坏或不可读时退回临时目录并保留错误文本。
fn open_settings(settings_path: PathBuf) -> (Arc<SettingsStore>, Option<String>) {
    match SettingsStore::open(settings_path) {
        Ok(settings) => (Arc::new(settings), None),
        Err(error) => {
            let fallback = std::env::temp_dir()
                .join("lumark-settings")
                .join("settings.json");
            (
                Arc::new(SettingsStore::open(fallback).expect("temporary Lumark settings store")),
                Some(error.to_string()),
            )
        }
    }
}

pub struct LumarkApp {
    browser: BrowserModel,
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
    task_progress: Option<TaskProgress>,
    active_task_kind: Option<TaskKind>,
    active_task_label: Option<String>,
    progress_windows: HashMap<u64, view::windows::ProgressWindowEntry>,
    task_records: Vec<TaskRecord>,
    compress_window: Option<gpui_kit::AnyWindowHandle>,
    extract_window: Option<gpui_kit::AnyWindowHandle>,
    extract_view: Option<gpui_kit::Entity<view::extract_window::ExtractWindowView>>,
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
    vault_transfer_path: gpui_kit::Entity<InputState>,
    vault_transfer_passphrase: gpui_kit::Entity<InputState>,
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
    /// 由 Shell 压缩请求启动的隐藏主窗口，在压缩窗口和任务结束后自动关闭。
    headless_compress_host: bool,
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
        let headless_compress_host = request
            .as_ref()
            .is_some_and(|request| request.mode == StartupMode::Compress);
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
        let compress_split = new_input(window, cx, "分卷大小，例如 100M", false);
        let compress_dictionary = new_input(window, cx, "7Z 字典大小，例如 64M", false);
        let compress_threads = new_input(window, cx, "线程数，例如 4", false);
        let compress_after = new_input(window, cx, "起始日期 YYYY-MM-DD", false);
        let compress_before = new_input(window, cx, "结束日期 YYYY-MM-DD", false);
        let extract_output = new_input(window, cx, "解压目录", false);
        let extract_password = new_input(window, cx, "解压密码", true);
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
        let vault_transfer_path = new_input(window, cx, "导出/导入文件路径", false);
        let vault_transfer_passphrase = new_input(window, cx, "独立导出口令", true);
        let browser_focus = cx.focus_handle();
        let details_visible = settings_snapshot.details_visible;

        let initial_format = format_from_settings(&settings_snapshot);
        let initial_preset = settings_snapshot.default_preset;
        let mut app = Self {
            browser,
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
            task_progress: None,
            active_task_kind: None,
            active_task_label: None,
            progress_windows: HashMap::new(),
            task_records: Vec::new(),
            compress_window: None,
            extract_window: None,
            extract_view: None,
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
            vault_transfer_path,
            vault_transfer_passphrase,
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
            headless_compress_host,
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
        let Some(target) = request.target().map(Path::to_path_buf) else {
            return;
        };
        match request.mode {
            StartupMode::Compress => {
                // 资源管理器会为每个选中项各启动一次进程；先短暂收集再统一打开
                // 一个轻巧压缩窗口，避免多选时弹出多个窗口。
                self.queue_shell_compress(request.paths);
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

pub(crate) use actions::tasks::{is_terminal_task_state, task_state_label};
pub(crate) use theme::{apply_theme, apply_theme_accent, hsla_to_hex, theme_accent_presets};
pub(crate) use util::*;

/// 生成主窗口的初始选项，新窗口按级联序号错开避免完全重叠。
///
/// 尺寸来自共享设置，最小尺寸与启动时保持一致；窗口位置只做小范围偏移，
/// 连续打开多个压缩包时每个窗口都能露出标题栏。
fn window_options(stores: &SharedStores) -> gpui_kit::WindowOptions {
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
    if std::env::args().any(|argument| argument == "--register-shell") {
        lumark_platform_windows::shell::register_current_user()?;
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--unregister-shell") {
        lumark_platform_windows::shell::unregister_current_user()?;
        return Ok(());
    }
    let instance = lumark_platform_windows::InstanceGuard::acquire("Lumark.SingleInstance")?;
    if instance.is_none() {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        let _ = lumark_platform_windows::ipc::send_request(&arguments);
        return Ok(());
    }
    let _instance = instance;
    let stores = SharedStores::open(SettingsStore::default_path());
    let request = StartupRequest::from_env();
    let mut window_options = window_options(&stores);
    // 资源管理器压缩只需要压缩窗口；主窗口仅作为隐藏的宿主，
    // 压缩窗口关闭或任务结束后再自动退出。
    if request
        .as_ref()
        .is_some_and(|request| request.mode == StartupMode::Compress)
    {
        window_options.show = false;
        window_options.focus = false;
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
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
