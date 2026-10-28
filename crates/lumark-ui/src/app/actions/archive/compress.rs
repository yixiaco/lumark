//! 压缩窗口打开、输出目录选择和任务提交。

use super::*;

impl LumarkApp {
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
        // 单文件流格式不能容纳多个源；多选或文件夹源自动改用对应的 TAR
        // 变体，避免用户看到“归档损坏”这类误导性错误。
        let format = resolve_compress_format(format, &sources);
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
        self.compress_sfx &= matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip);
        let mut output = output_dir.join(format!("{default_name}.{}", format.extension()));
        if self.compress_sfx {
            output.set_extension("exe");
        } else if self.compress_disguise {
            let extension = disguise_extension(
                self.compress_disguise_mask,
                self.compress_disguise_custom.as_deref(),
            )
            .unwrap_or_else(|| format.extension().to_string());
            output = replace_disguise_extension(&output, &extension);
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
        self.compress_disguise = options.disguise.is_some();
        if let Some(disguise) = &options.disguise {
            self.compress_disguise_mask = match &disguise.mask {
                lumark_archive::DisguiseMask::PresetVideo => DisguiseMaskPreference::Video,
                lumark_archive::DisguiseMask::PresetImage => DisguiseMaskPreference::Image,
                lumark_archive::DisguiseMask::File(path) => {
                    self.compress_disguise_custom = Some(path.clone());
                    DisguiseMaskPreference::Custom
                }
            };
        }
        let disguised = options.disguise.is_some();
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
        let label = format!(
            "{} {}",
            if disguised { "伪装压缩" } else { "压缩" },
            output.display()
        );
        self.record_task_queued(id, TaskKind::Compress, label.clone());
        self.active_task = Some(id);
        self.active_task_kind = Some(TaskKind::Compress);
        self.active_task_label = Some(label);
        self.task_progress = None;
        self.show_compress = false;
        self.status = match priority_error {
            Some(error) => format!("开始压缩：{}（降低优先级失败：{error}）", output.display()),
            None if disguised => format!("开始伪装压缩：{}", output.display()),
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
}
