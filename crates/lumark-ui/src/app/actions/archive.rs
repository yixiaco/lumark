use super::super::*;

impl LumarkApp {
    /// 先把一个归档条目解压到受控临时目录，再打开它。
    pub(crate) fn open_archive_entry(
        &mut self,
        entry: BrowserEntry,
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
                },
            ) {
                return;
            } else if self.vault.state() == VaultState::Locked {
                self.pending_unlock_archive = Some(archive);
                self.show_unlock_for_extract = true;
                self.unlock_for_browse = true;
                self.status = "压缩包已加密，解锁密码库后将尝试记录密码".to_string();
                return;
            } else {
                self.status = "压缩包已加密，请先使用解压对话框输入密码".to_string();
                return;
            }
        } else {
            None
        };
        self.open_archive_entry_with_password(archive, relative, password, window, cx);
    }

    /// 使用已解析的密码解压一个条目。
    pub(crate) fn open_archive_entry_with_password(
        &mut self,
        archive: PathBuf,
        relative: String,
        password: Option<zeroize::Zeroizing<String>>,
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
        let default_name = if first.is_dir() {
            first
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
                .to_string()
        } else {
            first
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
                .to_string()
        };
        let output_dir = settings
            .default_output_dir
            .filter(|path| path.is_dir())
            .or_else(|| first.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let output = unique_output_path(
            output_dir.join(format!("{default_name}.{}", format.extension())),
            format,
        );
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
        self.open_compress_window(window, cx);
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if sources.is_empty() {
            self.status = "当前没有可压缩的项目".to_string();
            return;
        }
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Compress {
            output: output.clone(),
            sources,
            options,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.show_compress = false;
        self.status = format!("开始压缩：{}", output.display());
        cx.notify();
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
        let split_size = parse_size(&input_value(&self.compress_split, cx));
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
            ..Default::default()
        };
        let id = self.tasks.submit(lumark_tasks::TaskRequest::Compress {
            output: output.clone(),
            sources,
            options,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.show_compress = false;
        self.status = format!("开始压缩：{}", output.display());
        let _ = window;
        let _ = cx;
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
            self.status = "已从密码管理中找到可用密码".to_string();
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
                    self.status = "压缩包已加密，请输入密码".to_string();
                } else {
                    match self.vault.state() {
                        VaultState::Unlocked => {
                            if !self.begin_vault_password_resolution(
                                &archive,
                                PendingPasswordAction::ExtractDialog {
                                    archive: archive.clone(),
                                },
                            ) {
                                self.status = "没有可用的记录密码，请手动输入".to_string();
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
                            self.status = "压缩包已加密，请输入密码".to_string();
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
    ) -> bool {
        if self.vault.state() != VaultState::Unlocked || !self.vault.settings().auto_extract {
            return false;
        }
        let fingerprint = lumark_vault::PasswordResolver::archive_fingerprint(archive).ok();
        let candidates = self.vault.candidates(archive, fingerprint.as_deref());
        let mut candidates = candidates.into_iter();
        let primary: Vec<_> = candidates.by_ref().take(32).collect();
        let extras: Vec<_> = candidates.collect();
        self.submit_password_candidates(archive, action, primary, extras)
    }

    /// 在工作线程中校验有序候选密码，不写入输出。
    pub(crate) fn submit_password_candidates(
        &mut self,
        archive: &Path,
        action: PendingPasswordAction,
        candidates: Vec<lumark_vault::PasswordCandidate>,
        extras: Vec<lumark_vault::PasswordCandidate>,
    ) -> bool {
        if candidates.is_empty() {
            return false;
        }
        self.vault.acquire_lease();
        let entry_ids = candidates
            .iter()
            .map(|candidate| candidate.entry_id)
            .collect();
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
            },
        );
        self.status = "正在验证记录密码…".to_string();
        true
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
        if !self.submit_password_candidates(&archive, extra.action, extra.candidates, Vec::new()) {
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
            PendingPasswordAction::OpenEntry { archive, relative } => {
                self.open_archive_entry_with_password(
                    archive,
                    relative,
                    Some(password),
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
                self.prepare_extract_dialog(archive, false, None, window, cx);
                self.status = "记录密码均不正确，请手动输入密码".to_string();
            }
            PendingPasswordAction::OpenArchive { archive, intent } => {
                if intent == ArchiveOpenIntent::Extract {
                    self.prepare_extract_dialog(archive, false, None, window, cx);
                    self.status = "记录密码均不正确，请手动输入密码".to_string();
                }
            }
            PendingPasswordAction::OpenEntry { .. } | PendingPasswordAction::Test { .. } => {}
        }
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
                    self.prepare_extract_dialog(archive, false, None, window, cx);
                } else {
                    self.status = "压缩包已加密，请先通过解压对话框输入密码".to_string();
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
            let _ = this.update_in(cx, |app, _window, cx| {
                app.finish_start_test(archive, encrypted, cx);
            });
        })
        .detach();
    }

    /// 应用加密探测结果并提交测试任务或开始密码匹配。
    fn finish_start_test(&mut self, archive: PathBuf, encrypted: bool, cx: &mut Context<Self>) {
        if encrypted {
            if self.begin_vault_password_resolution(
                &archive,
                PendingPasswordAction::Test {
                    archive: archive.clone(),
                },
            ) {
                return;
            }
            self.status = "压缩包已加密，请解锁密码库或添加匹配密码后重试".to_string();
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
    pub(crate) fn save_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = input_value(&self.password_label, cx);
        let password = input_value(&self.password_value, cx);
        let confirm = input_value(&self.password_confirm, cx);
        let pattern = input_value(&self.password_pattern, cx);
        if password.is_empty() {
            self.status = "密码不能为空".to_string();
            return;
        }
        if password != confirm {
            self.status = "两次输入的密码不一致".to_string();
            return;
        }
        let password_key = password.clone();
        let pattern_key = pattern.clone();
        let result = if let Some(id) = self.selected_password {
            let Some(mut entry) = self.vault.find_entry(id) else {
                self.status = "密码条目不存在".to_string();
                return;
            };
            entry.label = label;
            entry.password = lumark_vault::SecretString::new(password);
            entry.archive_pattern = (!pattern.is_empty()).then_some(pattern);
            entry.note = input_value(&self.password_note, cx);
            self.vault.update_entry(entry)
        } else {
            let mut entry = PasswordEntry::new(
                if label.is_empty() {
                    "未命名密码".to_string()
                } else {
                    label
                },
                password,
            );
            entry.archive_pattern = (!pattern.is_empty()).then_some(pattern);
            entry.note = input_value(&self.password_note, cx);
            self.vault.add_entry(entry).map(|_| ())
        };
        match result {
            Ok(()) => {
                self.status = "密码已保存到加密密码库".to_string();
                self.selected_password = None;
                self.show_save_prompt = false;
                set_input(&self.password_label, "", window, cx);
                set_input(&self.password_value, "", window, cx);
                set_input(&self.password_confirm, "", window, cx);
                set_input(&self.password_pattern, "", window, cx);
                set_input(&self.password_note, "", window, cx);
            }
            Err(lumark_vault::VaultError::DuplicatePassword) => {
                let duplicate = self
                    .vault
                    .entries()
                    .into_iter()
                    .find(|entry| entry.password.expose() == password_key);
                if let Some(mut entry) = duplicate {
                    if !pattern_key.trim().is_empty() {
                        entry.archive_pattern = Some(pattern_key);
                    }
                    let note = input_value(&self.password_note, cx);
                    if !note.trim().is_empty() {
                        entry.note = note;
                    }
                    match self.vault.update_entry(entry) {
                        Ok(()) => {
                            self.status = "密码已存在，已将当前压缩包关联到该条目".to_string();
                            self.show_save_prompt = false;
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                } else {
                    self.status = "密码已存在".to_string();
                }
            }
            Err(error) => self.status = error.to_string(),
        }
    }
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
