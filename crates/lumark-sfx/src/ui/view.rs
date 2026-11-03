//! 自解压交互窗口的状态机：设置参数、启动解压、展示进度和处理结果。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, PathPromptOptions, Subscription, Window,
};
use lumark_sfx::{SfxError, SfxFooter, SfxPlan};
use lumark_ui_shared::i18n::tr;
use lumark_ui_shared::progress::{ProgressSnapshot, ProgressState};
use zeroize::Zeroizing;

use super::render;
use super::worker::{self, ExtractionEvent, ExtractionTask};

/// 自解压窗口当前阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// 正在收集解压目录和密码。
    Setup,
    /// 解压进行中。
    Extracting,
    /// 解压已完成，等待用户关闭或打开目录。
    Completed,
}

/// 自解压窗口的实体状态。
pub(crate) struct SfxView {
    pub(crate) exe: PathBuf,
    pub(crate) footer: SfxFooter,
    /// 当前是否允许覆盖同名文件。
    pub(crate) overwrite: bool,
    pub(crate) target: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
    pub(crate) password_visible: bool,
    pub(crate) stage: Stage,
    pub(crate) error: Option<String>,
    pub(crate) progress: ProgressSnapshot,
    pub(crate) progress_state: ProgressState,
    pub(crate) message: Option<String>,
    pub(crate) completed_target: Option<PathBuf>,
    pub(crate) started: Option<Instant>,
    running: Option<ExtractionTask>,
    close_after_cancel: bool,
    prompt_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl SfxView {
    /// 创建自解压窗口状态，并注册回车提交和关窗拦截。
    pub(crate) fn new(
        exe: PathBuf,
        footer: SfxFooter,
        plan: SfxPlan,
        default_target: PathBuf,
        initial_password: Option<Zeroizing<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let overwrite = plan.overwrite;
        let target_value = plan.target.clone().unwrap_or(default_target);
        let password_value = plan
            .password
            .clone()
            .or(initial_password)
            .map(|value| value.to_string())
            .unwrap_or_default();
        let target = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("解压目录", "Extraction folder"))
                .default_value(target_value.to_string_lossy().to_string())
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr("解压密码", "Extraction password"))
                .masked(true)
                .default_value(password_value)
        });
        let mut subscriptions = Vec::new();
        for input in [&target, &password] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) && this.stage == Stage::Setup
                    {
                        this.start_extraction(window, cx);
                    }
                }),
            );
        }
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |view, cx| view.should_close(window, cx))
                .unwrap_or(true)
        });
        let focus = target.read(cx).focus_handle(cx).clone();
        let view = Self {
            exe,
            footer,
            overwrite,
            target,
            password,
            password_visible: false,
            stage: Stage::Setup,
            error: None,
            progress: ProgressSnapshot::default(),
            progress_state: ProgressState::Running,
            message: None,
            completed_target: None,
            started: None,
            running: None,
            close_after_cancel: false,
            prompt_open: false,
            _subscriptions: subscriptions,
        };
        focus.focus(window, cx);
        view
    }

    /// 读取解压目录输入框的当前值。
    pub(crate) fn target_value(&self, cx: &App) -> String {
        self.target.read(cx).value().to_string()
    }

    /// 读取密码输入框的当前值。
    pub(crate) fn password_value(&self, cx: &App) -> String {
        self.password.read(cx).value().to_string()
    }

    /// 返回进度面板展示的目标路径。
    pub(crate) fn progress_target(&self, cx: &App) -> String {
        self.completed_target
            .as_ref()
            .map(|target| target.display().to_string())
            .unwrap_or_else(|| self.target_value(cx))
    }

    /// 返回可执行文件名，用于标题栏和信息行。
    pub(crate) fn exe_name(&self) -> String {
        self.exe
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Lumark.exe".to_string())
    }

    /// 打开系统目录选择器，并把选中的目录写回输入框。
    pub(crate) fn choose_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("选择解压目标目录", "Choose extraction folder").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(directory) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    view.target.update(cx, |state, cx| {
                        state.set_value(directory.to_string_lossy().as_ref(), window, cx);
                    });
                    view.error = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 校验表单并开始解压；目标目录非空时先确认覆盖。
    pub(crate) fn start_extraction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage != Stage::Setup || self.running.is_some() {
            return;
        }
        let target_text = self.target_value(cx).trim().to_string();
        if target_text.is_empty() {
            self.error = Some(tr("请选择解压目录", "Choose an extraction folder").to_string());
            self.focus_target(window, cx);
            cx.notify();
            return;
        }
        if self.footer.encrypted && self.password_value(cx).is_empty() {
            self.error = Some(tr("请输入解压密码", "Enter the extraction password").to_string());
            self.focus_password(window, cx);
            cx.notify();
            return;
        }
        self.error = None;
        let target = PathBuf::from(target_text);
        if !self.overwrite && target.exists() && !directory_is_empty(&target) {
            self.request_overwrite(target, window, cx);
            return;
        }
        self.begin_extraction(target, self.overwrite, window, cx);
    }

    /// 切换密码的明文/掩码显示。
    pub(crate) fn toggle_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.password_visible = !self.password_visible;
        let masked = !self.password_visible;
        self.password.update(cx, |state, cx| {
            state.set_masked(masked, window, cx);
        });
        cx.notify();
    }

    /// 切换解压阶段并启动后台线程。
    fn begin_extraction(
        &mut self,
        target: PathBuf,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let password = self
            .footer
            .encrypted
            .then(|| Zeroizing::new(self.password_value(cx)));
        match worker::spawn(
            self.exe.clone(),
            self.footer,
            target.clone(),
            password,
            overwrite,
        ) {
            Ok(task) => {
                self.running = Some(task);
                self.stage = Stage::Extracting;
                self.progress_state = ProgressState::Running;
                self.progress = ProgressSnapshot::default();
                self.message = None;
                self.error = None;
                self.completed_target = Some(target);
                self.started = Some(Instant::now());
                self.close_after_cancel = false;
                self.poll_events(window, cx);
                cx.notify();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    /// 启动解压事件轮询；实体关闭后自动结束。
    fn poll_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                let finished =
                    match this.update_in(cx, |view, window, cx| view.drain_events(window, cx)) {
                        Ok(finished) => finished,
                        Err(_) => break,
                    };
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    /// 处理一批解压事件；返回是否已经进入终态。
    pub(crate) fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(task) = &self.running {
            loop {
                match task.events.try_recv() {
                    Ok(event) => events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut finished = false;
        for event in events {
            match event {
                ExtractionEvent::Progress {
                    progress,
                    speed_bps,
                    elapsed_seconds,
                } => {
                    self.progress = ProgressSnapshot {
                        current: progress.current,
                        bytes_done: progress.bytes_done,
                        bytes_total: progress.bytes_total,
                        current_file_done: progress.current_file_done,
                        current_file_total: progress.current_file_total,
                        display_bytes: progress.bytes_done,
                        entries_done: progress.entries_done,
                        entries_total: progress.entries_total,
                        speed_bps,
                        elapsed_seconds,
                        ..Default::default()
                    };
                }
                ExtractionEvent::Completed { files } => {
                    self.running = None;
                    self.stage = Stage::Completed;
                    self.progress_state = ProgressState::Completed;
                    self.progress.display_bytes = self.progress.bytes_done;
                    if let Some(target) = &self.completed_target {
                        reveal(target);
                        self.message = Some(format!(
                            "{}{}（{}）",
                            tr("已解压到：", "Extracted to: "),
                            target.display(),
                            files
                        ));
                    }
                    finished = true;
                }
                ExtractionEvent::Cancelled => {
                    self.running = None;
                    if self.close_after_cancel {
                        window.remove_window();
                        return true;
                    }
                    self.stage = Stage::Setup;
                    self.progress_state = ProgressState::Cancelled;
                    self.error = Some(tr("已取消解压", "Extraction cancelled").to_string());
                    finished = true;
                }
                ExtractionEvent::Failed { error } => {
                    self.running = None;
                    finished = true;
                    match error {
                        SfxError::WrongPassword => {
                            self.stage = Stage::Setup;
                            self.progress_state = ProgressState::Cancelled;
                            self.error = Some(
                                tr("密码错误，请重新输入", "Wrong password; please try again")
                                    .to_string(),
                            );
                            self.password.update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                            self.focus_password(window, cx);
                        }
                        other => {
                            self.stage = Stage::Setup;
                            self.progress_state = ProgressState::Cancelled;
                            self.error = Some(localized_error(&other));
                        }
                    }
                }
            }
        }
        if disconnected && self.running.is_some() {
            self.running = None;
            self.stage = Stage::Setup;
            self.error = Some(
                tr(
                    "解压线程意外结束，请重试",
                    "Extraction stopped unexpectedly; please try again",
                )
                .to_string(),
            );
            finished = true;
        }
        if finished {
            cx.notify();
        }
        finished || self.running.is_none()
    }

    /// 请求覆盖确认；确认后带 overwrite=true 重新开始。
    fn request_overwrite(&mut self, target: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompt_open {
            return;
        }
        self.prompt_open = true;
        let weak = cx.entity().downgrade();
        let description = format!(
            "{}{}",
            tr("目标文件夹已有文件：", "The target folder is not empty: "),
            target.display()
        );
        window.open_alert_dialog(cx, move |alert, _, _| {
            let on_ok = weak.clone();
            let on_cancel = weak.clone();
            let on_ok_target = target.clone();
            alert
                .title(tr("覆盖同名文件？", "Overwrite existing files?"))
                .description(description.clone())
                .confirm()
                .ok_text(tr("覆盖并解压", "Overwrite and extract"))
                .cancel_text(tr("取消", "Cancel"))
                .on_ok(move |_, window, cx| {
                    let target = on_ok_target.clone();
                    let _ = on_ok.update(cx, |view, cx| {
                        view.prompt_open = false;
                        view.overwrite = true;
                        view.begin_extraction(target, true, window, cx);
                    });
                    true
                })
                .on_cancel(move |_, _, cx| {
                    let _ = on_cancel.update(cx, |view, _| view.prompt_open = false);
                    true
                })
        });
    }

    /// 请求停止解压；确认后置位取消标记。
    pub(crate) fn request_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_none() || self.prompt_open {
            return;
        }
        self.prompt_open = true;
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let on_ok = weak.clone();
            let on_cancel = weak.clone();
            alert
                .title(tr("停止解压？", "Stop extraction?"))
                .description(tr(
                    "已写入的文件会保留，未解压的文件不会创建。",
                    "Files already written are kept; remaining files are not created.",
                ))
                .confirm()
                .ok_text(tr("停止解压", "Stop extraction"))
                .cancel_text(tr("继续解压", "Keep extracting"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = on_ok.update(cx, |view, cx| {
                        view.prompt_open = false;
                        view.cancel_running(cx);
                    });
                    true
                })
                .on_cancel(move |_, _, cx| {
                    let _ = on_cancel.update(cx, |view, _| view.prompt_open = false);
                    true
                })
        });
    }

    /// 请求关闭窗口；解压中先确认是否停止。
    pub(crate) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_none() {
            window.remove_window();
            return;
        }
        if self.prompt_open {
            return;
        }
        self.prompt_open = true;
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let on_ok = weak.clone();
            let on_cancel = weak.clone();
            alert
                .title(tr("解压仍在进行", "Extraction is running"))
                .description(tr(
                    "关闭窗口会停止解压，已写入的文件会保留。",
                    "Closing stops extraction; files already written are kept.",
                ))
                .confirm()
                .ok_text(tr("停止并关闭", "Stop and close"))
                .cancel_text(tr("继续解压", "Keep extracting"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    let _ = on_ok.update(cx, |view, cx| {
                        view.prompt_open = false;
                        view.close_after_cancel = true;
                        view.cancel_running(cx);
                    });
                    true
                })
                .on_cancel(move |_, _, cx| {
                    let _ = on_cancel.update(cx, |view, _| view.prompt_open = false);
                    true
                })
        });
    }

    /// 窗口关闭钩子：解压中拦截并弹出确认。
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.running.is_none() {
            return true;
        }
        if !self.prompt_open {
            cx.defer_in(window, |view, window, cx| view.request_close(window, cx));
        }
        false
    }

    /// 置位取消标记并把进度面板切换到取消中。
    fn cancel_running(&mut self, cx: &mut Context<Self>) {
        if let Some(task) = &self.running {
            task.cancel.store(true, Ordering::SeqCst);
        }
        self.progress_state = ProgressState::Cancelling;
        cx.notify();
    }

    /// 把焦点交给解压目录输入框。
    fn focus_target(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.target.read(cx).focus_handle(cx).clone();
        handle.focus(window, cx);
    }

    /// 把焦点交给密码输入框。
    fn focus_password(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.password.read(cx).focus_handle(cx).clone();
        handle.focus(window, cx);
    }

    /// 打开已完成解压的目标目录。
    pub(crate) fn open_folder(&self) {
        if let Some(target) = &self.completed_target {
            let _ = std::process::Command::new("explorer.exe")
                .arg(target)
                .spawn();
        }
    }
}

impl gpui_kit::Render for SfxView {
    /// 渲染标题栏、品牌横幅、当前阶段内容和底部操作。
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        render::render(self, window, cx)
    }
}

/// 在资源管理器中打开解压目录。
fn reveal(target: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(target)
        .spawn();
}

/// 判断目录是否已有内容；读取失败时按非空处理，避免静默覆盖。
fn directory_is_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

/// 把解压错误转换为当前界面语言的文本。
fn localized_error(error: &SfxError) -> String {
    match error {
        SfxError::PasswordRequired => {
            tr("该自解压包需要密码", "This package requires a password").to_string()
        }
        SfxError::WrongPassword => {
            tr("密码错误，请重新输入", "Wrong password; please try again").to_string()
        }
        SfxError::Cancelled => tr("已取消解压", "Extraction cancelled").to_string(),
        SfxError::Other(message) => message.clone(),
    }
}
