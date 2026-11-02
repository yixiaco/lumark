//! 查看器右侧检查器：场景概要、LOD、section、骨骼、动画与材质信息。

use gpui_kit::component::scroll::ScrollableElement as _;

use super::*;

impl AssetViewerWindowView {
    /// 渲染右侧检查器面板。
    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut content = v_flex().gap_2().p_2().child(self.inspector_overview(cx));
        if let Some(lods) = self.static_lods()
            && lods.len() > 1
        {
            content = content.child(self.inspector_lods(lods, cx));
        }
        if !self.hidden_sections.is_empty() {
            content = content.child(self.inspector_sections(cx));
        }
        if let Some(bones) = self.inspector_bones(cx) {
            content = content.child(bones);
        }
        if self.has_animation() {
            content = content.child(self.inspector_animation(cx));
        }
        if let ViewerScene::Material { viewer } = &self.scene {
            content = content.child(self.inspector_material(viewer, cx));
        }
        v_flex()
            .id("model-inspector")
            .w(px(248.))
            .h_full()
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

    /// 场景概要与几何统计。
    fn inspector_overview(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut rows = vec![
            inspector_row(cx, tr("名称", "Name"), &self.name),
            inspector_row(cx, tr("类型", "Type"), self.kind_label()),
        ];
        if let Some(mesh) = self.scene_mesh() {
            rows.push(inspector_row(
                cx,
                tr("LOD", "LOD"),
                &format!(
                    "{}/{}",
                    self.lod_index + 1,
                    self.static_lods().map_or(1, <[MeshLod]>::len)
                ),
            ));
            rows.push(inspector_row(
                cx,
                tr("顶点", "Vertices"),
                &mesh.positions.len().to_string(),
            ));
            rows.push(inspector_row(
                cx,
                tr("三角形", "Triangles"),
                &mesh.indices.len().div_ceil(3).to_string(),
            ));
            rows.push(inspector_row(
                cx,
                tr("Section", "Sections"),
                &mesh.sections.len().to_string(),
            ));
        }
        if let Some(bones) = self.inspector_bone_count() {
            rows.push(inspector_row(cx, tr("骨骼", "Bones"), &bones.to_string()));
        }
        if let Some((length, frames)) = self.animation_info() {
            rows.push(inspector_row(
                cx,
                tr("动画", "Animation"),
                &format!("{length:.2}s · {frames} 帧"),
            ));
        }
        if let Some(stats) = self.last_stats {
            rows.push(inspector_row(
                cx,
                tr("可见实例", "Visible instances"),
                &format!("{} / {}", stats.mesh_instances, stats.unique_meshes),
            ));
            rows.push(inspector_row(
                cx,
                tr("绘制批次", "Draw batches"),
                &stats.draw_calls.to_string(),
            ));
        }
        panel_section(
            cx,
            tr("概要", "Overview"),
            v_flex().gap_1().children(rows).into_any_element(),
        )
    }

