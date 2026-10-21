//! 密码库安全卡片：自动解压、空闲锁定和密码库维护操作。
use super::super::super::*;
use super::super::components::{AppIcon, card, section_heading};
use gpui_kit::base::Disableable as _;

impl LumarkApp {
    /// 渲染自动解压与安全设置卡片。
    pub(super) fn render_password_security(
        &self,
        settings: lumark_vault::VaultSettings,
        auto_lock_label: String,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let has_master = self.vault.has_master_password();
        card(cx)
                    .flex_shrink_0()
                    .p_4()
                    .gap_3()
                    .child(section_heading(
                        cx,
                        AppIcon::Shield,
                        "自动解压与安全",
                        Some("只在本机验证记录密码".to_string()),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                button("toggle-auto")
                                    .icon(AppIcon::Key.element(15.))
                                    .label(if settings.auto_extract {
                                        "自动尝试密码：开"
                                    } else {
                                        "自动尝试密码：关"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_auto_extract(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("toggle-save")
                                    .icon(AppIcon::Save.element(15.))
                                    .label(if settings.prompt_save {
                                        "保存密码提示：开"
                                    } else {
                                        "保存密码提示：关"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_prompt_save(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("toggle-auto-lock")
                                    .icon(AppIcon::Time.element(15.))
                                    .label(auto_lock_label)
                                    .disabled(!has_master)
                                    .tooltip(if has_master {
                                        "空闲一段时间后锁定密码库"
                                    } else {
                                        "未设置主密码时不需要锁定密码库"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_auto_lock(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("lock-vault")
                                    .icon(AppIcon::Lock.element(15.))
                                    .label("立即锁定")
                                    .disabled(!has_master)
                                    .tooltip(if has_master {
                                        "立即锁定密码库"
                                    } else {
                                        "未设置主密码时不需要锁定密码库"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.vault.has_master_password() {
                                            this.status = "未设置主密码，密码库不需要锁定".to_string();
                                            cx.notify();
                                            return;
                                        }
                                        this.vault.lock();
                                        this.dismiss_password_windows(cx);
                                        this.selected_password = None;
                                        this.status = "密码库已锁定".to_string();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("vault-transfer")
                                    .icon(AppIcon::Copy.element(15.))
                                    .label("导入 / 导出")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if input_value(&this.vault_transfer_path, cx)
                                            .trim()
                                            .is_empty()
                                        {
                                            let target = dirs::document_dir()
                                                .unwrap_or_else(std::env::temp_dir)
                                                .join("Lumark.lumarkvault");
                                            set_input(
                                                &this.vault_transfer_path,
                                                target.to_string_lossy().as_ref(),
                                                window,
                                                cx,
                                            );
                                        }
                                        this.show_vault_transfer = true;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("vault-clear")
                                    .danger()
                                    .icon(AppIcon::Delete.element(15.))
                                    .label("清除密码库")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_confirm_clear_vault = true;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .p_3()
                            .child(AppIcon::Shield.element(16.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "密码库使用 XChaCha20-Poly1305 加密；设置主密码后由 Argon2id 派生密钥保护。",
                                    ),
                            ),
                    )
            .into_any_element()
    }
}
