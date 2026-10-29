//! 主窗口弹窗入口和通用弹窗辅助。

use super::super::*;
use super::components::{AppIcon, form_label, hint_text};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;

mod archive_edit;
mod archive_password;
mod fs_edit;
mod password_attempt;
mod temp_extract;
mod vault;

/// 构建“标签 + 输入框”的纵向字段，避免表单只露出无说明的输入框。
fn labeled_input(cx: &gpui_kit::App, label: &str, input: impl IntoElement) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(form_label(cx, label))
        .child(input)
}

/// 返回密码弹窗需要展示的归档或条目名称。
fn password_target_label(action: &PendingPasswordAction) -> String {
    match action {
        PendingPasswordAction::OpenArchive { archive, .. }
        | PendingPasswordAction::ExtractDialog { archive }
        | PendingPasswordAction::Test { archive } => archive.display().to_string(),
        PendingPasswordAction::OpenEntry {
            archive, relative, ..
        } => {
            format!("{} · {relative}", archive.display())
        }
    }
}
