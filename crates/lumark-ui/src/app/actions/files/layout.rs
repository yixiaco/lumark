//! 文件列表的详情区和列宽布局动作。

use super::*;

impl LumarkApp {
    /// 切换详情检查器并持久化偏好。
    pub(crate) fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
        let visible = self.details_visible;
        let _ = self
            .settings
            .update(|settings| settings.details_visible = visible);
    }

    /// 循环切换文件列表列宽并持久化所选布局。
    pub(crate) fn cycle_column_layout(&mut self) {
        self.column_layout = (self.column_layout + 1) % 3;
        let layout = self.column_layout;
        self.column_widths = ColumnWidths::from_layout(layout);
        let _ = self.settings.update(|settings| {
            settings.column_layout = layout;
            settings.column_widths = None;
        });
    }

    /// 清除逐列拖动结果，恢复当前预设列宽。
    pub(crate) fn reset_column_widths(&mut self) {
        let layout = self.column_layout;
        self.column_widths = ColumnWidths::from_layout(layout);
        if let Err(error) = self
            .settings
            .update(|settings| settings.column_widths = None)
        {
            self.status = error.to_string();
        }
    }
}
