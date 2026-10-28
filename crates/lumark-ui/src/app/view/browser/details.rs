//! 侧栏详细信息检查器。

use super::*;
use crate::i18n::tr;

impl LumarkApp {
    /// 渲染侧栏下半部分的信息检查器。
    ///
    /// 选中项目时显示大小、时间、路径和归档条目元数据；未选中时显示
    /// 当前文件夹摘要，对应参考图片中“详细信息”区域的两种状态。
    pub(crate) fn render_details(
        &self,
        max_height: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_entry();
        let has_selection = selected.is_some();
        let display = self.browser.current_display();
        let folder_name = Path::new(&display)
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
        let entry_count = self.displayed_count();
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        v_flex()
            .id("details-panel")
            .w_full()
            .flex_shrink_0()
            .max_h(px(max_height))
            .overflow_hidden()
            .bg(cx.theme().popover)
            .border_t_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_1()
                    .items_center()
                    .bg(cx.theme().sidebar_primary)
                    .text_color(cx.theme().sidebar_foreground)
                    .child(AppIcon::Doc.element(13.))
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .child(tr("详细信息", "Details")),
                    )
                    .child(
                        icon_button(
                            "details-hide",
                            AppIcon::Close,
                            tr("隐藏详细信息", "Hide details"),
                        )
                        .ml_auto()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_details();
                            cx.notify();
                        })),
                    ),
            )
            .child(
                v_flex()
                    .id("details-body")
                    .p_3()
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when_some(selected, |this, entry| {
                        let icon_color = entry_icon_color(cx, &entry);
                        let kind_text = if entry.is_dir {
                            tr("文件夹", "Folder").to_string()
                        } else {
                            entry.kind.clone()
                        };
                        let detail_icon = if let Some(icon) = system_icon_for(self, &entry) {
                            gpui_kit::img(icon).size(px(17.)).into_any_element()
                        } else {
                            entry_icon(&entry).element(17.).into_any_element()
                        };
                        let size_text = if entry.is_dir {
                            self.folder_sizes
                                .get(&entry.path)
                                .map(|size| format_size(*size))
                                .unwrap_or_else(|| tr("可计算", "Calculate").to_string())
                        } else {
                            format_size(entry.size)
                        };
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(icon_color)
                                        .child(detail_icon),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .whitespace_normal()
                                        .child(entry.name.clone()),
                                ),
                        )
                        .child(detail_row(cx, tr("类型", "Type"), &kind_text))
                        .child(detail_row(cx, tr("大小", "Size"), &size_text))
                        .when_some(entry.modified, |this, modified| {
                            this.child(detail_row(
                                cx,
                                tr("修改时间", "Modified"),
                                &modified.format("%Y-%m-%d %H:%M:%S").to_string(),
                            ))
                        })
                        .child(detail_row(
                            cx,
                            tr("位置", "Location"),
                            &entry.path.display().to_string(),
                        ))
                        .when(entry.encrypted, |this| {
                            this.child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .rounded_sm()
                                    .bg(cx.theme().warning)
                                    .text_color(cx.theme().warning_foreground)
                                    .px_2()
                                    .py_1()
                                    .child(AppIcon::FileLock.element(12.))
                                    .child(
                                        div().text_xs().child(tr("加密条目", "Encrypted entry")),
                                    ),
                            )
                        })
                        .when_some(
                            entry.archive_entry.as_ref(),
                            |this, archive| {
                                this.child(div().h(px(1.)).bg(cx.theme().border))
                                    .child(detail_row(
                                        cx,
                                        tr("压缩后", "Compressed"),
                                        &archive
                                            .compressed_size
                                            .map(format_size)
                                            .unwrap_or_else(|| "—".to_string()),
                                    ))
                                    .child(detail_row(
                                        cx,
                                        tr("压缩方法", "Method"),
                                        &archive.method,
                                    ))
                                    .child(detail_row(
                                        cx,
                                        "CRC32",
                                        &archive
                                            .crc32
                                            .map(|value| format!("{value:08X}"))
                                            .unwrap_or_else(|| "—".to_string()),
                                    ))
                                    .child(detail_row(
                                        cx,
                                        tr("加密", "Encrypted"),
                                        if archive.encrypted {
                                            tr("是", "Yes")
                                        } else {
                                            tr("否", "No")
                                        },
                                    ))
                            },
                        )
                    })
                    .when(!has_selection, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(cx.theme().warning_foreground)
                                        .child(AppIcon::Folder.element(17.)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_semibold()
                                        .whitespace_normal()
                                        .child(folder_name),
                                ),
                        )
                        .child(detail_row(cx, tr("类型", "Type"), tr("文件夹", "Folder")))
                        .child(detail_row(
                            cx,
                            "项目",
                            &format!("{entry_count} {}", tr("项", "items")),
                        ))
                        .child(detail_row(
                            cx,
                            tr("总计", "Total"),
                            &format_size(total_size),
                        ))
                        .child(detail_row(cx, tr("位置", "Location"), &display))
                        .child(hint_text(
                            cx,
                            tr(
                                "选择一个项目可查看 CRC 和压缩方法。",
                                "Select an item to view CRC and compression method.",
                            ),
                        ))
                    }),
            )
    }
}

/// 渲染详情检查器中的带标签值，标签固定宽度保证多行值对齐。
pub(super) fn detail_row(cx: &Context<LumarkApp>, label: &str, value: &str) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .gap_2()
        .items_start()
        .child(
            div()
                .w(px(58.))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .whitespace_normal()
                .child(value.to_string()),
        )
}
