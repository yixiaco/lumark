use super::super::*;

impl LumarkApp {
    /// 先把一个归档条目解压到受控临时目录，再交给系统默认应用打开。
    pub(crate) fn open_archive_entry(
        &mut self,
        entry: BrowserEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_archive_entry_as(entry, PendingOpenMode::External, window, cx);
    }

    /// 用一个内置只读查看器打开归档条目。
    ///
    /// 加密且尚未拿到密码的压缩包会在打开前弹出密码输入框，
    /// 密码通过后再解压到受控临时目录交给查看器。
    pub(crate) fn view_archive_entry(
        &mut self,
        entry: BrowserEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_archive_entry_as(entry, PendingOpenMode::Viewer, window, cx);
    }

    /// 按指定处理方式解压归档条目并记录后续动作。
    fn open_archive_entry_as(
        &mut self,
        entry: BrowserEntry,
        mode: PendingOpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BrowserLocation::Archive { archive, .. } = &self.browser.location else {
            return;
        };
        let archive = archive.clone();
        let relative = entry.path.to_string_lossy().replace('\\', "/");
        let password = if self.browser.archive_encrypted() {
            if let Some(password) = &self.archive_password {
                Some(zeroize::Zeroizing::new(password.to_string()))
            } else if self.begin_vault_password_resolution(
                &archive,
                PendingPasswordAction::OpenEntry {
                    archive: archive.clone(),
                    relative: relative.clone(),
                    mode,
                },
                cx,
            ) {
                return;
            } else if self.vault.state() == VaultState::Locked {
                self.pending_unlock_archive = Some(archive);
                self.show_unlock_for_extract = true;
                self.unlock_for_browse = true;
                self.status = "压缩包已加密，解锁密码库后将尝试记录密码".to_string();
                return;
            } else {
                let action = PendingPasswordAction::OpenEntry {
                    archive: archive.clone(),
                    relative,
                    mode,
                };
                self.request_archive_password(archive, action, window, cx);
                return;
            }
        } else {
            None
        };
        self.open_archive_entry_with_password(archive, relative, password, mode, window, cx);
    }

