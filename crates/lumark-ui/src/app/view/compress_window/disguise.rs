//! 压缩窗口伪装选项和提示。

use super::*;
use crate::i18n::tr;

impl CompressWindowView {
    /// 当前面具偏好对应的输出扩展名；无法解析时回退到当前格式扩展名。
    pub(super) fn disguise_output_extension(&self) -> String {
        disguise_extension(self.disguise_mask, self.disguise_custom.as_deref())
            .unwrap_or_else(|| self.format.extension().to_string())
    }

    /// 切换伪装输出，并同步输出扩展名、取消自解压和分卷。
    pub(super) fn set_disguise(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.disguise = enabled;
        if enabled {
            // 自解压与分卷都不能和伪装同时使用；分卷输入框只在经典模式可见。
            self.sfx = false;
            set_input(&self.split, "", window, cx);
        }
        let current = PathBuf::from(input_value(&self.output, cx));
        if !current.as_os_str().is_empty() {
            let replacement = if enabled {
                replace_disguise_extension(&current, &self.disguise_output_extension())
            } else {
                // 关闭伪装时先剥掉当前面具扩展名，避免留下 `photos.mp4.zip`。
                replace_mask_extension(
                    &current,
                    Some(&self.disguise_output_extension()),
                    self.format.extension(),
                )
            };
            set_input(
                &self.output,
                replacement.to_string_lossy().as_ref(),
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// 切换面具类型；伪装已开启时同步输出扩展名。
    pub(super) fn set_disguise_mask(
        &mut self,
        preference: DisguiseMaskPreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 先记录旧面具扩展名，自定义面具可以是任意扩展名，需要显式剥离。
        let previous_extension =
            disguise_extension(self.disguise_mask, self.disguise_custom.as_deref());
        self.disguise_mask = preference;
        let index = disguise_mask_index(preference);
        self.disguise_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(index)), window, cx);
        });
        if self.disguise {
            let current = PathBuf::from(input_value(&self.output, cx));
            if !current.as_os_str().is_empty() {
                let replacement = replace_mask_extension(
                    &current,
                    previous_extension.as_deref(),
                    &self.disguise_output_extension(),
                );
                set_input(
                    &self.output,
                    replacement.to_string_lossy().as_ref(),
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// 选择自定义面具文件；选择成功后自动切换为“自定义文件”。
    pub(super) fn choose_disguise_mask(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("选择伪装面具文件", "Choose disguise mask file").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |view, window, cx| {
                    view.disguise_custom = Some(path);
                    view.set_disguise_mask(DisguiseMaskPreference::Custom, window, cx);
                });
            }
        })
        .detach();
    }

    /// 伪装区域的状态提示。
    pub(super) fn disguise_hint(&self) -> String {
        if !self.disguise {
            return tr(
                "把输出包装成可正常播放/查看的媒体文件；内层格式保持不变。",
                "Wrap output as playable/viewable media; the inner format is unchanged.",
            )
            .to_string();
        }
        match resolve_disguise_mask(self.disguise_mask, self.disguise_custom.as_deref()) {
            Ok(mask) => format!(
                "{}{}{} .{}{}",
                tr("面具：", "Mask: "),
                mask.display_name(),
                tr("，输出扩展名 .", ", output extension ."),
                mask.extension().unwrap_or_default(),
                tr(
                    "；伪装不加密，只能由 Lumark 打开或还原。",
                    "; disguise does not encrypt and can only be opened or restored by Lumark."
                )
            ),
            Err(error) => error,
        }
    }

    /// 渲染“伪装”选项卡：开启开关、面具选择和自定义面具文件。
    pub(super) fn render_disguise_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(classic_group(
                cx,
                tr("伪装输出", "Disguised output"),
                v_flex()
                    .gap_3()
                    .child(
                        Checkbox::new("classic-disguise")
                            .label(tr("伪装为媒体文件", "Disguise as media file"))
                            .checked(self.disguise)
                            .on_click(cx.listener(|this, value: &bool, window, cx| {
                                this.set_disguise(*value, window, cx);
                            })),
                    )
                    .child(classic_field(
                        tr("面具:", "Mask:"),
                        92.,
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(choice_select(
                                "compress-disguise-mask",
                                &self.disguise_select,
                                200.,
                                tr("伪装面具", "Disguise mask"),
                            ))
                            .child(
                                button("classic-disguise-choose-mask")
                                    .small()
                                    .rounded(px(6.))

                                    .label(tr("选择面具文件…", "Choose mask file..."))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_disguise_mask(window, cx);
                                    })),
                            ),
                    ))
                    .child(hint_text(cx, self.disguise_hint())),
            ))
            .child(classic_group(
                cx,
                tr("说明", "Description"),
                v_flex().gap_2().child(hint_text(
                    cx,
                    tr("伪装只改变文件外观，不提供加密；载荷经过混淆，只能由 Lumark 直接打开或一键还原。内层归档格式保持不变。", "Disguise changes only the appearance; it does not encrypt. The obfuscated payload can only be opened or restored by Lumark. The inner archive format is unchanged."),
                )),
            ))
            .into_any_element()
    }
}