    /// LOD 选择列表。
    fn inspector_lods(&self, lods: &[MeshLod], cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut list = v_flex().gap_1();
        for (index, lod) in lods.iter().enumerate() {
            list = list.child(
                button(format!("model-inspector-lod-{index}"))
                    .xsmall()
                    .ghost()
                    .selected(index == self.lod_index)
                    .label(format!(
                        "LOD {index} · {} 顶点 · {} 三角形",
                        lod.positions.len(),
                        lod.indices.len() / 3
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lod_index = index;
                        this.hidden_sections =
                            vec![false; this.scene_mesh().map_or(0, |mesh| mesh.sections.len())];
                        this.render_preview();
                        cx.notify();
                    })),
            );
        }
        panel_section(cx, tr("LOD", "LODs"), list.into_any_element())
    }

    /// section/材质槽可见性。
    fn inspector_sections(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut list = v_flex().gap_1();
        for (index, hidden) in self.hidden_sections.iter().enumerate() {
            let title = self
                .scene_mesh()
                .and_then(|mesh| mesh.sections.get(index))
                .map(|section| {
                    let material_name = usize::try_from(section.material_index)
                        .ok()
                        .and_then(|index| self.material_names.get(index))
                        .filter(|name| !name.is_empty());
                    match material_name {
                        Some(name) => format!("S{index} · {name}"),
                        None => format!("S{index} · 材质槽 {}", section.material_index),
                    }
                })
                .unwrap_or_else(|| format!("S{index}"));
            list = list.child(
                button(format!("model-inspector-section-{index}"))
                    .xsmall()
                    .ghost()
                    .selected(!hidden)
                    .label(if *hidden {
                        format!("{title} · 隐藏")
                    } else {
                        title
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(hidden) = this.hidden_sections.get_mut(index) {
                            *hidden = !*hidden;
                        }
                        this.render_preview();
                        cx.notify();
                    })),
            );
        }
        panel_section(cx, tr("Section", "Sections"), list.into_any_element())
    }

    /// 骨骼列表（支持名称搜索，最多 200 根）。
    fn inspector_bones(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let skeleton = match &self.scene {
            ViewerScene::Skeleton(skeleton) => skeleton,
            ViewerScene::SkeletalMesh { skeleton, .. } => skeleton,
            _ => return None,
        };
        let query = self
            .bone_filter
            .as_ref()
            .map(|input| input.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let matched: Vec<_> = skeleton
            .bones
            .iter()
            .filter(|bone| bone_matches(&bone.name, &query))
            .collect();
        let mut list = v_flex().gap_1();
        if let Some(input) = &self.bone_filter {
            list = list.child(
                Input::new(input)
                    .id("viewer-bone-filter")
                    .w_full()
                    .cleanable(true),
            );
        }
        for bone in matched.iter().take(200) {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · 父 {}", bone.name, bone.parent_index)),
            );
        }
        if matched.len() > 200 {
            list = list.child(
                div()
                    .text_xs()
                    .child(format!("… 共 {} 根匹配骨骼", matched.len())),
            );
        }
        if !query.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().primary)
                    .child(format!("匹配 {} / {}", matched.len(), skeleton.bones.len())),
            );
        }
        Some(panel_section(
            cx,
            tr("骨骼", "Bones"),
            list.into_any_element(),
        ))
    }

    /// 动画信息。
    fn inspector_animation(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let (length, frames) = self.animation_info().unwrap_or((0.0, 1));
        let list = v_flex().gap_1().children([
            div()
                .text_xs()
                .child(format!("时长 {length:.2}s · {frames} 帧")),
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "时间 {:.2}s · {}",
                    self.animation_time,
                    if self.animation_playing {
                        tr("播放中", "Playing")
                    } else {
                        tr("暂停", "Paused")
                    }
                )),
        ]);
        panel_section(cx, tr("动画", "Animation"), list.into_any_element())
    }

    /// 材质参数与贴图。
    fn inspector_material(
        &self,
        viewer: &lumark_unreal::MaterialViewerData,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let appearance = &viewer.appearance;
        let list = v_flex().gap_1().children([
            div().text_xs().child(format!(
                "基础色 ({:.2}, {:.2}, {:.2})",
                appearance.base_color[0], appearance.base_color[1], appearance.base_color[2]
            )),
            div().text_xs().child(format!(
                "金属度 {:.2} · 粗糙度 {:.2}",
                appearance.metallic, appearance.roughness
            )),
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "贴图：基础色 {} · 法线 {} · ORM {}",
                    viewer.base_color.is_some(),
                    viewer.normal.is_some(),
                    viewer.metallic_roughness.is_some()
                )),
        ]);
        panel_section(cx, tr("材质", "Material"), list.into_any_element())
    }

    /// 骨骼数量（Skeleton/SkeletalMesh）。
    fn inspector_bone_count(&self) -> Option<usize> {
        match &self.scene {
            ViewerScene::Skeleton(skeleton) => Some(skeleton.bones.len()),
            ViewerScene::SkeletalMesh { skeleton, .. } => Some(skeleton.bones.len()),
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
                .w(px(54.))
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