    /// 使用已解析的密码解压一个条目，并记录解压完成后的处理方式。
    pub(crate) fn open_archive_entry_with_password(
        &mut self,
        archive: PathBuf,
        relative: String,
        password: Option<zeroize::Zeroizing<String>>,
        mode: PendingOpenMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_open_entry.is_some() {
            self.status = "请等待当前归档条目准备完成".to_string();
            return;
        }
        let temporary = std::env::temp_dir()
            .join("Lumark")
            .join("open")
            .join(Uuid::new_v4().to_string());
        let options = ExtractionOptions {
            output_dir: temporary.clone(),
            selected_paths: vec![relative.clone()],
            overwrite: true,
            password,
            ..Default::default()
        };
        let id = self.tasks.submit(TaskRequest::Extract { archive, options });
        self.pending_open_entry = Some(PendingOpenEntry {
            task_id: id,
            temporary,
            relative,
            mode,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = "正在准备打开归档条目…".to_string();
        let _ = window;
        let _ = cx;
    }

    /// 打开临时解压结果，并在适当时候安排清理。
    pub(crate) fn open_extracted_target(&mut self, target: PathBuf, temporary: &Path) {
        if is_executable_like(&target) {
            self.pending_open_target = Some(target);
            self.show_open_executable_confirm = true;
            self.status = format!(
                "归档条目已解压到临时目录：{}；打开可执行文件前需要确认",
                temporary.display()
            );
            return;
        }
        let _ = std::process::Command::new("explorer.exe")
            .arg(&target)
            .spawn();
        self.status = format!("已解压到临时目录：{}", temporary.display());
    }

    /// 执行此前已确认的归档可执行文件或脚本。
    pub(crate) fn confirm_open_executable(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if let Some(target) = self.pending_open_target.take() {
            let _ = std::process::Command::new("explorer.exe")
                .arg(target)
                .spawn();
            self.status = "已按用户确认打开归档中的可执行文件".to_string();
        }
        self.show_open_executable_confirm = false;
    }

    /// 为所选源生成默认输出名，并重置压缩字段。
    pub(crate) fn open_compress_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sources = self.selected_sources();
        self.open_compress_dialog_with_sources(sources, window, cx);
    }

    /// 用指定路径打开轻巧压缩窗口。
    ///
    /// Shell 多选压缩和普通压缩共用同一套表单，但 Shell 场景不能依赖
    /// 浏览器当前选区，因此显式传入全部路径。
    pub(crate) fn open_compress_dialog_with_sources(
        &mut self,
        mut sources: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut seen = std::collections::HashSet::new();
        sources.retain(|path| path.exists() && seen.insert(path.clone()));
        let Some(first) = sources.first() else {
            self.status = "当前没有可压缩的项目".to_string();
            return;
        };
        let settings = self.settings.get();
        let format = if self.compress_format == ArchiveFormat::Unknown {
            format_from_settings(&settings)
        } else {
            self.compress_format
        };
        self.compress_format = format;
        self.compress_preset = settings.default_preset;
        let default_name = default_archive_name(&sources);
        let output_dir = settings
            .default_output_dir
            .filter(|path| path.is_dir())
            .or_else(|| first.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        // 同名旧压缩包不再自动追加 (1)：创建流程先写临时文件，成功后再
        // 原子替换目标路径，失败时旧归档保持不变。
        let mut output = output_dir.join(format!("{default_name}.{}", format.extension()));
        if self.compress_sfx {
            output.set_extension("exe");
        }
        set_input(
            &self.compress_output,
            output.to_string_lossy().as_ref(),
            window,
            cx,
        );
        set_input(&self.compress_password, "", window, cx);
        set_input(&self.compress_comment, "", window, cx);
        set_input(&self.compress_exclude, "", window, cx);
        set_input(&self.compress_store, "", window, cx);
        set_input(&self.compress_split, "", window, cx);
        set_input(&self.compress_dictionary, "", window, cx);
        set_input(
            &self.compress_threads,
            &default_compress_threads().to_string(),
            window,
            cx,
        );
        set_input(&self.compress_after, "", window, cx);
        set_input(&self.compress_before, "", window, cx);
        self.compress_tab = 0;
        self.compress_test_after = false;
        self.compress_delete_source = false;
        self.compress_zipcrypto = false;
        self.compress_password_visible = false;
        self.show_compress = false;
        self.status = format!("准备压缩 {} 个项目", sources.len());
        self.open_compress_window_for_sources(sources, window, cx);
    }

    /// 切换压缩格式，并把输出输入框的扩展名同步为新格式。
    ///
    /// 输出路径为空时只记录格式，避免生成以 "." 开头的怪文件名。
    #[allow(dead_code)]
    pub(crate) fn set_compress_format(
        &mut self,
        format: ArchiveFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compress_format = format;
        let current = PathBuf::from(input_value(&self.compress_output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = replace_archive_extension(&current, format);
            set_input(
                &self.compress_output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
    }

    /// 打开系统目录选择器，把压缩包输出位置切换到用户选中的目录。
    ///
    /// 保留原文件名，只替换父目录；用户取消选择时输入框保持不变。
    #[allow(dead_code)]
    pub(crate) fn choose_compress_output_dir(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = PathBuf::from(input_value(&self.compress_output, cx));
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择压缩包输出目录".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(directory) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, window, cx| {
                    let name = current
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("archive.zip");
                    let target = directory.join(name);
                    set_input(
                        &app.compress_output,
                        target.to_string_lossy().as_ref(),
                        window,
                        cx,
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 提交独立压缩窗口构建好的任务参数。
    ///
    /// 对话框窗口只负责收集参数；任务和进度窗口仍由主应用统一管理，
    /// 因此关闭对话框不会影响已经提交的后台任务。
    pub(crate) fn start_compress_from_options(
        &mut self,
        output: PathBuf,
        sources: Vec<PathBuf>,
        options: CompressionOptions,
        task_options: CompressTaskOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            self.status = "当前没有可压缩的项目".to_string();
            return;
        }
        // 记住勾选状态，下次打开压缩窗口时沿用。
        self.compress_sfx = options.sfx;
        // 自解压运行时模块随安装包分发；开发环境可用环境变量覆盖路径。
        let sfx_stub = if options.sfx { sfx_stub_path() } else { None };
        if options.sfx && sfx_stub.is_none() {
            self.status =
                "找不到自解压运行时模块 lumark-sfx.exe，请先构建 lumark-sfx 或重新安装 Lumark"
                    .to_string();
            return;
        }
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Compress {
            output: output.clone(),
            sources,
            options,
            sfx_stub,
        });
        if task_options.shutdown_after {
            self.pending_shutdown_task = Some(id);
        }
        // 先降低优先级再提交任务，压缩线程创建时就继承较低优先级；失败只
        // 在状态栏提示，不影响任务本身。
        let mut priority_error = None;
        if task_options.low_priority {
            self.low_priority_tasks.insert(id);
            if let Err(error) = platform::process::set_low_priority(true) {
                self.low_priority_tasks.remove(&id);
                priority_error = Some(error);
            }
        }
        let label = format!("压缩 {}", output.display());
        self.record_task_queued(id, TaskKind::Compress, label.clone());
        self.active_task = Some(id);
        self.active_task_kind = Some(TaskKind::Compress);
        self.active_task_label = Some(label);
        self.task_progress = None;
        self.show_compress = false;
        self.status = match priority_error {
            Some(error) => format!("开始压缩：{}（降低优先级失败：{error}）", output.display()),
            None => format!("开始压缩：{}", output.display()),
        };
        // 先创建进度窗口，避免短任务在首次轮询前就结束而只看到 100%。
        self.open_auto_progress_window_for(id, window, cx);
        cx.notify();
    }

    /// 结束低优先级任务；多个低优先级任务并存时等最后一个结束再恢复。
    ///
    /// 优先级是进程级状态，单个任务结束就恢复会让仍在运行的任务失去限制，
    /// 因此按任务集合计数，只有集合清空才回到普通优先级。
    pub(crate) fn finish_low_priority(&mut self, task_id: u64) {
        if !self.low_priority_tasks.remove(&task_id) {
            return;
        }
        if !self.low_priority_tasks.is_empty() {
            return;
        }
        if let Err(error) = platform::process::set_low_priority(false) {
            self.status = format!("{}；恢复进程优先级失败：{error}", self.status);
        }
    }

    /// 校验压缩输入并提交后台压缩任务。
    #[allow(dead_code)]
    pub(crate) fn start_compress(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let output = PathBuf::from(input_value(&self.compress_output, cx));
        if output.as_os_str().is_empty() {
            self.status = "请输入输出路径".to_string();
            return;
        }
        let sources = self.selected_sources();
        if sources.is_empty() {
            self.status = "当前没有可压缩的项目".to_string();
            return;
        }
        let password = input_value(&self.compress_password, cx);
        let comment = input_value(&self.compress_comment, cx);
        let exclude = input_value(&self.compress_exclude, cx);
        let store_only = input_value(&self.compress_store, cx);
        let split_size = parse_size_in_unit(
            &input_value(&self.compress_split, cx),
            self.compress_split_unit,
        );
        let dictionary_size = parse_size(&input_value(&self.compress_dictionary, cx))
            .map(|value| value.min(u32::MAX as u64) as u32);
        let threads = input_value(&self.compress_threads, cx)
            .trim()
            .parse::<u32>()
            .unwrap_or_else(|_| default_compress_threads())
            .clamp(1, 64);
        let modified_after = parse_date(&input_value(&self.compress_after, cx));
        let modified_before = parse_date(&input_value(&self.compress_before, cx));
        let options = CompressionOptions {
            format: self.compress_format,
            level: Some(level_for_preset(self.compress_preset)),
            preset: archive_preset(self.compress_preset),
            solid: self.compress_solid,
            encrypt_header: self.compress_encrypt_header,
            comment: (!comment.trim().is_empty()).then_some(comment),
            split_size,
            dictionary_size,
            threads,
            test_after: self.compress_test_after,
            delete_source: self.compress_delete_source,
            modified_after,
            modified_before,
            exclude_patterns: parse_patterns(&exclude),
            store_only_patterns: parse_patterns(&store_only),
            path_mode: self.compress_path_mode,
            password: (!password.is_empty()).then(|| zeroize::Zeroizing::new(password)),
            zip_encryption: if self.compress_zipcrypto {
                ZipEncryption::ZipCrypto
            } else {
                ZipEncryption::Aes256
            },
            zpaq_compression: ZpaqCompression::from_preset(archive_preset(self.compress_preset)),
            ..Default::default()
        };
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Compress {
            output: output.clone(),
            sources,
            options,
            sfx_stub: None,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.show_compress = false;
        self.status = format!("开始压缩：{}", output.display());
        let _ = window;
        let _ = cx;
    }

    /// 显示独立的压缩包密码输入弹窗，并暂存密码通过后的目标动作。
    pub(crate) fn request_archive_password(
        &mut self,
        archive: PathBuf,
        action: PendingPasswordAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_archive_password_prompt = true;
        self.archive_password_action = Some(action);
        self.archive_password_visible = false;
        self.archive_password_use_vault =
            self.vault.state() == VaultState::Unlocked && self.vault.settings().auto_extract;
        // 保存引导只在自动尝试失败后出现，普通密码输入保持不写入密码库。
        self.archive_password_after_failure = false;
        self.archive_password_save_to_vault = false;
        set_input(&self.archive_password_input, "", window, cx);
        self.archive_password_input
            .update(cx, |state, cx| state.set_masked(true, window, cx));
        self.dismiss_extract_window(cx);
        self.status = format!("{} 已加密，请输入密码", archive.display());
        cx.notify();
    }

    /// 读取密码弹窗并继续之前的打开、解压或测试动作。
    pub(crate) fn confirm_archive_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.archive_password_action.clone() else {
            self.show_archive_password_prompt = false;
            return;
        };
        let password = input_value(&self.archive_password_input, cx);
        if password.is_empty() {
            if self.archive_password_use_vault
                && self.vault.state() == VaultState::Unlocked
                && self.vault.settings().auto_extract
            {
                let archive = password_action_archive(&action).to_path_buf();
                if self.begin_vault_password_resolution(&archive, action, cx) {
                    self.show_archive_password_prompt = false;
                    self.archive_password_action = None;
                } else {
                    self.status = "密码库中没有匹配密码，请手动输入".to_string();
                }
            } else {
                self.status = "请输入压缩包密码".to_string();
            }
            cx.notify();
            return;
        }

        self.show_archive_password_prompt = false;
        self.archive_password_action = None;
        self.archive_password_after_failure = false;
        // 勾选保存时先在工作线程校验密码：命中后才写入密码库并继续原动作，
        // 避免把输错的密码持久化到密码库。
        let save_to_vault = std::mem::take(&mut self.archive_password_save_to_vault);
        if save_to_vault && self.vault.state() == VaultState::Unlocked {
            let archive = password_action_archive(&action).to_path_buf();
            self.submit_manual_password_verification(
                archive,
                action,
                zeroize::Zeroizing::new(password),
            );
            cx.notify();
            return;
        }
        match action {
            PendingPasswordAction::OpenArchive { archive, intent } => {
                self.begin_archive_load(archive, Some(zeroize::Zeroizing::new(password)), intent);
            }
            PendingPasswordAction::ExtractDialog { archive } => {
                self.prepare_extract_dialog(archive, false, Some(&password), window, cx);
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                self.open_archive_entry_with_password(
                    archive,
                    relative,
                    Some(zeroize::Zeroizing::new(password)),
                    mode,
                    window,
                    cx,
                );
            }
            PendingPasswordAction::Test { archive } => {
                self.submit_archive_test(archive, Some(password));
            }
        }
        cx.notify();
    }

    /// 为所选归档打开解压流程，并在提示前探测加密状态。
    pub(crate) fn open_extract_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(archive) = self.selected_archive() else {
            self.status = "请选择一个压缩包".to_string();
            return;
        };
        let password = self
            .archive_password
            .as_ref()
            .map(|value| value.to_string());
        self.prepare_extract_dialog(archive, password.is_none(), password.as_deref(), window, cx);
    }

    /// 在密码解析后准备解压目标目录和选择状态。
    ///
    /// 窗口先立即打开，加密探测放到后台线程，避免打开大压缩包时卡住界面；
    /// 探测结果再决定是否自动尝试密码库记录或提示手动输入。
    pub(crate) fn prepare_extract_dialog(
        &mut self,
        archive: PathBuf,
        auto_resolve: bool,
        password: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let output = archive
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        set_input(
            &self.extract_output,
            output.to_string_lossy().as_ref(),
            window,
            cx,
        );
        set_input(
            &self.extract_password,
            password.unwrap_or_default(),
            window,
            cx,
        );
        self.extract_password
            .update(cx, |state, cx| state.set_masked(true, window, cx));
        self.open_extract_window(archive.clone(), window, cx);
        if password.is_some() {
            self.archive_password = Some(zeroize::Zeroizing::new(
                password.unwrap_or_default().to_string(),
            ));
            self.status = "已应用可用密码，正在准备解压".to_string();
            return;
        }
        if !auto_resolve {
            self.status = format!("准备解压：{}（加密包请输入密码）", archive.display());
            return;
        }
        self.status = format!("正在检查压缩包：{}", archive.display());
        let probe_archive = archive.clone();
        cx.spawn(async move |this, cx| {
            let encryption = cx
                .background_executor()
                .spawn(async move { ArchiveService::probe_encryption(&probe_archive) })
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.apply_extract_probe(archive, encryption, window, cx);
            });
        })
        .detach();
    }

    /// 应用后台加密探测结果，并继续密码库自动匹配流程。
    fn apply_extract_probe(
        &mut self,
        archive: PathBuf,
        encryption: Result<lumark_archive::EncryptionInfo, lumark_archive::ArchiveError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match encryption {
            Ok(info) if info.encrypted => {
                if !self.vault.settings().auto_extract {
                    let action = PendingPasswordAction::OpenArchive {
                        archive: archive.clone(),
                        intent: ArchiveOpenIntent::Extract,
                    };
                    self.request_archive_password(archive, action, window, cx);
                } else {
                    match self.vault.state() {
                        VaultState::Unlocked => {
                            if !self.begin_vault_password_resolution(
                                &archive,
                                PendingPasswordAction::ExtractDialog {
                                    archive: archive.clone(),
                                },
                                cx,
                            ) {
                                let action = PendingPasswordAction::OpenArchive {
                                    archive: archive.clone(),
                                    intent: ArchiveOpenIntent::Extract,
                                };
                                self.request_archive_password(archive, action, window, cx);
                            }
                        }
                        VaultState::Locked => {
                            self.dismiss_extract_window(cx);
                            self.pending_unlock_archive = Some(archive);
                            self.show_unlock_for_extract = true;
                            self.unlock_for_browse = false;
                            self.status = "密码库已锁定，解锁后将自动尝试记录密码".to_string();
                        }
                        VaultState::Uninitialized => {
                            let action = PendingPasswordAction::OpenArchive {
                                archive: archive.clone(),
                                intent: ArchiveOpenIntent::Extract,
                            };
                            self.request_archive_password(archive, action, window, cx);
                        }
                    }
                }
            }
            Ok(_) => {
                self.status = format!("准备解压：{}", archive.display());
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
        let _ = window;
        cx.notify();
    }

    /// 启动基于密码库的密码匹配，或在需要时先请求解锁。
    pub(crate) fn begin_vault_password_resolution(
        &mut self,
        archive: &Path,
        action: PendingPasswordAction,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.vault.state() != VaultState::Unlocked || !self.vault.settings().auto_extract {
            return false;
        }
        let fingerprint = lumark_vault::PasswordResolver::archive_fingerprint(archive).ok();
        let candidates = self.vault.candidates(archive, fingerprint.as_deref());
        let mut candidates = candidates.into_iter();
        let primary: Vec<_> = candidates.by_ref().take(32).collect();
        let extras: Vec<_> = candidates.collect();
        self.submit_password_candidates(archive, action, primary, extras, false, cx)
    }

    /// 在工作线程中校验有序候选密码，不写入输出。
    ///
    /// `save_on_success` 只用于手动输入密码的校验：命中后先把密码写入密码库，
    /// 再继续原动作；自动尝试沿用已有候选记录，不会重复写入。
    pub(crate) fn submit_password_candidates(
        &mut self,
        archive: &Path,
        action: PendingPasswordAction,
        candidates: Vec<lumark_vault::PasswordCandidate>,
        extras: Vec<lumark_vault::PasswordCandidate>,
        save_on_success: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if candidates.is_empty() {
            return false;
        }
        self.vault.acquire_lease();
        let entry_ids = candidates
            .iter()
            .map(|candidate| candidate.entry_id)
            .collect();
        let total = candidates.len();
        let passwords = candidates
            .into_iter()
            .map(|candidate| zeroize::Zeroizing::new(candidate.password.expose().to_string()))
            .collect();
        let load_info = matches!(&action, PendingPasswordAction::OpenArchive { .. });
        let id = self.tasks.submit(TaskRequest::ResolvePassword {
            archive: archive.to_path_buf(),
            candidates: passwords,
            load_info,
        });
        self.pending_password_jobs.insert(
            id,
            PendingPasswordJob {
                action,
                entry_ids,
                extras,
                save_on_success,
            },
        );
        if save_on_success {
            self.status = "正在验证密码，成功后会保存到密码库…".to_string();
            return true;
        }
        self.status = "正在验证记录密码…".to_string();
        self.track_password_attempt(cx, id, archive, total);
        true
    }

    /// 校验手动输入的密码，命中后先写入密码库再继续原动作。
    ///
    /// 单独走一次单候选校验，是为了在保存前确认密码正确；否则输错一次就会
    /// 在密码库里留下一条永远解不开压缩包的记录。
    pub(crate) fn submit_manual_password_verification(
        &mut self,
        archive: PathBuf,
        action: PendingPasswordAction,
        password: zeroize::Zeroizing<String>,
    ) {
        self.vault.acquire_lease();
        let id = self.tasks.submit(TaskRequest::ResolvePassword {
            archive,
            candidates: vec![password],
            load_info: false,
        });
        self.pending_password_jobs.insert(
            id,
            PendingPasswordJob {
                action,
                entry_ids: Vec::new(),
                extras: Vec::new(),
                save_on_success: true,
            },
        );
        self.status = "正在验证密码，成功后会保存到密码库…".to_string();
    }

    /// 记录多候选校验的进度，并安排一次延迟重绘。
    ///
    /// 只有多候选校验才值得展示停止入口；延迟重绘让弹窗在验证变慢时出现，
    /// 快速命中时用户看不到任何闪烁。
    fn track_password_attempt(
        &mut self,
        cx: &mut Context<Self>,
        task_id: u64,
        archive: &Path,
        total: usize,
    ) {
        if total < 2 {
            return;
        }
        self.password_attempt = Some(PasswordAttemptProgress {
            task_id,
            archive: archive.to_path_buf(),
            index: 0,
            total,
            started: std::time::Instant::now(),
            hidden: false,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(420))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app
                    .password_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.task_id == task_id)
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 停止当前记录密码校验，取消事件到达后回退到手动输入。
    pub(crate) fn cancel_password_attempt(&mut self, cx: &mut Context<Self>) {
        let Some(attempt) = self.password_attempt.take() else {
            return;
        };
        match self.tasks.control(attempt.task_id) {
            Some(control) => {
                control.cancel();
                self.status = format!(
                    "正在停止密码验证，随后请手动输入：{}",
                    attempt.archive.display()
                );
            }
            None => self.status = "密码验证已经结束".to_string(),
        }
        cx.notify();
    }

    /// 清除属于指定任务的校验进度。
    ///
    /// 事件可能晚于用户点击停止到达，按任务 ID 比对可以避免清掉新任务的进度。
    pub(crate) fn clear_password_attempt(&mut self, task_id: u64) {
        if self
            .password_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.task_id == task_id)
        {
            self.password_attempt = None;
        }
    }

    /// 应用密码解析结果：保存勾选的密码、记录使用次数并继续原动作。
    pub(crate) fn finish_password_resolution(
        &mut self,
        job: PendingPasswordJob,
        resolved: lumark_tasks::ResolvedPassword,
        archive_info: Option<lumark_archive::ArchiveInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry_id) = job.entry_ids.get(resolved.candidate_index) {
            let _ = self.vault.mark_used(*entry_id);
        }
        let vault_locked = job.save_on_success && self.vault.state() != VaultState::Unlocked;
        let saved = job.save_on_success
            && self.remember_verified_password(
                password_action_archive(&job.action),
                resolved.password.as_str(),
            );
        self.apply_resolved_password(job.action, resolved.password, archive_info, window, cx);
        if saved {
            self.status = format!("{}（密码已保存到密码库）", self.status);
        } else if vault_locked {
            self.status = format!("{}（密码库已锁定，未保存密码）", self.status);
        }
    }

    /// 把刚验证通过的密码写入密码库，重复记录保持原状。
    pub(crate) fn remember_verified_password(&mut self, archive: &Path, password: &str) -> bool {
        if self.vault.state() != VaultState::Unlocked {
            return false;
        }
        if self
            .vault
            .entries()
            .iter()
            .any(|entry| entry.password.expose() == password)
        {
            return false;
        }
        let label = archive
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("压缩包密码")
            .to_string();
        let pattern = archive.to_string_lossy();
        self.save_password_entry(None, &label, password.to_string(), &pattern, "")
            .is_ok()
    }

    /// 继续尝试超出首次自动尝试上限的候选密码。
    pub(crate) fn try_extra_password_candidates(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(extra) = self.pending_extra_password.take() else {
            self.show_extra_password_prompt = false;
            return;
        };
        self.show_extra_password_prompt = false;
        let archive = match &extra.action {
            PendingPasswordAction::OpenArchive { archive, .. }
            | PendingPasswordAction::ExtractDialog { archive }
            | PendingPasswordAction::OpenEntry { archive, .. }
            | PendingPasswordAction::Test { archive } => archive.clone(),
        };
        if self.vault.state() != VaultState::Unlocked {
            self.apply_password_failure(extra.action, window, cx);
            self.status = "密码库已锁定，请先解锁".to_string();
            return;
        }
        if !self.submit_password_candidates(
            &archive,
            extra.action,
            extra.candidates,
            Vec::new(),
            false,
            cx,
        ) {
            self.status = "没有剩余记录密码".to_string();
        }
    }

    /// 停止自动尝试并回退到手动密码输入。
    pub(crate) fn decline_extra_password_candidates(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_extra_password_prompt = false;
        if let Some(extra) = self.pending_extra_password.take() {
            self.apply_password_failure(extra.action, window, cx);
            self.status = "已停止自动尝试，请手动输入密码".to_string();
        }
    }

    /// 把成功解析的密码分发给待处理的归档操作。
    pub(crate) fn apply_resolved_password(
        &mut self,
        action: PendingPasswordAction,
        password: zeroize::Zeroizing<String>,
        archive_info: Option<lumark_archive::ArchiveInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingPasswordAction::OpenArchive { archive, intent } => {
                self.open_archive_now(archive, Some(password), intent, archive_info, window, cx);
            }
            PendingPasswordAction::ExtractDialog { archive } => {
                self.prepare_extract_dialog(archive, false, Some(password.as_str()), window, cx);
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                self.open_archive_entry_with_password(
                    archive,
                    relative,
                    Some(password),
                    mode,
                    window,
                    cx,
                );
            }
            PendingPasswordAction::Test { archive } => {
                self.submit_archive_test(archive, Some(password.to_string()));
            }
        }
    }

    /// 所有记录密码失败后回退到手动输入。
    pub(crate) fn apply_password_failure(
        &mut self,
        action: PendingPasswordAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingPasswordAction::ExtractDialog { archive } => {
                let prompt_action = PendingPasswordAction::OpenArchive {
                    archive: archive.clone(),
                    intent: ArchiveOpenIntent::Extract,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = "记录密码均不正确，请手动输入密码".to_string();
            }
            PendingPasswordAction::OpenArchive { archive, intent } => {
                let prompt_action = PendingPasswordAction::OpenArchive {
                    archive: archive.clone(),
                    intent,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = "记录密码均不正确，请手动输入密码".to_string();
            }
            PendingPasswordAction::OpenEntry {
                archive,
                relative,
                mode,
            } => {
                let prompt_action = PendingPasswordAction::OpenEntry {
                    archive: archive.clone(),
                    relative,
                    mode,
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = "记录密码均不正确，请手动输入密码".to_string();
            }
            PendingPasswordAction::Test { archive } => {
                let prompt_action = PendingPasswordAction::Test {
                    archive: archive.clone(),
                };
                self.request_archive_password(archive, prompt_action, window, cx);
                self.status = "记录密码均不正确，请手动输入密码".to_string();
            }
        }
        // 自动尝试失败后引导用户把本次密码存进密码库：默认勾选，输入后先
        // 校验再写入，避免保存错误密码。
        self.archive_password_after_failure = true;
        self.archive_password_save_to_vault = self.vault.state() == VaultState::Unlocked;
    }

    /// 探测加密并匹配记录密码后打开或解压归档。
    pub(crate) fn open_archive_with_auto_password(
        &mut self,
        archive: PathBuf,
        intent: ArchiveOpenIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ArchiveService::probe_encryption(&archive) {
            Ok(info) if info.encrypted => {
                if self.vault.settings().auto_extract {
                    match self.vault.state() {
                        VaultState::Unlocked => {
                            if self.begin_vault_password_resolution(
                                &archive,
                                PendingPasswordAction::OpenArchive {
                                    archive: archive.clone(),
                                    intent,
                                },
                                cx,
                            ) {
                                return;
                            }
                        }
                        VaultState::Locked => {
                            self.pending_unlock_archive = Some(archive);
                            self.show_unlock_for_extract = true;
                            self.unlock_for_browse = intent == ArchiveOpenIntent::Browse;
                            self.status = "压缩包已加密，解锁密码库后将尝试记录密码".to_string();
                            return;
                        }
                        VaultState::Uninitialized => {}
                    }
                }
                if intent == ArchiveOpenIntent::Extract {
                    let action = PendingPasswordAction::OpenArchive {
                        archive: archive.clone(),
                        intent: ArchiveOpenIntent::Extract,
                    };
                    self.request_archive_password(archive, action, window, cx);
                } else {
                    let action = PendingPasswordAction::OpenArchive {
                        archive: archive.clone(),
                        intent: ArchiveOpenIntent::Browse,
                    };
                    self.request_archive_password(archive, action, window, cx);
                }
            }
            Ok(_) => {
                self.begin_archive_load(archive, None, intent);
                let _ = window;
                let _ = cx;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// 在后台工作线程中排队枚举归档。
    pub(crate) fn begin_archive_load(
        &mut self,
        archive: PathBuf,
        password: Option<zeroize::Zeroizing<String>>,
        intent: ArchiveOpenIntent,
    ) {
        let id = self.tasks.submit(TaskRequest::LoadArchive {
            archive: archive.clone(),
            password,
        });
        self.pending_archive_loads
            .insert(id, PendingPasswordAction::OpenArchive { archive, intent });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = "正在读取归档目录…".to_string();
    }

    /// 把已加载归档安装到浏览器，并完成待处理操作。
    pub(crate) fn apply_loaded_archive(
        &mut self,
        action: PendingPasswordAction,
        result: lumark_tasks::ArchiveLoadResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let PendingPasswordAction::OpenArchive { archive, intent } = action {
            self.open_archive_now(
                archive,
                result.password,
                intent,
                Some(result.info),
                window,
                cx,
            );
        }
    }

    /// 在密码解析完成后完成归档打开或解压。
    pub(crate) fn open_archive_now(
        &mut self,
        archive: PathBuf,
        password: Option<zeroize::Zeroizing<String>>,
        intent: ArchiveOpenIntent,
        archive_info: Option<lumark_archive::ArchiveInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = if let Some(info) = archive_info {
            self.browser.open_archive_info(archive.clone(), info)
        } else {
            self.browser
                .open_archive(&archive, password.as_deref().map(String::as_str))
        };
        match result {
            Ok(()) => {
                self.archive_password = password;
                self.search_results = None;
                self.dropped_sources = None;
                set_input(
                    &self.location_input,
                    self.browser.current_display().as_str(),
                    window,
                    cx,
                );
                if intent == ArchiveOpenIntent::Extract {
                    let dialog_password = self
                        .archive_password
                        .as_ref()
                        .map(|value| value.to_string());
                    self.prepare_extract_dialog(
                        archive,
                        false,
                        dialog_password.as_deref(),
                        window,
                        cx,
                    );
                } else {
                    self.status = format!("已打开 {}", archive.display());
                }
            }
            Err(error) => {
                self.archive_password = None;
                self.status = error.to_string();
            }
        }
    }

    /// 校验解压选项并提交可取消的解压任务。
    ///
    /// 归档路径和“仅解压选中条目”由独立解压窗口固定传入，避免窗口打开后
    /// 主窗口选择变化影响任务目标；目录和密码仍读取共享输入框。
    pub(crate) fn start_extract_for(
        &mut self,
        archive: PathBuf,
        selection_only: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let output = PathBuf::from(input_value(&self.extract_output, cx));
        if output.as_os_str().is_empty() {
            let message = "请输入解压目录".to_string();
            self.status = message.clone();
            return Err(message);
        }
        let mut password = input_value(&self.extract_password, cx);
        if password.is_empty()
            && let Some(known) = &self.archive_password
        {
            password = known.to_string();
        }
        let save_password = !password.is_empty();
        let open_after = self.settings.get().open_after_extract;
        let open_dir = output.clone();
        let overwrite_policy = match self.settings.get().overwrite_policy {
            lumark_settings::OverwritePolicy::Ask => ArchiveOverwritePolicy::Ask,
            lumark_settings::OverwritePolicy::Overwrite => ArchiveOverwritePolicy::Overwrite,
            lumark_settings::OverwritePolicy::Skip => ArchiveOverwritePolicy::Skip,
            lumark_settings::OverwritePolicy::Rename => ArchiveOverwritePolicy::Rename,
        };
        let options = ExtractionOptions {
            output_dir: output,
            overwrite: overwrite_policy != ArchiveOverwritePolicy::Skip,
            overwrite_policy,
            selected_paths: if selection_only {
                self.browser
                    .selection
                    .selected_ids()
                    .into_iter()
                    .filter_map(|id| {
                        self.browser
                            .entries
                            .iter()
                            .find(|entry| entry.id == id)
                            .map(|entry| entry.path.to_string_lossy().replace('\\', "/"))
                    })
                    .collect()
            } else {
                Vec::new()
            },
            password: (!password.is_empty()).then(|| zeroize::Zeroizing::new(password.clone())),
            ..Default::default()
        };
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Extract {
            archive: archive.clone(),
            options,
        });
        if save_password {
            self.pending_extract_saves
                .insert(id, (archive.clone(), zeroize::Zeroizing::new(password)));
        }
        if open_after {
            self.pending_extract_open_dirs.insert(id, open_dir);
        }
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = format!("开始解压：{}", archive.display());
        Ok(())
    }

    /// 测试所选归档，并在检测到加密时解析已保存密码。
    ///
    /// 加密探测放到后台线程，避免点击测试后主界面等待压缩包扫描而卡顿；
    /// 探测结果返回后再决定是否使用密码库记录或直接提交测试任务。
    pub(crate) fn start_test(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(archive) = self.selected_archive() else {
            self.status = "请选择一个压缩包".to_string();
            return;
        };
        if let Some(password) = &self.archive_password {
            self.submit_archive_test(archive, Some(password.to_string()));
            return;
        }
        self.status = format!("正在检查压缩包：{}", archive.display());
        let probe_archive = archive.clone();
        cx.spawn(async move |this, cx| {
            let encrypted = cx
                .background_executor()
                .spawn(async move {
                    ArchiveService::probe_encryption(&probe_archive)
                        .is_ok_and(|info| info.encrypted)
                })
                .await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.finish_start_test(archive, encrypted, window, cx);
            });
        })
        .detach();
    }

    /// 应用加密探测结果并提交测试任务或开始密码匹配。
    fn finish_start_test(
        &mut self,
        archive: PathBuf,
        encrypted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if encrypted {
            if self.begin_vault_password_resolution(
                &archive,
                PendingPasswordAction::Test {
                    archive: archive.clone(),
                },
                cx,
            ) {
                return;
            }
            let action = PendingPasswordAction::Test {
                archive: archive.clone(),
            };
            self.request_archive_password(archive, action, window, cx);
            self.status = "压缩包已加密，请输入密码".to_string();
            cx.notify();
            return;
        }
        self.submit_archive_test(archive, None);
        cx.notify();
    }

    /// 把归档完整性测试提交为后台任务。
    pub(crate) fn submit_archive_test(&mut self, archive: PathBuf, password: Option<String>) {
        let id = self.tasks.submit(TaskRequest::Test {
            archive: archive.clone(),
            password,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = format!("开始测试：{}", archive.display());
    }

    /// 把新验证成功的解压密码写入加密密码库。
    ///
    /// 保存提示只负责新增记录；若同一密码已经存在，则把当前压缩包路径
    /// 补充到已有条目，避免密码库出现重复密码。
    pub(crate) fn save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = input_value(&self.password_label, cx);
        let password = input_value(&self.password_value, cx);
        let pattern = input_value(&self.password_pattern, cx);
        let note = input_value(&self.password_note, cx);
        if password.is_empty() {
            self.status = "密码不能为空".to_string();
            return;
        }
        let duplicate = self
            .vault
            .entries()
            .into_iter()
            .find(|entry| entry.password.expose() == password);
        if let Some(mut entry) = duplicate {
            if !pattern.trim().is_empty() {
                entry.archive_pattern = Some(pattern.trim().to_string());
            }
            if !note.trim().is_empty() {
                entry.note = note.trim().to_string();
            }
            match self.vault.update_entry(entry) {
                Ok(()) => {
                    self.status = "密码已存在，已将当前压缩包关联到该条目".to_string();
                    self.show_save_prompt = false;
                }
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        match self.save_password_entry(None, &label, password, &pattern, &note) {
            Ok(_) => {
                self.show_save_prompt = false;
                set_input(&self.password_label, "", window, cx);
                set_input(&self.password_value, "", window, cx);
                set_input(&self.password_pattern, "", window, cx);
                set_input(&self.password_note, "", window, cx);
            }
            Err(error) => self.status = error,
        }
    }
}

/// 定位自解压运行时模块。
///
/// 安装包把 `lumark-sfx.exe` 放在主程序同目录；`LUMARK_SFX_STUB` 环境变量
/// 优先，便于开发环境和自动化测试指向构建产物。
fn sfx_stub_path() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("LUMARK_SFX_STUB") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("lumark-sfx.exe");
    candidate.is_file().then_some(candidate)
}

/// 返回密码动作对应的归档路径，供重新提交密码库匹配时使用。
fn password_action_archive(action: &PendingPasswordAction) -> &Path {
    match action {
        PendingPasswordAction::OpenArchive { archive, .. }
        | PendingPasswordAction::ExtractDialog { archive }
        | PendingPasswordAction::OpenEntry { archive, .. }
        | PendingPasswordAction::Test { archive } => archive,
    }
}

/// 返回默认归档名：单选使用文件 stem，多选优先父目录名，根目录退回第一项。
fn default_archive_name(sources: &[PathBuf]) -> String {
    let Some(first) = sources.first() else {
        return "archive".to_string();
    };
    let parent_name = first.parent().and_then(Path::file_name);
    let source_name = if first.is_dir() {
        first.file_name()
    } else {
        first.file_stem()
    };
    let archive_base = if sources.len() > 1 {
        parent_name.or(source_name)
    } else {
        source_name
    };
    archive_base
        .and_then(|value| value.to_str())
        .unwrap_or("archive")
        .to_string()
}

/// 返回适合当前机器的默认压缩线程数。
///
/// 7Z 的 LZMA2 多线程编码会按线程数分配工作缓冲，这里限制在 8 以内，
/// 兼顾速度与内存占用；ZIP 的 Deflate 仍由压缩库决定是否并行。
fn default_compress_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
        .clamp(1, 8)
}

#[cfg(test)]
mod tests {
    use super::default_archive_name;
    use std::path::PathBuf;

    /// 多选时优先使用父目录名。
    #[test]
    fn archive_name_uses_parent_for_multiple_sources() {
        let sources = [
            PathBuf::from(r"C:\工作\a.txt"),
            PathBuf::from(r"C:\工作\b.txt"),
        ];
        assert_eq!(default_archive_name(&sources), "工作");
    }

    /// 根目录没有父目录名时退回第一个文件名。
    #[test]
    fn archive_name_falls_back_to_first_file_at_root() {
        let sources = [PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b.txt")];
        assert_eq!(default_archive_name(&sources), "a");
    }

    /// 单选文件使用文件 stem，而不是父目录名。
    #[test]
    fn archive_name_uses_file_name_for_single_source() {
        let sources = [PathBuf::from(r"C:\工作\a.txt")];
        assert_eq!(default_archive_name(&sources), "a");
    }
}
