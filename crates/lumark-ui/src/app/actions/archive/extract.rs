//! 解压窗口、加密探测和任务提交。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 为所选归档打开解压流程，并在提示前探测加密状态。
    pub(crate) fn open_extract_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(archive) = self.selected_archive() else {
            self.status = tr("请选择一个压缩包", "Select an archive").to_string();
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
    pub(crate) fn apply_extract_probe(
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
                self.status = format!(
                    "{}{}",
                    tr("准备解压：", "Preparing to extract: "),
                    archive.display()
                );
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
        let _ = window;
        cx.notify();
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
        self.status = format!(
            "{}{}",
            tr("开始解压：", "Starting extraction: "),
            archive.display()
        );
        Ok(())
    }
}
