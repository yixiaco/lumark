//! 检查器材质标签：section 列表、可见性与选中材质详情。

use gpui_kit::component::checkbox::Checkbox;

use super::*;

impl AssetViewerWindowView {
    /// 材质标签：section 列表 + 选中材质参数/贴图。
    pub(super) fn render_materials(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(mesh) = self.scene_mesh() else {
            return div()
                .p_3()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(tr("当前场景没有材质 section", "No material sections"))
                .into_any_element();
        };
        let mut list = v_flex().gap_1();
        for (index, section) in mesh.sections.iter().enumerate() {
            let hidden = self.hidden_sections.get(index).copied().unwrap_or(false);
            let material = self.section_material(index);
            let material_name = material
                .map(|material| material.slot.name.clone())
                .or_else(|| {
                    usize::try_from(section.material_index.max(0))
                        .ok()
                        .and_then(|material_index| self.material_names.get(material_index))
                        .cloned()
                })
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| {
                    tr("材质槽", "Material slot").to_string()
                        + &format!(" {}", section.material_index)
                });
            let auto_hidden = self.material_auto_hidden(
                usize::try_from(section.material_index.max(0)).unwrap_or(usize::MAX),
            );
            let selected = index == self.selected_section;
            list = list.child(
                h_flex()
                    .id(format!("model-inspector-section-{index}"))
                    .w_full()
                    .gap_2()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .when(selected, |this| this.bg(cx.theme().secondary))
                    .hover(|style| style.bg(cx.theme().secondary_hover))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_section = index;
                        cx.notify();
                    }))
                    .child(
                        Checkbox::new(format!("model-section-visible-{index}"))
                            .checked(!hidden)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Some(hidden) = this.hidden_sections.get_mut(index) {
                                    *hidden = !*checked;
                                }
                                this.request_preview(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_xs()
                            .child(format!("S{index} · {material_name}")),
                    )
                    .when(auto_hidden, |this| {
                        this.child(inspector_badge(
                            cx,
                            tr("默认隐藏", "Auto hidden"),
                            StatusTone::Warning,
                        ))
                    }),
            );
        }
        // 顶部提供全部显示/按 FModel 默认隐藏。
        let actions = h_flex()
            .gap_1()
            .child(
                button("model-sections-show-all")
                    .ghost()
                    .label(tr("全部显示", "Show all"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hidden_sections
                            .iter_mut()
                            .for_each(|hidden| *hidden = false);
                        this.request_preview(cx);
                        cx.notify();
                    })),
            )
            .child(
                button("model-sections-reset")
                    .ghost()
                    .label(tr("FModel 默认", "FModel default"))
                    .tooltip(tr(
                        "隐藏无贴图/半透明 section（与 FModel 一致）",
                        "Hide texture-less/translucent sections (FModel default)",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_section_visibility();
                        this.request_preview(cx);
                        cx.notify();
                    })),
            );
        let sections = panel_section(
            cx,
            tr("Section / 材质槽", "Sections / material slots"),
            v_flex()
                .gap_1()
                .child(actions)
                .child(div().max_h(px(220.)).overflow_y_scrollbar().child(list))
                .into_any_element(),
        );
        v_flex()
            .gap_3()
            .p_3()
            .child(sections)
            .child(self.render_material_details(cx))
            .into_any_element()
    }

    /// 选中 section 的材质详情。
    fn render_material_details(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(material) = self.section_material(self.selected_section) else {
            return panel_section(
                cx,
                tr("材质", "Material"),
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(
                        "该 section 未解析到材质（可尝试在详情面板重新解析）",
                        "No resolved material for this section",
                    ))
                    .into_any_element(),
            );
        };
        let appearance = material.appearance.as_ref();
        let mut rows = vec![inspector_row(cx, tr("槽位", "Slot"), &material.slot.name)];
        if let Some(path) = material.package_path.as_ref() {
            rows.push(inspector_row(cx, tr("材质包", "Package"), path));
        }
        if let Some(appearance) = appearance {
            if !appearance.blend_mode.is_empty() {
                rows.push(inspector_row(
                    cx,
                    tr("混合", "Blend"),
                    &blend_mode_label(&appearance.blend_mode),
                ));
            }
            if !appearance.shading_model.is_empty() {
                rows.push(inspector_row(
                    cx,
                    tr("着色", "Shading"),
                    &shading_model_label(&appearance.shading_model),
                ));
            }
            rows.push(inspector_row(
                cx,
                tr("双面", "Two sided"),
                if appearance.two_sided {
                    tr("是", "Yes")
                } else {
                    tr("否", "No")
                },
            ));
            rows.push(inspector_row(
                cx,
                tr("基础色 tint", "Base tint"),
                &format!(
                    "{:.2}, {:.2}, {:.2}",
                    appearance.base_color[0], appearance.base_color[1], appearance.base_color[2]
                ),
            ));
            rows.push(inspector_row(
                cx,
                tr("高光强度", "Specular"),
                &format!("{:.2}", appearance.specular),
            ));
            rows.push(inspector_row(
                cx,
                tr("金属度", "Metallic"),
                &format!("{:.2}", appearance.metallic),
            ));
            rows.push(inspector_row(
                cx,
                tr("粗糙度", "Roughness"),
                &format!(
                    "{:.2} – {:.2}",
                    appearance.roughness_min, appearance.roughness_max
                ),
            ));
            if appearance.emissive_multiplier != 1.0 || appearance.emissive != [0.0; 3] {
                rows.push(inspector_row(
                    cx,
                    tr("自发光", "Emissive"),
                    &format!(
                        "{:.2}, {:.2}, {:.2} × {:.2}",
                        appearance.emissive[0],
                        appearance.emissive[1],
                        appearance.emissive[2],
                        appearance.emissive_multiplier
                    ),
                ));
            }
            if appearance.has_ao_boost {
                rows.push(inspector_row(
                    cx,
                    tr("AO 增强", "AO boost"),
                    &format!(
                        "{:.2}, {:.2}, {:.2} ^ {:.2}",
                        appearance.ao_boost_color[0],
                        appearance.ao_boost_color[1],
                        appearance.ao_boost_color[2],
                        appearance.ao_boost_exponent
                    ),
                ));
            }
        }
        let mut details = v_flex().gap_1().children(rows);
        if let Some(viewer) = material.viewer.as_ref() {
            let mut textures = v_flex().gap_1();
            for info in &viewer.texture_infos {
                let size = if info.width > 0 && info.height > 0 {
                    format!("{}×{}", info.width, info.height)
                } else {
                    tr("未解码", "Not decoded").to_string()
                };
                textures = textures.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .w(px(82.))
                                .flex_shrink_0()
                                .text_color(cx.theme().muted_foreground)
                                .child(texture_kind_label(info.kind)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(info.name.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(cx.theme().muted_foreground)
                                .child(size),
                        ),
                );
            }
            if !viewer.texture_infos.is_empty() {
                details = details.child(
                    div()
                        .mt_2()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("贴图", "Textures")),
                );
                details = details.child(textures);
            }
        }
        if !material.loaded_assets.is_empty() {
            details = details.child(
                div()
                    .mt_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("已加载资源", "Loaded assets")),
            );
            for asset in material.loaded_assets.iter().take(8) {
                details = details.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(asset.clone()),
                );
            }
        }
        if !material.warnings.is_empty() {
            details = details.child(
                div()
                    .mt_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(StatusTone::Warning.color(cx))
                    .child(tr("警告", "Warnings")),
            );
            for warning in &material.warnings {
                details = details.child(
                    div()
                        .text_xs()
                        .text_color(StatusTone::Warning.color(cx))
                        .child(warning.clone()),
                );
            }
        }
        panel_section(cx, tr("材质", "Material"), details.into_any_element())
    }
}

/// 混合模式显示名。
fn blend_mode_label(raw: &str) -> String {
    raw.trim_start_matches("BLEND_").replace('_', " ")
}

/// 着色模型显示名。
fn shading_model_label(raw: &str) -> String {
    raw.trim_start_matches("MSM_").replace('_', " ")
}
