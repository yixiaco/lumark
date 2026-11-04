//! 侧栏详细信息检查器。

use super::*;
use crate::i18n::tr;
use gpui_kit::AnyElement;
use gpui_kit::component::button::{Button, ButtonVariants};

impl LumarkApp {
    /// 渲染侧栏下半部分的信息检查器。
    ///
    /// 选中项目时显示大小、时间、路径和归档条目元数据；未选中时显示
    /// 当前文件夹摘要，对应参考图片中“详细信息”区域的两种状态。
    /// 面板高度由可拖动分栏决定，这里填满所属面板。
    pub(crate) fn render_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_entry();
        let has_selection = selected.is_some();
        let display = self.browser.current_display();
        let folder_name = Path::new(&display)
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
        // 只有未选中时才需要目录摘要；一趟遍历得到数量/总计/压缩后总计，
        // 不再为详情面板克隆整张文件表。
        let summary = (!has_selection).then(|| self.displayed_summary());
        v_flex()
            .id("details-panel")
            .w_full()
            .h_full()
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
                    .bg(cx.theme().secondary)
                    .text_color(cx.theme().secondary_foreground)
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
                        let detail_icon_kind = entry_icon(self, &entry);
                        let icon_color = entry_icon_color(cx, &entry, detail_icon_kind);
                        let kind_text = if entry.is_dir {
                            tr("文件夹", "Folder").to_string()
                        } else if let Some(kind) = self.game_asset_kind(&entry) {
                            kind.label().to_string()
                        } else {
                            entry.kind.clone()
                        };
                        let detail_icon = if let Some(icon) = system_icon_for(self, &entry) {
                            gpui_kit::img(icon).size(px(17.)).into_any_element()
                        } else {
                            detail_icon_kind.element(17.).into_any_element()
                        };
                        let size_text = if entry.is_dir {
                            if let Some(size) = self.folder_sizes.get(&entry.path) {
                                format_size(*size)
                            } else if matches!(
                                &self.browser.location,
                                BrowserLocation::Archive { .. }
                            ) {
                                format_size(entry.size)
                            } else {
                                tr("可计算", "Calculate").to_string()
                            }
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
                        .child(render_details_preview(self, &entry, cx))
                        .child(render_asset_export_action(self, &entry, cx))
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
                            entry.archive_entry.as_ref().filter(|_| !entry.is_dir),
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
                        let summary = summary.unwrap_or_default();
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(StatusTone::Warning.color(cx))
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
                            &format!("{} {}", summary.count, tr("项", "items")),
                        ))
                        .child(detail_row(
                            cx,
                            tr("总计", "Total"),
                            &format_size(summary.total_size),
                        ))
                        .when_some(summary.compressed_size, |this, compressed| {
                            this.child(detail_row(
                                cx,
                                tr("压缩后总计", "Compressed total"),
                                &format_size(compressed),
                            ))
                        })
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

/// 渲染详情检查器中的图片预览槽，包括加载、错误和不可用状态。
/// 游戏容器内 `.uasset` 的导出入口；其它位置返回空元素。
fn render_asset_export_action(
    app: &LumarkApp,
    entry: &BrowserEntry,
    cx: &mut Context<LumarkApp>,
) -> AnyElement {
    let eligible = app.game_session.is_some()
        && matches!(&app.browser.location, BrowserLocation::Archive { .. })
        && !entry.is_dir
        && entry
            .path
            .extension()
            .is_some_and(|extension| extension == "uasset");
    if !eligible {
        return div().into_any_element();
    }
    Button::new("details-export-asset")
        .ghost()
        .label(tr("导出资产…", "Export asset…"))
        .tooltip(tr(
            "选择模型格式、材质旁挂并导出当前资产",
            "Choose model formats and material sidecars to export this asset",
        ))
        .on_click(cx.listener(|this, _, window, cx| {
            this.open_asset_export(window, cx);
        }))
        .into_any_element()
}

fn render_details_preview(
    app: &LumarkApp,
    entry: &BrowserEntry,
    cx: &mut Context<LumarkApp>,
) -> gpui_kit::AnyElement {
    let Some(preview) = app.details_preview.as_ref() else {
        return div().into_any_element();
    };
    if !is_image_name(&entry.name) && !is_ue_package_name(&entry.name) {
        return div().into_any_element();
    }
    match preview {
        DetailsPreview::Loading { .. } => h_flex()
            .id("details-preview")
            .w_full()
            .h(px(84.))
            .gap_2()
            .items_center()
            .justify_center()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(gpui_kit::component::spinner::Spinner::new())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("正在准备预览…", "Preparing preview...")),
            )
            .into_any_element(),
        DetailsPreview::Ready {
            path,
            name,
            subtitle,
            rows,
            ..
        } => {
            let path = path.clone();
            let name = name.clone();
            let subtitle = subtitle.clone();
            let rows = rows.clone();
            v_flex()
                .id("details-preview")
                .w_full()
                .gap_1()
                .child(
                    div()
                        .id("details-preview-image")
                        .w_full()
                        .h(px(146.))
                        .overflow_hidden()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            gpui_kit::img(path.clone())
                                .w_full()
                                .max_h(px(140.))
                                .object_fit(gpui_kit::ObjectFit::Contain),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(scene) = this.details_viewer_scene() {
                                this.open_asset_viewer(scene, name.clone(), window, cx);
                            } else {
                                this.open_image_viewer(path.clone(), name.clone(), window, cx);
                            }
                            cx.notify();
                        })),
                )
                .when_some(subtitle, |this, subtitle| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    )
                })
                .children(
                    rows.into_iter()
                        .map(|(label, value)| detail_row(cx, &label, &value)),
                )
                .into_any_element()
        }
        DetailsPreview::Metadata {
            title,
            subtitle,
            rows,
            timeline_seconds,
            ..
        } => v_flex()
            .w_full()
            .gap_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .p_2()
            .child(div().text_sm().font_semibold().child(title.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle.clone()),
            )
            .children(rows.iter().map(|(label, value)| {
                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .w(px(72.))
                            .flex_shrink_0()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.clone()),
                    )
                    .child(div().flex_1().min_w_0().child(value.clone()))
            }))
            .when_some(*timeline_seconds, |this, seconds| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .w_full()
                                .h(px(6.))
                                .rounded_full()
                                .bg(cx.theme().border)
                                .child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(cx.theme().button_primary),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("0.000 s / {seconds:.3} s")),
                        ),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if timeline_seconds.is_some() {
                        tr(
                            "动画时间轴（只读预览）",
                            "Animation timeline (read-only preview)",
                        )
                    } else {
                        tr(
                            "3D 模型/骨骼查看器可点击缩略图打开",
                            "Click the thumbnail to open the 3D model/skeleton viewer",
                        )
                    }),
            )
            .into_any_element(),
        DetailsPreview::Error { message, .. } => h_flex()
            .w_full()
            .gap_2()
            .items_start()
            .rounded(cx.theme().radius)
            .bg(cx.theme().danger.opacity(0.08))
            .p_2()
            .text_xs()
            .text_color(StatusTone::Danger.color(cx))
            .child(message.clone())
            .into_any_element(),
        DetailsPreview::Unavailable { message, .. } => h_flex()
            .w_full()
            .gap_2()
            .items_start()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .p_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(message.clone())
            .into_any_element(),
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
