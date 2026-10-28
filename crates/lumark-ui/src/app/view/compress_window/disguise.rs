use super::*;

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
            prompt: Some("选择伪装面具文件".into()),
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
            return "把输出包装成可正常播放/查看的媒体文件；内层格式保持不变。".to_string();
        }
        match resolve_disguise_mask(self.disguise_mask, self.disguise_custom.as_deref()) {
            Ok(mask) => format!(
                "面具：{}，输出扩展名 .{}；伪装不加密，只能由 Lumark 打开或还原。",
                mask.display_name(),
                mask.extension().unwrap_or_default()
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
                "伪装输出",
                v_flex()
                    .gap_3()
                    .child(
                        Checkbox::new("classic-disguise")
                            .label("伪装为媒体文件")
                            .checked(self.disguise)
                            .on_click(cx.listener(|this, value: &bool, window, cx| {
                                this.set_disguise(*value, window, cx);
                            })),
                    )
                    .child(classic_field(
                        "面具:",
                        92.,
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(choice_select(
                                "compress-disguise-mask",
                                &self.disguise_select,
                                200.,
                                "伪装面具",
                            ))
                            .child(
                                button("classic-disguise-choose-mask")
                                    .small()
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .label("选择面具文件…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_disguise_mask(window, cx);
                                    })),
                            ),
                    ))
                    .child(hint_text(cx, self.disguise_hint())),
            ))
            .child(classic_group(
                cx,
                "说明",
                v_flex().gap_2().child(hint_text(
                    cx,
                    "伪装只改变文件外观，不提供加密；载荷经过混淆，只能由 Lumark 直接打开或一键还原。内层归档格式保持不变。",
                )),
            ))
            .into_any_element()
    }
}
