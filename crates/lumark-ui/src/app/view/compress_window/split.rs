//! 压缩窗口分卷大小输入和预设。

use super::*;
use crate::i18n::tr;

impl CompressWindowView {
    /// 渲染分卷大小组合控件：数值输入、常用大小下拉和单位下拉。
    ///
    /// 数值框保留手动输入能力，可以直接写 `100M`、`4.38G`；下拉箭头只负责
    /// 填入常用数值并同步单位，不会锁住输入框。显式后缀在解析时优先于单位
    /// 下拉，两种输入方式不会互相覆盖。
    pub(super) fn split_size_field(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let current = parse_size_in_unit(&input_value(&self.split, cx), self.split_unit);
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                InputGroup::new("classic-split-group")
                    .flex_1()
                    .min_w_0()
                    .input(
                        InputGroupInput::new(&self.split)
                            .id("classic-split")
                            .aria_label(tr("分卷大小数值", "Volume size value")),
                    )
                    .addon(
                        InputGroupAddon::new("classic-split-preset-addon")
                            .align(InputGroupAddonAlignment::InlineEnd)
                            .child(
                                InputGroupButton::new("classic-split-presets")
                                    .cursor_pointer()
                                    .accessibility_label(tr("常用分卷大小", "Common volume sizes"))
                                    .tooltip(tr(
                                        "从常用大小中选择，也可以直接输入",
                                        "Choose a common size or type one directly",
                                    ))
                                    .dropdown_caret(true)
                                    .dropdown_menu_with_anchor(
                                        gpui_kit::Anchor::TopRight,
                                        move |menu, _, _| {
                                            SPLIT_PRESETS.iter().fold(menu, |menu, preset| {
                                                let view = view.clone();
                                                let amount = preset.amount;
                                                let unit = preset.unit;
                                                menu.item(
                                                    PopupMenuItem::new(split_preset_label(preset))
                                                        .checked(current == Some(preset.bytes()))
                                                        .on_click(move |_, window, cx| {
                                                            view.update(cx, |this, cx| {
                                                                this.apply_split_preset(
                                                                    amount, unit, window, cx,
                                                                );
                                                            })
                                                            .ok();
                                                        }),
                                                )
                                            })
                                        },
                                    ),
                            ),
                    ),
            )
            .child(choice_select(
                "compress-split-unit",
                &self.split_unit_select,
                92.,
                tr("分卷大小单位", "Volume size unit"),
            ))
    }

    /// 把常用分卷预设写入数值输入框，并同步单位下拉。
    pub(super) fn apply_split_preset(
        &mut self,
        amount: &str,
        unit: SizeUnit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set_input(&self.split, amount, window, cx);
        self.split_unit = unit;
        self.app.update(cx, |app, _| app.compress_split_unit = unit);
        self.split_unit_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(unit.index())), window, cx);
        });
        cx.notify();
    }

    /// 生成分卷提示：单位换算结果实时可见，并说明留空和手动输入的语义。
    pub(super) fn split_hint(&self, cx: &Context<Self>) -> String {
        let value = input_value(&self.split, cx);
        match parse_size_in_unit(&value, self.split_unit) {
            Some(bytes) => {
                format!(
                    "{}{}{}",
                    tr("当前分卷大小：", "Current volume size: "),
                    format_size_precise(bytes),
                    tr("；留空表示不分卷。", "; leave empty for no volumes.")
                )
            }
            None if value.trim().is_empty() => {
                tr("留空表示不分卷；最小 64KB，可从下拉选择或输入 100M、2G。", "Leave empty for no volumes; minimum 64 KB. Choose a preset or type e.g. 100M or 2G.").to_string()
            }
            None => tr("无法识别分卷大小；请输入数字或 100M、2G 等写法。", "Unrecognized volume size; enter a number or e.g. 100M or 2G.").to_string(),
        }
    }
}

/// 返回分卷预设的当前语言标签。
fn split_preset_label(preset: &SplitPreset) -> &'static str {
    match (preset.amount, preset.unit) {
        ("512", SizeUnit::Kilobyte) => tr("512KB - 论坛附件 小", "512KB - Forum attachment small"),
        ("1", SizeUnit::Megabyte) => tr("1MB - 论坛附件 中", "1MB - Forum attachment medium"),
        ("2", SizeUnit::Megabyte) => tr("2MB - 论坛附件 大", "2MB - Forum attachment large"),
        ("100", SizeUnit::Megabyte) => tr("100MB - 网盘", "100MB - Cloud storage"),
        ("700", SizeUnit::Megabyte) => tr("700MB - CD刻录", "700MB - CD"),
        ("4.38", SizeUnit::Gigabyte) => tr("4.38GB - DVD刻录", "4.38GB - DVD"),
        ("7.96", SizeUnit::Gigabyte) => tr("7.96GB - 双层DVD刻录", "7.96GB - Dual-layer DVD"),
        _ => preset.label,
    }
}
