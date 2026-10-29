//! 归档打开、条目查看和后台加载。

use super::*;
use crate::i18n::tr;
use gpui_kit::component::button::ButtonVariant;

impl LumarkApp {
    /// 在新的独立窗口中浏览归档内解压出的嵌套压缩包。
    ///
    /// 临时目录所有权在窗口创建成功后转交子窗口；失败时调用方仍持有
    /// `OwnedTempDir`，会在返回后删除该目录。
    pub(crate) fn spawn_nested_archive_window(
        &self,
        archive: PathBuf,
        owned_temp: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let stores = self.shared_stores();
        let options = window_options(&stores);
        let request = StartupRequest::for_nested_archive(archive, owned_temp);
        gpui_kit::open_window(options, cx, move |window, cx| {
            window.on_next_frame(|window, _| window.activate_window());
            cx.new(|cx| Self::new_shared(window, cx, stores, None, Some(request)))
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

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

    /// 把归档内的一个压缩包解压到子窗口专属目录，并在新窗口中打开。
    pub(crate) fn open_nested_archive_entry(
        &mut self,
        entry: BrowserEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_archive_entry_as(entry, PendingOpenMode::NestedArchive, window, cx);
    }

    /// 按指定处理方式解压归档条目并记录后续动作。
    pub(crate) fn open_archive_entry_as(
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
                self.unlock_for_browse = true;
                self.request_unlock_for_extract(window, cx);
                self.status = tr(
                    "压缩包已加密，解锁密码库后将尝试记录密码",
                    "Archive is encrypted; saved passwords will be tried after unlocking the vault",
                )
                .to_string();
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
            self.status = tr(
                "请等待当前归档条目准备完成",
                "Wait for the current archive entry to finish preparing",
            )
            .to_string();
            return;
        }
        let temporary = std::env::temp_dir()
            .join("Lumark")
            .join("open")
            .join(Uuid::new_v4().to_string());
        let archive_for_target = archive.clone();
        let password_for_target = password.clone();
        let options = ExtractionOptions {
            output_dir: temporary.clone(),
            selected_paths: vec![relative.clone()],
            overwrite: true,
            password,
            ..Default::default()
        };
        let id = self.tasks.submit(TaskRequest::Extract { archive, options });
        let owned_temp =
            (mode == PendingOpenMode::NestedArchive).then(|| OwnedTempDir::new(temporary.clone()));
        self.pending_open_entry = Some(PendingOpenEntry {
            task_id: id,
            temporary,
            relative,
            mode,
            owned_temp,
            archive: archive_for_target,
            password: password_for_target,
        });
        self.active_task = Some(id);
        self.task_progress = None;
        self.status = tr(
            "正在准备打开归档条目…",
            "Preparing to open archive entry...",
        )
        .to_string();
        let _ = window;
        let _ = cx;
    }

    /// 打开临时解压结果；可执行文件先弹官方确认框。
    pub(crate) fn open_extracted_target(
        &mut self,
        target: PathBuf,
        temporary: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_executable_like(&target) {
            let app = cx.entity();
            let dialog_target = target.clone();
            let temporary_text = temporary.display().to_string();
            window.open_alert_dialog(cx, move |alert, _, _| {
                let app = app.clone();
                let target = dialog_target.clone();
                alert
                    .title(tr(
                        "打开归档中的可执行文件？",
                        "Open executable from archive?",
                    ))
                    .description(format!(
                        "{}\n{}{}",
                        tr(
                            "该条目已解压到临时目录，但可执行文件或脚本可能修改系统。",
                            "The entry has been extracted to a temporary folder, but executable files or scripts may modify the system.",
                        ),
                        tr("路径：", "Path: "),
                        target.display()
                    ))
                    .confirm()
                    .ok_text(tr("仍然打开", "Open anyway"))
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text(tr("取消", "Cancel"))
                    .on_ok(move |_, _, cx| {
                        let app = app.clone();
                        let target = target.clone();
                        app.update(cx, |app, cx| {
                            app.open_confirmed_target(target);
                            cx.notify();
                        });
                        true
                    })
            });
            self.status = format!(
                "{}{}{}",
                tr(
                    "归档条目已解压到临时目录：",
                    "Archive entry extracted to temporary folder: "
                ),
                temporary_text,
                tr(
                    "；打开可执行文件前需要确认",
                    "; confirmation is required before opening an executable"
                )
            );
            return;
        }
        self.open_confirmed_target(target);
        self.status = format!(
            "{}{}",
            tr("已解压到临时目录：", "Extracted to temporary folder: "),
            temporary.display()
        );
    }

    /// 实际启动系统默认应用打开解压结果。
    fn open_confirmed_target(&mut self, target: PathBuf) {
        let _ = std::process::Command::new("explorer.exe")
            .arg(&target)
            .spawn();
        self.status = tr(
            "已按用户确认打开归档中的可执行文件",
            "Opened the archive executable after confirmation",
        )
        .to_string();
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
                            self.unlock_for_browse = intent == ArchiveOpenIntent::Browse;
                            self.request_unlock_for_extract(window, cx);
                            self.status = tr("压缩包已加密，解锁密码库后将尝试记录密码", "Archive is encrypted; saved passwords will be tried after unlocking the vault").to_string();
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
        self.status = tr("正在读取归档目录…", "Reading archive contents...").to_string();
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
                    self.status = format!("{}{}", tr("已打开 ", "Opened "), archive.display());
                }
            }
            Err(error) => {
                self.archive_password = None;
                self.status = error.to_string();
            }
        }
    }
}
