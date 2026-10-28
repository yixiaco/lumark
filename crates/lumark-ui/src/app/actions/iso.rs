//! ISO 虚拟光驱挂载与卸载动作。

use super::super::*;
use crate::i18n::tr;
use lumark_platform_windows::iso as iso_platform;

impl LumarkApp {
    /// 把当前选中的 ISO 挂载为 Windows 虚拟光驱。
    pub(crate) fn mount_selected_iso(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_entry()
            .filter(|entry| !entry.is_dir && is_iso_path(&entry.path))
            .map(|entry| entry.path)
        else {
            self.status = tr("请选择一个 ISO 文件", "Select an ISO file").to_string();
            cx.notify();
            return;
        };
        self.mount_iso_path(path, window, cx);
    }

    /// 挂载指定 ISO。
    ///
    /// 挂载和卷枚举在后台执行，成功后自动打开对应盘符；失败只更新状态栏，
    /// 不弹阻塞式对话框。已挂载的 ISO 会直接返回现有盘符。
    pub(crate) fn mount_iso_path(
        &mut self,
        path: PathBuf,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_iso_path(&path) {
            self.status = tr("请选择一个 ISO 文件", "Select an ISO file").to_string();
            cx.notify();
            return;
        }
        self.status = format!("{}{}", tr("正在挂载 ", "Mounting "), path.display());
        cx.notify();
        let mount_path = path;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { iso_platform::mount(&mount_path) })
                .await;
            let _ = this.update_in(cx, |app, _, cx| {
                match result {
                    Ok(mounted) => {
                        if let Some(letter) = mounted.drive_letter {
                            app.status = format!("已挂载到 {letter}: 虚拟光驱");
                            let drive = PathBuf::from(format!("{letter}:\\"));
                            let _ = lumark_platform_windows::open_default(&drive);
                        } else {
                            app.status = tr(
                                "ISO 已挂载，但系统尚未分配盘符",
                                "ISO mounted, but Windows has not assigned a drive letter yet",
                            )
                            .to_string();
                        }
                    }
                    Err(error) => {
                        app.status = format!("{}{error}", tr("挂载失败：", "Mount failed: "))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 卸载当前选中 ISO 对应的虚拟光驱。
    pub(crate) fn unmount_selected_iso(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_entry()
            .filter(|entry| !entry.is_dir && is_iso_path(&entry.path))
            .map(|entry| entry.path)
        else {
            self.status = tr("请选择一个 ISO 文件", "Select an ISO file").to_string();
            cx.notify();
            return;
        };
        self.unmount_iso_path(path, window, cx);
    }

    /// 卸载指定 ISO；未挂载时按成功处理。
    pub(crate) fn unmount_iso_path(
        &mut self,
        path: PathBuf,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_iso_path(&path) {
            self.status = tr("请选择一个 ISO 文件", "Select an ISO file").to_string();
            cx.notify();
            return;
        }
        self.status = format!("{}{}", tr("正在卸载 ", "Unmounting "), path.display());
        let label = path.display().to_string();
        cx.notify();
        let unmount_path = path;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { iso_platform::unmount(&unmount_path) })
                .await;
            let _ = this.update_in(cx, |app, _, cx| {
                match result {
                    Ok(()) => {
                        app.status = format!(
                            "{}{label}",
                            tr("已卸载虚拟光驱：", "Virtual drive unmounted: ")
                        )
                    }
                    Err(error) => {
                        app.status = format!("{}{error}", tr("卸载失败：", "Unmount failed: "))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
