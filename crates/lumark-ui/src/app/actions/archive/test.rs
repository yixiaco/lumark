use super::*;

impl LumarkApp {
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
    pub(crate) fn finish_start_test(
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
}
