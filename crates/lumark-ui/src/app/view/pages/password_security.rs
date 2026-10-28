//! 密码库安全卡片：自动解压、空闲锁定和密码库维护操作。
use super::super::super::*;
use super::super::components::{AppIcon, card, section_heading};
use crate::i18n::tr;
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
                        tr("自动解压与安全", "Auto-extract and security"),
                        Some(tr("只在本机验证记录密码", "Verify saved passwords locally").to_string()),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                button("toggle-auto")
                                    .icon(AppIcon::Key.element(15.))
                                    .label(if settings.auto_extract {
                                        tr("自动尝试密码：开", "Auto-try passwords: on")
                                    } else {
                                        tr("自动尝试密码：关", "Auto-try passwords: off")
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
                                        tr("保存密码提示：开", "Save password prompts: on")
                                    } else {
                                        tr("保存密码提示：关", "Save password prompts: off")
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
                                        tr("空闲一段时间后锁定密码库", "Lock the vault after idle time")
                                    } else {
                                        tr("未设置主密码时不需要锁定密码库", "Vault locking is unnecessary without a master password")
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_auto_lock(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("lock-vault")
                                    .icon(AppIcon::Lock.element(15.))
                                    .label(tr("立即锁定", "Lock now"))
                                    .disabled(!has_master)
                                    .tooltip(if has_master {
                                        tr("立即锁定密码库", "Lock the vault now")
                                    } else {
                                        tr("未设置主密码时不需要锁定密码库", "Vault locking is unnecessary without a master password")
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.vault.has_master_password() {
                                            this.notify(
                                                window,
                                                cx,
                                                NotificationType::Warning,
                                                tr("未设置主密码，密码库不需要锁定", "No master password is set; the vault does not need locking"),
                                            );
                                            return;
                                        }
                                        this.vault.lock();
                                        this.dismiss_password_windows(cx);
                                        this.selected_password = None;
                                        this.notify(
                                            window,
                                            cx,
                                            NotificationType::Success,
                                            tr("密码库已锁定", "Vault locked"),
                                        );
                                    })),
                            )
                            .child(
                                button("vault-transfer")
                                    .icon(AppIcon::Copy.element(15.))
                                    .label(tr("导入 / 导出", "Import / Export"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if input_value(&this.vault_export_dir, cx)
                                            .trim()
                                            .is_empty()
                                        {
                                            let target = dirs::document_dir()
                                                .filter(|path| path.is_dir())
                                                .unwrap_or_else(std::env::temp_dir);
                                            set_input(
                                                &this.vault_export_dir,
                                                target.to_string_lossy().as_ref(),
                                                window,
                                                cx,
                                            );
                                        }
                                        set_input(&this.vault_transfer_unlock, "", window, cx);
                                        this.show_vault_transfer = true;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("vault-clear")
                                    .danger()
                                    .icon(AppIcon::Delete.element(15.))
                                    .label(tr("清除密码库", "Clear vault"))
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
                                        tr("密码库使用 XChaCha20-Poly1305 加密；设置主密码后由 Argon2id 派生密钥保护。", "The vault uses XChaCha20-Poly1305; a master password protects it with an Argon2id-derived key."),
                                    ),
                            ),
                    )
            .into_any_element()
    }
}
