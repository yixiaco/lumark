//! 查看器右侧检查器：概要、section/材质详情与骨骼树。

use gpui_kit::component::scroll::ScrollableElement as _;

use super::*;

mod bones;
mod material;
mod overview;

impl AssetViewerWindowView {
    /// 渲染右侧检查器面板（可拖拽分栏由窗口布局负责宽度）。
    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let content = match self.inspector_tab {
            InspectorTab::Overview => self.render_overview(cx),
            InspectorTab::Materials => self.render_materials(cx),
            InspectorTab::Bones => self.render_bones(cx),
        };
        v_flex()
            .id("model-inspector")
            .debug_selector(|| "model-inspector".into())
            .w_full()
            .h_full()
            .min_w_0()
            .flex_shrink_0()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h(px(34.))
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_semibold()
                            .child(tr("检查器", "Inspector")),
                    )
                    .child(
                        button("model-inspector-close")
                            .ghost()
                            .label("×")
                            .tooltip(tr("关闭检查器", "Close inspector"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.inspector_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_inspector_tabs(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(content),
            )
            .into_any_element()
    }

    /// 检查器标签行：概要 / 材质 / 骨骼。
    fn render_inspector_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(36.))
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.inspector_tab_button(
                InspectorTab::Overview,
                tr("概要", "Overview"),
                "model-inspector-tab-overview",
                cx,
            ))
            .when(self.scene_mesh().is_some(), |this| {
                this.child(self.inspector_tab_button(
                    InspectorTab::Materials,
                    tr("材质", "Materials"),
                    "model-inspector-tab-materials",
                    cx,
                ))
            })
            .when(self.skeleton().is_some(), |this| {
                this.child(self.inspector_tab_button(
                    InspectorTab::Bones,
                    tr("骨骼", "Bones"),
                    "model-inspector-tab-bones",
                    cx,
                ))
            })
    }

    /// 单个标签按钮。
    fn inspector_tab_button(
        &self,
        tab: InspectorTab,
        label: &str,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        button(id)
            .ghost()
            .selected(self.inspector_tab == tab)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.inspector_tab = tab;
                cx.notify();
            }))
    }

    /// 当前场景骨骼；非骨骼场景返回 None。
    fn skeleton(&self) -> Option<&lumark_unreal::Skeleton> {
        match self.scene.as_ref() {
            ViewerScene::Skeleton(skeleton) => Some(skeleton),
            ViewerScene::SkeletalMesh { skeleton, .. } => Some(skeleton),
            _ => None,
        }
    }
}

/// 一行标签 + 值。
fn inspector_row(
    cx: &mut Context<AssetViewerWindowView>,
    label: &str,
    value: &str,
) -> gpui_kit::AnyElement {
    h_flex()
        .gap_2()
        .child(
            div()
                .w(px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .child(value.to_string()),
        )
        .into_any_element()
}

/// 骨骼名称是否匹配搜索词（大小写不敏感）。
pub(super) fn bone_matches(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(query)
}

/// 分节容器。
fn panel_section(
    cx: &mut Context<AssetViewerWindowView>,
    title: &str,
    content: gpui_kit::AnyElement,
) -> gpui_kit::AnyElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(title.to_string()),
        )
        .child(content)
        .into_any_element()
}

/// 小徽标。
fn inspector_badge(
    cx: &mut Context<AssetViewerWindowView>,
    text: &str,
    tone: StatusTone,
) -> gpui_kit::AnyElement {
    div()
        .px(px(6.))
        .py(px(1.))
        .rounded(cx.theme().radius)
        .bg(tone.color(cx).opacity(0.12))
        .text_color(tone.color(cx))
        .text_xs()
        .child(text.to_string())
        .into_any_element()
}

/// 材质贴图用途显示名。
pub(super) fn texture_kind_label(kind: lumark_unreal::MaterialTextureKind) -> &'static str {
    match kind {
        lumark_unreal::MaterialTextureKind::BaseColor => tr("基础色", "Base color"),
        lumark_unreal::MaterialTextureKind::Normal => tr("法线", "Normals"),
        lumark_unreal::MaterialTextureKind::MetallicRoughness => {
            tr("高光遮罩 (G/B)", "Specular mask (G/B)")
        }
        lumark_unreal::MaterialTextureKind::Occlusion => tr("AO", "AO"),
        lumark_unreal::MaterialTextureKind::Emissive => tr("自发光", "Emissive"),
        lumark_unreal::MaterialTextureKind::Mask => tr("遮罩", "Mask"),
        lumark_unreal::MaterialTextureKind::Other => tr("其他", "Other"),
    }
}
