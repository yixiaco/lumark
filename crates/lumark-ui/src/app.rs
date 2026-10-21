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
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, Theme, ThemeMode, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClipboardItem, Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Window, div,
    hsla, px,
};
use uuid::Uuid;

use lumark_archive::{
    ArchiveFormat, ArchiveService, ArchiveTimeSource, CompressionOptions, EditOptions,
    ExtractionOptions, FileTimeFilter, OverwritePolicy as ArchiveOverwritePolicy, PathMode,
    SevenZCompression, ZipCompression, ZipEncryption,
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

struct PendingPasswordJob {
    action: PendingPasswordAction,
    entry_ids: Vec<Uuid>,
    extras: Vec<lumark_vault::PasswordCandidate>,
}

struct PendingExtraPasswordJob {
    action: PendingPasswordAction,
    candidates: Vec<lumark_vault::PasswordCandidate>,
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

    /// 把 Shell 传来的压缩路径加入短暂批次。
    ///
    /// 资源管理器多选时会为每个文件启动一次进程，这里用一个很短的收集
    /// 窗口把这些请求合并成同一个压缩任务。
    fn queue_shell_compress(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if !self.pending_shell_compress.contains(&path) {
                self.pending_shell_compress.push(path);
            }
        }
        if !self.pending_shell_compress.is_empty() {
            self.shell_compress_deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
        }
    }

    /// 收集窗口结束后，用全部选中路径打开一个轻巧压缩窗口。
    fn flush_shell_compress(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(deadline) = self.shell_compress_deadline else {
            return;
        };
        if std::time::Instant::now() < deadline {
            return;
        }
        self.shell_compress_deadline = None;
        let mut paths = std::mem::take(&mut self.pending_shell_compress);
        let mut seen = HashSet::new();
        paths.retain(|path| path.exists() && seen.insert(path.clone()));
        if !paths.is_empty() {
            self.open_compress_dialog_with_sources(paths, window, cx);
        }
    }

    /// 关闭仅供 Shell 压缩使用的隐藏主窗口；所有压缩相关窗口都结束后才执行。
    fn maybe_close_headless_host(&mut self, cx: &mut Context<Self>) {
        if !self.headless_compress_host || !self.pending_shell_compress.is_empty() {
            return;
        }
        if self
            .compress_window
            .as_ref()
            .is_some_and(|handle| handle.update(cx, |_, _, _| {}).is_ok())
        {
            return;
        }
        if self.active_task.is_some() {
            return;
        }
        if self
            .progress_windows
            .values()
            .any(|entry| entry.handle.update(cx, |_, _, _| {}).is_ok())
        {
            return;
        }
        self.headless_compress_host = false;
        let _ = self
            .main_window
            .update(cx, |_, window, _| window.remove_window());
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

    /// 启动后台任务事件桥；它先合并进度，再通知界面。
    fn start_task_polling(&self, cx: &mut Context<Self>) {
        let tasks = self.tasks.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                let mut events = Vec::new();
                while let Some(event) = tasks.try_recv() {
                    events.push(event);
                }
                let _ = this.update_in(cx, |app, window, cx| {
                    if !app.ipc_poll_owner && !IPC_POLL_OWNER.swap(true, Ordering::AcqRel) {
                        app.ipc_poll_owner = true;
                    }
                    let requests = if app.ipc_poll_owner {
                        lumark_platform_windows::ipc::take_requests().unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let had_requests = !requests.is_empty();
                    let had_events = !events.is_empty() || had_requests;
                    let auto_locked = app.vault.maybe_auto_lock();
                    if auto_locked {
                        app.dismiss_password_windows(cx);
                        app.selected_password = None;
                        app.pending_extra_password = None;
                        app.show_extra_password_prompt = false;
                        app.status = "密码库已自动锁定".to_string();
                    }
                    for event in events {
                        match event {
                            TaskEvent::Queued { id, kind, label } => {
                                app.record_task_queued(id, kind, label.clone());
                                if kind != TaskKind::PasswordResolve {
                                    app.active_task = Some(id);
                                    app.task_progress = None;
                                    if matches!(
                                        kind,
                                        TaskKind::Compress | TaskKind::Extract | TaskKind::Test
                                    ) {
                                        app.active_task_kind = Some(kind);
                                        app.active_task_label = Some(label.clone());
                                        cx.defer_in(window, move |app, window, cx| {
                                            app.open_progress_window_for(id, window, cx);
                                        });
                                    }
                                }
                                app.status = label;
                            }
                            TaskEvent::Progress { id, progress } => {
                                app.record_task_progress(id, progress.clone());
                                app.update_progress_window_for(id, progress.clone(), cx);
                                if app.active_task == Some(id) {
                                    app.task_progress = Some(progress);
                                }
                            }
                            TaskEvent::Completed { id, message } => {
                                app.record_task_completed(id, message.clone());
                                if let Some(job) = app.pending_password_jobs.remove(&id) {
                                    app.vault.release_lease();
                                    if let Some(resolved) = tasks.take_password_result(id) {
                                        if let Some(entry_id) =
                                            job.entry_ids.get(resolved.candidate_index)
                                        {
                                            let _ = app.vault.mark_used(*entry_id);
                                        }
                                        let archive_info = tasks
                                            .take_archive_load_result(id)
                                            .map(|result| result.info);
                                        app.apply_resolved_password(
                                            job.action,
                                            resolved.password,
                                            archive_info,
                                            window,
                                            cx,
                                        );
                                    } else {
                                        if job.extras.is_empty() {
                                            app.status =
                                                "记录密码均不正确，请手动输入密码".to_string();
                                            app.apply_password_failure(job.action, window, cx);
                                        } else {
                                            let count = job.extras.len();
                                            app.pending_extra_password =
                                                Some(PendingExtraPasswordJob {
                                                    action: job.action,
                                                    candidates: job.extras,
                                                });
                                            app.show_extra_password_prompt = true;
                                            app.status = format!(
                                                "前 32 条记录密码不正确，还有 {count} 条未尝试"
                                            );
                                        }
                                    }
                                    continue;
                                }
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    if let Some(pending) = app.pending_open_entry.take() {
                                        if app.active_task == Some(id) {
                                            app.active_task = None;
                                            app.task_progress = None;
                                        }
                                        // 条目只为“打开”服务，解压完成即关闭进度窗口，
                                        // 否则窗口会一直挡住刚打开的文档。
                                        app.dismiss_progress_window_for(id, cx);
                                        let target = lumark_archive::safe_join(
                                            &pending.temporary,
                                            &pending.relative,
                                        )
                                        .unwrap_or_else(|_| pending.temporary.clone());
                                        match pending.mode {
                                            PendingOpenMode::External => {
                                                app.open_extracted_target(
                                                    target,
                                                    &pending.temporary,
                                                );
                                            }
                                            PendingOpenMode::Viewer => {
                                                app.open_text_viewer(
                                                    target,
                                                    pending.relative.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if let Some(action) = app.pending_archive_loads.remove(&id) {
                                    if app.active_task == Some(id) {
                                        app.active_task = None;
                                        app.task_progress = None;
                                    }
                                    if let Some(result) = tasks.take_archive_load_result(id) {
                                        app.apply_loaded_archive(action, result, window, cx);
                                    } else {
                                        app.status = message;
                                    }
                                    continue;
                                }
                                if let Some(edit) = app.pending_edit_jobs.remove(&id) {
                                    if app.active_task == Some(id) {
                                        app.active_task = None;
                                        app.task_progress = None;
                                    }
                                    let current_archive = match &app.browser.location {
                                        BrowserLocation::Archive { archive, .. } => {
                                            Some(archive.clone())
                                        }
                                        BrowserLocation::FileSystem(_) => None,
                                    };
                                    if current_archive.as_ref() == Some(&edit.archive) {
                                        let password =
                                            app.archive_password.as_deref().map(String::as_str);
                                        if let Err(error) =
                                            app.browser.open_archive(&edit.archive, password)
                                        {
                                            app.status = error.to_string();
                                        } else {
                                            set_input(
                                                &app.location_input,
                                                app.browser.current_display().as_str(),
                                                window,
                                                cx,
                                            );
                                        }
                                    }
                                    app.finish_edit_action(edit.action);
                                    if current_archive.as_ref() == Some(&edit.archive) {
                                        app.status = match edit.action {
                                            PendingEditAction::Add => {
                                                "已将文件添加到归档".to_string()
                                            }
                                            PendingEditAction::Delete => {
                                                "已从归档删除项目".to_string()
                                            }
                                            PendingEditAction::Rename => {
                                                "已重命名归档条目".to_string()
                                            }
                                        };
                                    } else {
                                        app.status = message;
                                    }
                                    continue;
                                }
                                app.dismiss_progress_window_for(id, cx);
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = message;
                                if let Some(directory) = app.pending_extract_open_dirs.remove(&id) {
                                    let _ = std::process::Command::new("explorer.exe")
                                        .arg(directory)
                                        .spawn();
                                }
                                if let Some((archive, password)) =
                                    app.pending_extract_saves.remove(&id)
                                    && app.vault.settings().prompt_save
                                {
                                    let label = archive
                                        .file_name()
                                        .and_then(|value| value.to_str())
                                        .unwrap_or("压缩包密码")
                                        .to_string();
                                    set_input(&app.password_label, &label, window, cx);
                                    set_input(&app.password_value, password.as_str(), window, cx);
                                    set_input(
                                        &app.password_pattern,
                                        archive.to_string_lossy().as_ref(),
                                        window,
                                        cx,
                                    );
                                    app.selected_password = None;
                                    app.show_save_prompt = true;
                                }
                            }
                            TaskEvent::Failed { id, message } => {
                                app.record_task_failed(id, message.clone());
                                app.pending_extract_saves.remove(&id);
                                app.pending_extract_open_dirs.remove(&id);
                                if app.pending_password_jobs.remove(&id).is_some() {
                                    app.vault.release_lease();
                                }
                                app.pending_edit_jobs.remove(&id);
                                app.pending_archive_loads.remove(&id);
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    app.pending_open_entry = None;
                                }
                                app.dismiss_progress_window_for(id, cx);
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = format!("失败：{message}");
                            }
                            TaskEvent::Cancelled { id } => {
                                app.record_task_cancelled(id);
                                app.pending_extract_saves.remove(&id);
                                app.pending_extract_open_dirs.remove(&id);
                                if app.pending_password_jobs.remove(&id).is_some() {
                                    app.vault.release_lease();
                                }
                                app.pending_edit_jobs.remove(&id);
                                app.pending_archive_loads.remove(&id);
                                if app
                                    .pending_open_entry
                                    .as_ref()
                                    .is_some_and(|pending| pending.task_id == id)
                                {
                                    app.pending_open_entry = None;
                                }
                                app.dismiss_progress_window_for(id, cx);
                                if app.active_task == Some(id) {
                                    app.focus_next_running_task(id);
                                }
                                app.status = "任务已取消".to_string();
                            }
                            TaskEvent::PasswordAttempt { id, index, total } => {
                                if app.pending_password_jobs.contains_key(&id) {
                                    app.status = format!("正在尝试记录密码 {index}/{total}");
                                }
                            }
                            TaskEvent::StateChanged { id, state } => {
                                app.record_task_state(id, state);
                            }
                        }
                    }
                    // 压缩请求按批次合并；打开/解压仍由独立窗口处理，避免覆盖
                    // 当前窗口正在查看的压缩包。
                    for arguments in requests {
                        if let Some(request) = StartupRequest::parse(arguments) {
                            if request.mode == StartupMode::Compress {
                                app.queue_shell_compress(request.paths);
                            } else {
                                app.open_request_window(request, cx);
                            }
                        }
                    }
                    app.flush_shell_compress(window, cx);
                    app.maybe_close_headless_host(cx);
                    if had_events || auto_locked {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
}

mod actions;
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
pub(crate) use util::*;
/// 默认强调色：蓝色。
const DEFAULT_ACCENT_COLOR: &str = "#2563eb";

/// 在 gpui-kit 基础主题上应用 Lumark 的中性表面和用户强调色。
///
/// 语义颜色会同步刷新，确保原生控件与自定义表面在浅色和深色模式之间保持一致；
/// 强调色来自设置，默认蓝色，修改后无需重启即可刷新所有窗口。
fn apply_theme(
    preference: ThemePreference,
    accent_color: &str,
    window: &mut Window,
    cx: &mut Context<LumarkApp>,
) {
    let mode = match preference {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::System => match window.appearance() {
            gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark => {
                ThemeMode::Dark
            }
            _ => ThemeMode::Light,
        },
    };
    Theme::change(mode, Some(window), cx);
    Theme::update(cx, |theme| {
        let dark = theme.mode.is_dark();
        let background = if dark {
            hsla(217. / 360., 0.25, 0.08, 1.)
        } else {
            hsla(220. / 360., 0.33, 0.97, 1.)
        };
        let surface = if dark {
            hsla(217. / 360., 0.22, 0.12, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        let foreground = if dark {
            hsla(214. / 360., 0.24, 0.92, 1.)
        } else {
            hsla(220. / 360., 0.27, 0.15, 1.)
        };
        let muted_foreground = if dark {
            hsla(215. / 360., 0.12, 0.62, 1.)
        } else {
            hsla(217. / 360., 0.12, 0.47, 1.)
        };
        let border = if dark {
            hsla(216. / 360., 0.18, 0.20, 1.)
        } else {
            hsla(214. / 360., 0.32, 0.91, 1.)
        };

        theme.background = background;
        theme.foreground = foreground;
        theme.popover = surface;
        theme.popover_foreground = foreground;
        theme.border = border;
        theme.input = border;
        theme.muted = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(218. / 360., 0.32, 0.96, 1.)
        };
        theme.muted_foreground = muted_foreground;
        theme.secondary = theme.muted;
        theme.secondary_hover = if dark {
            hsla(217. / 360., 0.18, 0.20, 1.)
        } else {
            hsla(216. / 360., 0.29, 0.93, 1.)
        };
        theme.secondary_active = if dark {
            hsla(217. / 360., 0.17, 0.24, 1.)
        } else {
            hsla(216. / 360., 0.27, 0.90, 1.)
        };
        theme.secondary_foreground = foreground;
        theme.button = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        theme.button_hover = theme.secondary_hover;
        theme.button_active = theme.secondary_active;
        theme.button_foreground = foreground;
        theme.title_bar = if dark {
            hsla(217. / 360., 0.25, 0.09, 1.)
        } else {
            hsla(0., 0., 1., 1.)
        };
        theme.title_bar_border = border;
        theme.sidebar = if dark {
            hsla(217. / 360., 0.24, 0.10, 1.)
        } else {
            hsla(220. / 360., 0.28, 0.985, 1.)
        };
        theme.sidebar_foreground = foreground;
        theme.sidebar_border = border;
        theme.sidebar_primary = if dark {
            hsla(217. / 360., 0.20, 0.15, 1.)
        } else {
            hsla(218. / 360., 0.31, 0.955, 1.)
        };
        theme.list_hover = if dark {
            hsla(217. / 360., 0.19, 0.16, 1.)
        } else {
            hsla(218. / 360., 0.32, 0.955, 1.)
        };
        theme.list_head = if dark {
            hsla(217. / 360., 0.21, 0.13, 1.)
        } else {
            hsla(218. / 360., 0.30, 0.975, 1.)
        };
        theme.table = surface;
        theme.table_head = theme.list_head;
        theme.table_hover = theme.list_hover;
        theme.status_bar = theme.title_bar;
        theme.status_bar_border = border;
        theme.group_box = surface;
        theme.group_box_foreground = foreground;
        theme.overlay = hsla(220. / 360., 0.22, 0.08, 0.40);
        theme.success = if dark {
            hsla(156. / 360., 0.45, 0.20, 1.)
        } else {
            hsla(151. / 360., 0.52, 0.91, 1.)
        };
        theme.success_foreground = if dark {
            hsla(151. / 360., 0.60, 0.72, 1.)
        } else {
            hsla(152. / 360., 0.62, 0.27, 1.)
        };
        theme.warning = if dark {
            hsla(38. / 360., 0.50, 0.20, 1.)
        } else {
            hsla(40. / 360., 0.84, 0.91, 1.)
        };
        theme.warning_foreground = if dark {
            hsla(41. / 360., 0.86, 0.70, 1.)
        } else {
            hsla(33. / 360., 0.75, 0.30, 1.)
        };
        theme.danger = if dark {
            hsla(356. / 360., 0.52, 0.22, 1.)
        } else {
            hsla(356. / 360., 0.78, 0.94, 1.)
        };
        theme.danger_foreground = if dark {
            hsla(356. / 360., 0.84, 0.74, 1.)
        } else {
            hsla(356. / 360., 0.66, 0.38, 1.)
        };
        theme.radius = px(6.);
        theme.radius_lg = px(14.);
        theme.font_size = px(13.5);
        theme.shadow = true;
        theme.scrollbar_mode = gpui_kit::component::scroll::ScrollbarMode::Always;
        apply_accent_color(theme, accent_color);
    });
}

/// 只刷新强调色相关的语义 token，供设置页颜色选择器实时调用。
fn apply_theme_accent(accent_color: &str, cx: &mut Context<LumarkApp>) {
    Theme::update(cx, |theme| apply_accent_color(theme, accent_color));
}

/// 从用户选定的主题色派生主色、悬停色、选中底色和前景色。
///
/// 只保留用户选择的色相和饱和度，明度按浅色/深色模式收敛到可读区间，
/// 再用相对亮度决定主按钮文字使用白色还是深色，避免浅黄色等主题失去对比度。
fn apply_accent_color(theme: &mut Theme, accent_color: &str) {
    let dark = theme.mode.is_dark();
    let base = gpui_kit::component::try_parse_color(accent_color)
        .or_else(|_| gpui_kit::component::try_parse_color(DEFAULT_ACCENT_COLOR))
        .unwrap_or_else(|_| hsla(221. / 360., 0.83, 0.53, 1.));
    let saturation = base.s.clamp(0.12, 0.92);
    let lightness = if dark {
        base.l.clamp(0.56, 0.76)
    } else {
        base.l.clamp(0.34, 0.68)
    };
    let primary = hsla(base.h, saturation, lightness, 1.);
    let primary_hover = hsla(
        base.h,
        saturation,
        (lightness + if dark { 0.05 } else { 0.06 }).min(0.82),
        1.,
    );
    let primary_active = hsla(
        base.h,
        saturation,
        (lightness - if dark { 0.05 } else { 0.07 }).max(0.24),
        1.,
    );
    let rgba = gpui_kit::Rgba::from(primary);
    let luminance = 0.2126 * rgba.r + 0.7152 * rgba.g + 0.0722 * rgba.b;
    let primary_foreground = if luminance > 0.56 {
        hsla(base.h, 0.25, 0.12, 1.)
    } else {
        hsla(0., 0., 1., 1.)
    };
    let accent = if dark {
        hsla(base.h, (saturation * 0.38).clamp(0.12, 0.36), 0.19, 1.)
    } else {
        hsla(base.h, (saturation * 0.50).clamp(0.18, 0.52), 0.94, 1.)
    };
    let accent_foreground = if dark {
        hsla(base.h, (saturation * 0.55).clamp(0.20, 0.55), 0.80, 1.)
    } else {
        hsla(base.h, (saturation * 0.70).clamp(0.25, 0.75), 0.32, 1.)
    };

    theme.primary = primary;
    theme.primary_hover = primary_hover;
    theme.primary_active = primary_active;
    theme.primary_foreground = primary_foreground;
    theme.button_primary = primary;
    theme.button_primary_hover = primary_hover;
    theme.button_primary_active = primary_active;
    theme.button_primary_foreground = primary_foreground;
    theme.accent = accent;
    theme.accent_foreground = accent_foreground;
    theme.selection = accent;
    theme.ring = primary;
    theme.link = primary;
    theme.link_hover = primary_hover;
    theme.link_active = primary_active;
    theme.list_active = accent;
    theme.list_active_border = primary;
    theme.sidebar_accent = accent;
    theme.sidebar_accent_foreground = accent_foreground;
    theme.table_active = accent;
}

/// 将颜色选择器返回的 Hsla 转成可持久化的十六进制字符串。
fn hsla_to_hex(color: gpui_kit::Hsla) -> String {
    let rgba = gpui_kit::Rgba::from(color);
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r.clamp(0., 1.) * 255.).round() as u8,
        (rgba.g.clamp(0., 1.) * 255.).round() as u8,
        (rgba.b.clamp(0., 1.) * 255.).round() as u8
    )
}

/// 设置页颜色选择器提供的快捷主题色。
fn theme_accent_presets() -> Vec<gpui_kit::Hsla> {
    [
        "#2563eb", "#0d9488", "#16a34a", "#7c3aed", "#ea580c", "#e11d48",
    ]
    .iter()
    .filter_map(|hex| gpui_kit::component::try_parse_color(hex).ok())
    .collect()
}

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
mod startup_tests {
    use super::{StartupMode, StartupRequest};
    use std::path::PathBuf;

    /// 验证三种开关都能解析，并且路径原样保留。
    #[test]
    fn startup_request_parses_mode_and_paths() {
        let request = StartupRequest::parse(vec![
            "--extract".to_string(),
            r"C:\downloads\report.zip".to_string(),
        ])
        .expect("extract request");
        assert_eq!(request.mode, StartupMode::Extract);
        assert_eq!(
            request.paths,
            vec![PathBuf::from(r"C:\downloads\report.zip")]
        );

        let request = StartupRequest::parse(vec![
            "--compress".to_string(),
            r"C:\work\images".to_string(),
        ])
        .expect("compress request");
        assert_eq!(request.mode, StartupMode::Compress);
    }

    /// 验证没有路径时不会创建窗口，未知开关按默认浏览处理。
    #[test]
    fn startup_request_ignores_empty_and_unknown_arguments() {
        assert!(StartupRequest::parse(vec!["--open".to_string()]).is_none());
        assert!(StartupRequest::parse(vec!["--register-shell".to_string()]).is_none());
        let request = StartupRequest::parse(vec![
            "--unknown".to_string(),
            r"C:\downloads\a.zip".to_string(),
        ])
        .expect("default open request");
        assert_eq!(request.mode, StartupMode::Open);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use gpui_kit::test::TestWindowExt;
    use lumark_vault::{VaultCrypto, VaultSettings};
    use tempfile::tempdir;

    #[gpui_kit::test]
    /// 验证虚拟文件行和平铺卡片都暴露预期的测试目标。
    fn browser_renders_virtual_rows_and_tile_view(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        for index in 0..80 {
            std::fs::write(
                directory.path().join(format!("file-{index:03}.txt")),
                b"lumark",
            )
            .unwrap();
        }
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let settings_path = directory.path().join("settings.json");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        let first_row = format!(
            "file-row-{}",
            directory.path().join("file-000.txt").display()
        );
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert!(window.find(first_row.clone()).visible());
            window.click("view-toggle", cx);
            window.draw(cx).clear(cx);
            let first_tile = format!(
                "file-tile-name-{}",
                directory.path().join("file-000.txt").display()
            );
            assert!(window.find(first_tile).visible());
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码列表先消费滚轮，外层页面不会抢先滚动。
    fn password_list_scrolls_before_page(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.vault.unlock("master-pass").unwrap();
                for index in 0..20 {
                    app.vault
                        .add_entry(PasswordEntry::new(
                            format!("记录 {index}"),
                            format!("pass-{index}"),
                        ))
                        .unwrap();
                }
                app.screen = Screen::Passwords;
                cx.notify();
            });
            window.draw(cx).clear(cx);
            window.scroll(
                "password-list",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-200.))),
                cx,
            );
            window.draw(cx).clear(cx);
            let (list_offset, page_offset, page_max) = {
                let app = app.read(cx);
                (
                    app.password_list_scroll.offset().y,
                    app.password_page_scroll.offset().y,
                    app.password_page_scroll.max_offset().y,
                )
            };
            assert!(list_offset < px(0.));
            assert_eq!(page_offset, px(0.));
            if page_max < px(0.) {
                window.scroll(
                    "lock-vault",
                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-60.))),
                    cx,
                );
                window.draw(cx).clear(cx);
                assert!(app.read(cx).password_page_scroll.offset().y < px(0.));
            }
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证 Shell 多选压缩会合并路径，只打开一个轻巧压缩窗口。
    fn shell_compress_batches_into_one_window(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let first = directory.path().join("第一份.txt");
        let second = directory.path().join("第二份.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_shell_compress(vec![first.clone()]);
                app.queue_shell_compress(vec![second.clone(), first.clone()]);
                assert_eq!(app.pending_shell_compress.len(), 2);
                app.shell_compress_deadline = Some(std::time::Instant::now());
                app.flush_shell_compress(window, cx);
            });
            window.draw(cx).clear(cx);
            let mut compress_open = false;
            let handle = app.read(cx).compress_window.unwrap();
            handle
                .update(cx, |_, _, _| {
                    compress_open = true;
                })
                .unwrap();
            assert!(compress_open);
            let output = {
                let app = app.read(cx);
                input_value(&app.compress_output, cx)
            };
            assert_eq!(
                Path::new(&output)
                    .file_stem()
                    .and_then(|value| value.to_str()),
                directory
                    .path()
                    .file_name()
                    .and_then(|value| value.to_str()),
                "多选压缩应使用父目录名作为默认归档名"
            );
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证打开加密归档时会弹出独立密码输入框，而不是只改状态栏。
    fn encrypted_archive_requests_password_dialog(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let source = directory.path().join("secret.txt");
        std::fs::write(&source, b"secret").unwrap();
        let archive = directory.path().join("secret.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: ArchiveFormat::Zip,
                password: Some(zeroize::Zeroizing::new("archive-pass".to_string())),
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_archive_with_auto_password(archive, ArchiveOpenIntent::Browse, window, cx);
                assert!(app.show_archive_password_prompt);
                assert!(app.archive_password_action.is_some());
            });
            window.draw(cx).clear(cx);
            assert!(window.find("archive-password").visible());
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码页的主操作按钮使用标准按钮高度。
    fn password_actions_use_standard_button_height(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.vault.unlock("master-pass").unwrap();
                app.screen = Screen::Passwords;
                cx.notify();
            });
            window.draw(cx).clear(cx);
            for id in ["password-add-new", "open-master-window"] {
                let button = window.find(id);
                assert!(button.visible(), "密码页按钮未渲染：{id}");
                assert!(
                    button.bounds().size.height >= px(30.),
                    "密码页按钮仍为小尺寸：{id} 高度 {:?}",
                    button.bounds().size.height
                );
            }
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码记录新增、修改和删除都直接写入持久化密码库。
    fn password_records_are_written_without_draft(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let (vault, _) =
            VaultCrypto::new_master(Vec::new(), VaultSettings::default(), "master-pass").unwrap();
        VaultCrypto::write_file(&vault_path, &vault).unwrap();
        let settings_path = directory.path().join("settings.json");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.vault.unlock("master-pass").unwrap();
                let id = app
                    .save_password_entry(
                        None,
                        "测试记录",
                        "first-secret".to_string(),
                        "*.zip",
                        "直接写入",
                    )
                    .unwrap();
                assert_eq!(app.vault.entry_count(), 1);
                assert_eq!(
                    app.vault.find_entry(id).unwrap().password.expose(),
                    "first-secret"
                );
                app.save_password_entry(
                    Some(id),
                    "测试记录 2",
                    "second-secret".to_string(),
                    "",
                    "修改后写入",
                )
                .unwrap();
                let saved = app.vault.find_entry(id).unwrap();
                assert_eq!(saved.label, "测试记录 2");
                assert_eq!(saved.password.expose(), "second-secret");
                assert_eq!(saved.archive_pattern, None);
                app.delete_password(id).unwrap();
                assert_eq!(app.vault.entry_count(), 0);
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证密码库可以迁移到自定义目录并持久化新位置。
    fn vault_can_move_to_custom_location(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let settings_path = directory.path().join("settings.json");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path.clone(),
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        let moved_directory = directory.path().join("moved");
        std::fs::create_dir(&moved_directory).unwrap();
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.vault
                    .add_entry(PasswordEntry::new("移动测试", "move-secret-123"))
                    .unwrap();
                app.move_vault_to(moved_directory.clone(), cx).unwrap();
                let target = moved_directory.join("vault.dat");
                assert_eq!(app.vault.path(), target);
                assert!(target.exists());
                assert!(!vault_path.exists());
                assert_eq!(app.settings.get().vault_path, Some(target.clone()));
                assert!(
                    app.vault
                        .entries()
                        .iter()
                        .any(|entry| { entry.password.expose() == "move-secret-123" })
                );
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证恢复默认设置会复位界面偏好，但保留自定义密码库位置。
    fn reset_preferences_keeps_vault_location(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.dat");
        let settings_path = directory.path().join("settings.json");
        let custom_vault = directory.path().join("custom-vault.dat");
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                vault_path,
                settings_path,
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings
                    .update(|settings| {
                        settings.accent_color = "#ff0000".to_string();
                        settings.details_visible = false;
                        settings.column_layout = 2;
                        settings.vault_path = Some(custom_vault.clone());
                    })
                    .unwrap();
                app.details_visible = false;
                app.column_layout = 2;
                app.reset_preferences(window, cx).unwrap();
                let settings = app.settings.get();
                assert_eq!(settings.accent_color, "#2563eb");
                assert!(settings.details_visible);
                assert_eq!(settings.column_layout, 1);
                assert_eq!(settings.vault_path, Some(custom_vault.clone()));
                assert!(app.details_visible);
                assert_eq!(app.column_layout, 1);
            });
        });
        let _ = app;
    }

    #[gpui_kit::test]
    /// 验证内置文本查看器可以渲染只读编辑器和状态行。
    fn text_viewer_renders_readonly_editor(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let (_view, cx) = cx.add_window_view(|window, cx| {
            crate::app::view::text_viewer_window::TextViewerWindowView::new(
                "第一行\n第二行\n".to_string(),
                "note.txt".to_string(),
                r"C:\downloads\note.txt".to_string(),
                18,
                window,
                cx,
            )
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    #[gpui_kit::test]
    /// 验证内置查看器能从普通文本文件读取内容并打开窗口。
    fn text_viewer_opens_for_text_file(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let file = directory.path().join("note.txt");
        std::fs::write(&file, "hello lumark\n").unwrap();
        let (app, cx) = cx.add_window_view(|window, cx| {
            LumarkApp::new_with_stores(
                window,
                cx,
                directory.path().join("vault.dat"),
                directory.path().join("settings.json"),
                Some(directory.path().to_path_buf()),
            )
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_text_viewer(file.clone(), "note.txt".to_string(), window, cx);
                assert!(
                    app.status.contains("已在内置查看器中打开"),
                    "{}",
                    app.status
                );
            });
        });
        let _ = app;
    }
}
