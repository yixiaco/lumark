use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::{StyledExt, h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, Theme, ThemeMode, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClipboardItem, Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, hsla, px,
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
    Settings,
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
    pending_requests: Vec<Vec<String>>,
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
    password_page_scroll: gpui_kit::ScrollHandle,
    password_list_scroll: gpui_kit::ScrollHandle,
    settings_page_scroll: gpui_kit::ScrollHandle,
    task_list_scroll: gpui_kit::ScrollHandle,
    theme_color_picker: Option<gpui_kit::Entity<ColorPickerState>>,
    theme_color_subscription: Option<Subscription>,
}

impl LumarkApp {
    /// 使用默认密码库和设置路径创建生产应用。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_stores(
            window,
            cx,
            VaultStore::default_path(),
            SettingsStore::default_path(),
            None,
        )
    }

    /// 使用自定义密码库路径创建应用，主要用于集成测试。
    pub fn new_with_paths(
        window: &mut Window,
        cx: &mut Context<Self>,
        vault_path: PathBuf,
        start_dir: Option<PathBuf>,
    ) -> Self {
        Self::new_with_stores(
            window,
            cx,
            vault_path,
            SettingsStore::default_path(),
            start_dir,
        )
    }

    /// 构建应用状态，应用持久化偏好，并启动后台任务轮询。
    pub fn new_with_stores(
        window: &mut Window,
        cx: &mut Context<Self>,
        vault_path: PathBuf,
        settings_path: PathBuf,
        start_dir: Option<PathBuf>,
    ) -> Self {
        let (settings, settings_error) = match SettingsStore::open(settings_path) {
            Ok(settings) => (Arc::new(settings), None),
            Err(error) => {
                let fallback = std::env::temp_dir()
                    .join("lumark-settings")
                    .join("settings.json");
                (
                    Arc::new(
                        SettingsStore::open(fallback).expect("temporary Lumark settings store"),
                    ),
                    Some(error.to_string()),
                )
            }
        };
        let settings_snapshot = settings.get();
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
            pending_requests: Vec::new(),
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
            password_page_scroll: gpui_kit::ScrollHandle::default(),
            password_list_scroll: gpui_kit::ScrollHandle::default(),
            settings_page_scroll: gpui_kit::ScrollHandle::default(),
            task_list_scroll: gpui_kit::ScrollHandle::default(),
            theme_color_picker: None,
            theme_color_subscription: None,
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
        let mut mode = "open";
        let mut startup_paths = Vec::new();
        for argument in std::env::args_os().skip(1) {
            let value = argument.to_string_lossy();
            match value.as_ref() {
                "--open" => mode = "open",
                "--extract" => mode = "extract",
                "--compress" => mode = "compress",
                value if value.starts_with('-') => {}
                _ => startup_paths.push(PathBuf::from(argument)),
            }
        }
        if mode == "compress" && !startup_paths.is_empty() {
            app.dropped_sources = Some(startup_paths);
            app.open_compress_dialog(window, cx);
        } else if mode == "extract"
            && let Some(path) = startup_paths.first().filter(|path| path.is_file())
        {
            app.open_archive_with_auto_password(
                path.clone(),
                ArchiveOpenIntent::Extract,
                window,
                cx,
            );
        } else if let Some(path) = startup_paths
            .first()
            .filter(|path| path.is_file() && is_archive_path(path))
        {
            app.open_archive_with_auto_password(
                path.clone(),
                ArchiveOpenIntent::Browse,
                window,
                cx,
            );
        }
        app.ensure_theme_color_picker(window, cx);
        app.start_task_polling(cx);
        app
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
                    let requests =
                        lumark_platform_windows::ipc::take_requests().unwrap_or_default();
                    let had_events = !events.is_empty() || !requests.is_empty();
                    app.pending_requests.extend(requests);
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
                                        let target = lumark_archive::safe_join(
                                            &pending.temporary,
                                            &pending.relative,
                                        )
                                        .unwrap_or_else(|_| pending.temporary.clone());
                                        app.open_extracted_target(target, &pending.temporary);
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

impl LumarkApp {
    /// 创建设置页的主题色选择器，并把用户选择实时写回设置和当前主题。
    ///
    /// 选择器只在启动时创建一次；它持有自己的输入和弹层状态，
    /// 不参与主界面渲染借用，因此切换颜色时不需要重建窗口。
    fn ensure_theme_color_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_color_picker.is_some() {
            return;
        }
        let accent_color = self.settings.get().accent_color;
        let picker = cx.new(|cx| {
            let mut state = ColorPickerState::new(window, cx);
            if let Ok(color) = gpui_kit::component::try_parse_color(&accent_color) {
                state.set_value(color, window, cx);
            }
            state
        });
        self.theme_color_subscription = Some(cx.subscribe(
            &picker,
            |this, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let hex = hsla_to_hex(*color);
                match this
                    .settings
                    .update(|settings| settings.accent_color = hex.clone())
                {
                    Ok(()) => {
                        apply_theme_accent(&hex, cx);
                        this.status = format!("主题色：{hex}");
                    }
                    Err(error) => this.status = error.to_string(),
                }
                cx.notify();
            },
        ));
        self.theme_color_picker = Some(picker);
    }
}

/// 启动 GPUI 应用，并在打开主窗口前处理 Shell 集成请求。
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
    let startup_settings = SettingsStore::open(SettingsStore::default_path())
        .map(|store| store.get())
        .unwrap_or_default();
    let window_options = gpui_kit::WindowOptions {
        window_min_size: Some(gpui_kit::size(px(960.), px(640.))),
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
            origin: gpui_kit::point(px(80.), px(60.)),
            size: gpui_kit::size(
                px(startup_settings.window_width.clamp(720., 4096.)),
                px(startup_settings.window_height.clamp(520., 2160.)),
            ),
        })),
        ..TitleBar::window_options()
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(window_options, cx, |window, cx| {
                cx.new(|cx| LumarkApp::new(window, cx))
            })
            .expect("open Lumark window");
        });
    Ok(())
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
}
