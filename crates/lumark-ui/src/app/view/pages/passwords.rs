//! 密码库页面：组合记录列表、详情、主密码入口和安全操作。
use super::super::super::*;
use super::super::components::AppIcon;
use super::shared::stat_card;
use gpui_kit::component::scroll::ScrollableElement;

impl LumarkApp {
    /// 渲染密码工作区。
    ///
    /// 记录修改会立即写入加密密码库；新建和编辑在独立窗口中完成，
    /// 页面只负责列表、详情和主密码入口，不主动暴露密码。
    pub(crate) fn render_passwords(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let search = input_value(&self.password_search, cx).to_lowercase();
        let entries: Vec<PasswordEntry> = self
            .vault
            .entries()
            .into_iter()
            .filter(|entry| {
                search.is_empty()
                    || entry.label.to_lowercase().contains(&search)
                    || entry.note.to_lowercase().contains(&search)
                    || entry
                        .archive_pattern
                        .as_deref()
                        .is_some_and(|pattern| pattern.to_lowercase().contains(&search))
            })
            .collect();
        let settings = self.vault.settings();
        let locked = self.vault.state() == VaultState::Locked;
        let has_master = self.vault.has_master_password();
        let auto_lock_label = if settings.auto_lock_minutes == 0 {
            "空闲锁定：关".to_string()
        } else {
            format!("空闲锁定：{} 分钟", settings.auto_lock_minutes)
        };
        let total_records = self.vault.entry_count();
        let selected = self
            .selected_password
            .and_then(|id| entries.iter().find(|entry| entry.id == id).cloned());

        v_flex()
            .id("passwords-page")
            .w_full()
            .h_full()
            .min_h_0()
            .overflow_y_scrollbar()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .w_full()
                    .child(stat_card(
                        cx,
                        AppIcon::Key,
                        "记录总数",
                        &total_records.to_string(),
                    ))
                    .child(stat_card(
                        cx,
                        if locked {
                            AppIcon::Lock
                        } else {
                            AppIcon::Unlock
                        },
                        "密码库状态",
                        if locked { "已锁定" } else { "已解锁" },
                    ))
                    .child(stat_card(
                        cx,
                        AppIcon::Shield,
                        "自动尝试",
                        if settings.auto_extract {
                            "已开启"
                        } else {
                            "已关闭"
                        },
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_h_0()
                    .flex_shrink_0()
                    .items_stretch()
                    .gap_3()
                    .child(self.render_password_list(entries, locked, cx))
                    .when(!locked, |this| {
                        this.child(self.render_password_sidebar(selected, has_master, cx))
                    }),
            )
            .child(self.render_password_security(settings, auto_lock_label, cx))
    }
}
