//! 伪装压缩、任意文件伪装和一键还原动作。
//!
//! 独立伪装窗口只收集源文件、输出目录和面具，提交后仍在主应用的统一任务
//! 池执行；一键还原由资源管理器菜单或右键菜单触发，隐藏宿主下用系统消息
//! 框报告无效输入，避免留下不可见的后台进程。

use super::super::*;

impl LumarkApp {
    /// 为当前选择打开独立伪装窗口。
    pub(crate) fn open_disguise_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sources = self.selected_sources();
        self.open_disguise_dialog_with_sources(sources, window, cx);
    }

    /// 用指定路径打开独立伪装窗口。
    ///
    /// 只接受普通文件；文件夹需要先压缩再伪装，因此提示用户改用
    /// “伪装压缩”入口，避免生成语义不清晰的输出。
    pub(crate) fn open_disguise_dialog_with_sources(
        &mut self,
        sources: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut seen = std::collections::HashSet::new();
        let files: Vec<PathBuf> = sources
            .into_iter()
            .filter(|path| path.is_file())
            .filter(|path| seen.insert(path.clone()))
            .collect();
        if files.is_empty() {
            self.status = "请选择要伪装的普通文件；文件夹请使用「伪装压缩」".to_string();
            cx.notify();
            return;
        }
        let settings = self.settings.get();
        let output_dir = settings
            .default_output_dir
            .filter(|path| path.is_dir())
            .or_else(|| {
                files
                    .first()
                    .and_then(|path| path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        set_input(
            &self.disguise_output,
            output_dir.to_string_lossy().as_ref(),
            window,
            cx,
        );
        self.status = format!("准备伪装 {} 个文件", files.len());
        self.open_disguise_window(files, window, cx);
    }

    /// 提交批量伪装任务，并打开自动进度窗口。
    pub(crate) fn start_disguise_from_options(
        &mut self,
        sources: Vec<PathBuf>,
        output_dir: PathBuf,
        mask: DisguiseMask,
        delete_source: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if sources.is_empty() {
            return Err("当前没有可伪装的文件".to_string());
        }
        if output_dir.as_os_str().is_empty() {
            return Err("请选择输出目录".to_string());
        }
        mask.validate().map_err(|error| error.to_string())?;
        let label = format!("伪装 {} 个文件", sources.len());
        let id = self.tasks.submit(TaskRequest::Disguise {
            sources,
            options: lumark_archive::DisguiseFileOptions {
                mask,
                output_dir,
                delete_source,
                cancel: None,
            },
        });
        self.record_task_queued(id, TaskKind::Disguise, label.clone());
        self.active_task = Some(id);
        self.active_task_kind = Some(TaskKind::Disguise);
        self.active_task_label = Some(label.clone());
        self.task_progress = None;
        self.status = label;
        self.open_auto_progress_window_for(id, window, cx);
        cx.notify();
        Ok(())
    }

    /// 文件夹右键“伪装压缩”：打开压缩窗口并默认勾选伪装输出。
    pub(crate) fn open_compress_dialog_for_disguise(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compress_disguise = true;
        self.open_compress_dialog(window, cx);
    }

    /// 还原当前选中的伪装文件。
    pub(crate) fn restore_selected_disguise(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.selected_entry().filter(|entry| !entry.is_dir) else {
            self.status = "请选择一个伪装文件".to_string();
            cx.notify();
            return;
        };
        self.start_restore_disguise(entry.path, window, cx);
    }

    /// 提交还原任务；无效输入按当前宿主形态提示。
    pub(crate) fn start_restore_disguise(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.submit_restore(&path, window, cx) {
            Ok(()) => {}
            Err(message) => self.report_shell_error(message, window, cx),
        }
    }

    /// 校验并提交一键还原任务。
    ///
    /// 还原目标固定为伪装文件所在目录，同名文件由任务层自动改名；校验在
    /// 提交前完成，避免无效请求占用工作线程。
    fn submit_restore(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !path.is_file() {
            return Err("请选择一个有效文件".to_string());
        }
        match ArchiveService::probe_disguise(path) {
            Ok(Some(info)) => {
                let output_dir = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                let label = format!("还原 {}", path.display());
                let id = self.tasks.submit(TaskRequest::Restore {
                    archive: path.to_path_buf(),
                    output_dir,
                });
                self.record_task_queued(id, TaskKind::Restore, label.clone());
                self.active_task = Some(id);
                self.active_task_kind = Some(TaskKind::Restore);
                self.active_task_label = Some(label.clone());
                self.task_progress = None;
                self.status = format!("正在还原 {}（{}）", path.display(), info.original_name);
                self.open_auto_progress_window_for(id, window, cx);
                cx.notify();
                Ok(())
            }
            Ok(None) => Err(format!("不是 Lumark 伪装文件：{}", path.display())),
            Err(error) => Err(error.to_string()),
        }
    }

    /// 隐藏宿主下用系统消息框提示并退出；可见窗口使用通知。
    pub(crate) fn report_shell_error(
        &mut self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.headless_shell_host {
            platform::show_message("Lumark", &message, true);
            cx.quit();
        } else {
            self.notify(window, cx, NotificationType::Error, message);
        }
    }
}
