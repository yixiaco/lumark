//! 压缩窗口表单动作、校验和任务提交。

use super::*;

impl CompressWindowView {
    /// 刷新格式并把输出输入的扩展名同步过去。
    pub(super) fn set_format(
        &mut self,
        format: ArchiveFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.format = format;
        if !matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip) {
            // 自解压运行时只实现 ZIP/7Z 解压，切换格式时自动取消勾选。
            self.sfx = false;
        }
        let current = PathBuf::from(input_value(&self.output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = if self.disguise {
                replace_disguise_extension(&current, &self.disguise_output_extension())
            } else {
                replace_archive_extension(&current, format)
            };
            set_input(
                &self.output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 格式选择旁的解释文本。
    ///
    /// 单文件源只显示扩展名；多文件或文件夹源会提示已限制为可容纳多条目的
    /// 格式，避免用户误以为 GZ/Brotli 等单流格式仍可用。开启伪装后同时
    /// 显示面具扩展名和内层归档扩展名。
    pub(super) fn format_hint(&self) -> String {
        if self.disguise {
            let extension = disguise_extension(self.disguise_mask, self.disguise_custom.as_deref())
                .unwrap_or_else(|| "mp4".to_string());
            return format!(
                "输出扩展名 .{extension}（内层 .{}）",
                self.format.extension()
            );
        }
        let extension = self.format.extension();
        if !single_stream_allowed(&self.sources) && extension.starts_with("tar") {
            format!("扩展名 .{extension}（多文件源用 TAR 变体）")
        } else {
            format!("扩展名 .{extension}")
        }
    }

    /// 切换压缩预设，并同步 ZPAQ 的当前级别。
    ///
    /// 轻巧模式只有这三个预设、没有算法页；如果只改标签而不同步级别，
    /// “体积小”仍会沿用窗口打开时的 ZPAQ 方法，压缩效果自然和平衡接近。
    pub(super) fn set_preset(
        &mut self,
        preset: SettingsPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preset = preset;
        self.zpaq_compression = ZpaqCompression::from_preset(archive_preset(preset));
        let index = zpaq_method_index(self.zpaq_compression);
        self.zpaq_method_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(index)), window, cx);
        });
        cx.notify();
    }

    /// 切换自解压输出，并把输出扩展名在 `.exe` 与当前格式扩展名之间同步。
    pub(super) fn set_sfx(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.sfx = enabled;
        if enabled {
            // 自解压与伪装互斥。
            self.disguise = false;
        }
        let current = PathBuf::from(input_value(&self.output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = if enabled {
                current.with_extension("exe")
            } else if self.disguise {
                replace_disguise_extension(&current, &self.disguise_output_extension())
            } else {
                replace_archive_extension(&current, self.format)
            };
            set_input(
                &self.output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 打开系统目录选择器，只替换输出路径的父目录。
    pub(super) fn choose_directory(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let current = PathBuf::from(input_value(&self.output, cx));
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
                let _ = this.update_in(cx, |view, window, cx| {
                    let name = current
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("archive.zip");
                    let target = directory.join(name);
                    set_input(&view.output, target.to_string_lossy().as_ref(), window, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 打开系统文件选择器，为自解压 EXE 选择图标。
    pub(super) fn choose_sfx_icon(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择自解压图标（.ico）".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(icon) = paths.into_iter().next()
            {
                let _ = this.update(cx, |view, cx| {
                    view.sfx_icon = Some(icon);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 关闭压缩窗口，不改动主应用已经提交的任务。
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| {
            app.status = "已取消压缩".to_string();
            cx.notify();
        });
        window.remove_window();
    }

    /// 校验表单并提交压缩任务，然后关闭独立窗口。
    pub(super) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut output = PathBuf::from(input_value(&self.output, cx));
        if output.as_os_str().is_empty() {
            self.error = Some("请输入输出路径".to_string());
            cx.notify();
            return;
        }
        if self.sources.is_empty() {
            self.error = Some("当前没有可压缩的项目".to_string());
            cx.notify();
            return;
        }
        // 正常流程在打开窗口时已隐藏单流格式；这里防御直接调用或文件状态
        // 变化（例如唯一源在窗口打开后被换成目录）导致的无效组合。
        if self.format.is_single_stream() && !single_stream_allowed(&self.sources) {
            let suggestion = self
                .format
                .single_stream_tar_variant()
                .map(|format| format.label())
                .unwrap_or("TAR 归档");
            self.error = Some(format!(
                "{} 只能压缩一个普通文件；压缩多个文件或文件夹请改用 {}。",
                self.format.label(),
                suggestion
            ));
            cx.notify();
            return;
        }
        let password = input_value(&self.password, cx);
        // 伪装先把面具解析成具体文件，自定义面具缺失时直接给出可操作提示。
        let disguise = if self.disguise {
            match resolve_disguise_mask(self.disguise_mask, self.disguise_custom.as_deref()) {
                Ok(mask) => Some(mask),
                Err(message) => {
                    self.error = Some(message);
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        // 自解压支持 ZIP/7Z 和密码；分卷与伪装仍不支持。
        let sfx = self.sfx && matches!(self.format, ArchiveFormat::Zip | ArchiveFormat::SevenZip);
        if sfx && disguise.is_some() {
            self.error = Some("自解压与伪装不能同时启用".to_string());
            cx.notify();
            return;
        }
        let split_value = input_value(&self.split, cx);
        let split_size = match split_size_or_error(&split_value, self.split_unit) {
            Ok(size) => size,
            Err(message) if self.classic => {
                self.error = Some(message.to_string());
                cx.notify();
                return;
            }
            // 轻巧模式没有分卷字段，隐藏的旧值按原有规则静默忽略。
            Err(_) => None,
        };
        if sfx && split_size.is_some_and(|size| size >= 64 * 1024) {
            self.error = Some("自解压不支持分卷".to_string());
            cx.notify();
            return;
        }
        if disguise.is_some() && split_size.is_some_and(|size| size >= 64 * 1024) {
            self.error = Some("伪装不支持分卷，请先清空分卷大小".to_string());
            cx.notify();
            return;
        }
        if let Some(mask) = &disguise {
            let extension = mask
                .extension()
                .unwrap_or_else(|_| self.format.extension().to_string());
            output = replace_disguise_extension(&output, &extension);
        } else if sfx {
            output.set_extension("exe");
        }
        let sfx_icon = if sfx {
            if let Some(icon) = &self.sfx_icon
                && !icon
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ico"))
            {
                self.error = Some("自解压图标必须是 .ico 文件".to_string());
                cx.notify();
                return;
            }
            self.sfx_icon.clone()
        } else {
            None
        };
        let comment = self.comment.read(cx).value().to_string();
        let (modified_after, modified_before) = match self.file_time_filter {
            FileTimeFilter::Any => (None, None),
            FileTimeFilter::Before => (None, parse_date(&input_value(&self.before, cx))),
            FileTimeFilter::After => (parse_date(&input_value(&self.after, cx)), None),
            FileTimeFilter::Range => (
                parse_date(&input_value(&self.after, cx)),
                parse_date(&input_value(&self.before, cx)),
            ),
        };
        let zpaq_compression =
            effective_zpaq_compression(self.classic, self.preset, self.zpaq_compression);
        let options = CompressionOptions {
            format: self.format,
            level: Some(level_for_preset(self.preset)),
            preset: archive_preset(self.preset),
            solid: self.solid && self.format == ArchiveFormat::SevenZip,
            encrypt_header: self.encrypt_header,
            dictionary_size: (self.format == ArchiveFormat::SevenZip)
                .then_some(self.dictionary_mb.saturating_mul(1024 * 1024)),
            threads: self.threads.clamp(1, 64),
            zip_compression: self.zip_compression,
            sevenz_compression: self.sevenz_compression,
            zpaq_compression,
            word_size: (self.format == ArchiveFormat::SevenZip).then_some(self.word_size),
            solid_block_size: (self.format == ArchiveFormat::SevenZip)
                .then_some(self.solid_block_mb as u64 * 1024 * 1024),
            archive_time: self.archive_time,
            file_time_filter: self.file_time_filter,
            modified_after,
            modified_before,
            path_mode: self.path_mode,
            password: (!password.is_empty()).then(|| zeroize::Zeroizing::new(password)),
            zip_encryption: if self.zipcrypto {
                ZipEncryption::ZipCrypto
            } else {
                ZipEncryption::Aes256
            },
            comment: (!comment.trim().is_empty()).then_some(comment),
            split_size,
            test_after: self.test_after,
            delete_source: self.delete_source,
            sfx,
            sfx_icon,
            disguise: disguise.map(|mask| DisguiseOptions {
                mask,
                original_name: None,
            }),
            exclude_patterns: parse_patterns(&input_value(&self.exclude, cx)),
            store_only_patterns: parse_patterns(&input_value(&self.store, cx)),
            ..Default::default()
        };
        let sources = self.sources.clone();
        let task_options = CompressTaskOptions {
            shutdown_after: self.shutdown_after,
            low_priority: self.low_priority,
        };
        let app = self.app.clone();
        app.update(cx, |app, cx| {
            app.start_compress_from_options(output, sources, options, task_options, window, cx);
        });
        window.remove_window();
    }

    /// 估算当前 7Z 算法所需的内存。
    pub(super) fn memory_hint(&self) -> String {
        if self.format == ArchiveFormat::SevenZip {
            let encoder = self.dictionary_mb.saturating_mul(10) + self.word_size;
            let decoder = self.dictionary_mb + 2;
            format!("压缩所需内存：约 {encoder} MB；解压所需内存：约 {decoder} MB")
        } else if self.format == ArchiveFormat::Zip {
            "ZIP Deflate 使用固定内存；LZMA/BZip2 方法会额外占用少量内存。".to_string()
        } else {
            "该格式由流编码器处理，内存占用较小。".to_string()
        }
    }
    /// 在轻巧和经典布局之间切换，并调整窗口尺寸。
    pub(super) fn switch_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.classic = !self.classic;
        let (width, height) = if self.classic {
            CLASSIC_WINDOW_SIZE
        } else {
            LIGHT_WINDOW_SIZE
        };
        window.resize(gpui_kit::size(px(width), px(height)));
        self.output.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }
}
