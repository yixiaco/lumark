//! 检查器概要标签：几何、LOD、动画与统计。

use super::*;

impl AssetViewerWindowView {
    /// 场景概要与几何统计。
    pub(super) fn render_overview(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut content = v_flex().gap_3().p_3().child(self.overview_basic(cx));
        if let Some(lods) = self.static_lods()
            && lods.len() > 1
        {
            content = content.child(self.overview_lods(lods, cx));
        }
        if self.has_animation() {
            content = content.child(self.overview_animation(cx));
        }
        if let ViewerScene::Material { viewer } = self.scene.as_ref() {
            content = content.child(self.overview_material(viewer, cx));
        }
        content.into_any_element()
    }

    /// 名称/类型/几何/骨骼/材质/包围盒统计。
    fn overview_basic(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
                &mesh.triangle_count().to_string(),
            ));
            rows.push(inspector_row(
                cx,
                tr("Section", "Sections"),
                &format!("{} / {}", mesh.sections.len(), self.materials().len()),
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
        if let Some((min, max)) = self.lod_bounds.get(self.lod_index).copied().flatten() {
            // UE 单位是 cm；FModel 的 Bounds 以米显示。
            let size = [
                (max[0] - min[0]) * 0.01,
                (max[1] - min[1]) * 0.01,
                (max[2] - min[2]) * 0.01,
            ];
            rows.push(inspector_row(
                cx,
                tr("包围盒", "Bounds"),
                &format!("{:.2} × {:.2} × {:.2} m", size[0], size[1], size[2]),
            ));
        }
        panel_section(
            cx,
            tr("概要", "Overview"),
            v_flex().gap_1().children(rows).into_any_element(),
        )
    }

    /// LOD 选择列表。
    fn overview_lods(&self, lods: &[MeshLod], cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut list = v_flex().gap_1();
        for (index, lod) in lods.iter().enumerate() {
            list = list.child(
                button(format!("model-inspector-lod-{index}"))
                    .ghost()
                    .selected(index == self.lod_index)
                    .label(format!(
                        "LOD {index} · {} 顶点 · {} 三角形",
                        lod.positions.len(),
                        lod.triangle_count()
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lod_index = index;
                        this.reset_section_visibility();
                        this.request_preview(cx);
                        cx.notify();
                    })),
            );
        }
        panel_section(cx, tr("LOD", "LODs"), list.into_any_element())
    }

    /// 动画信息。
    fn overview_animation(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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

    /// 材质资产查看器的外观参数。
    fn overview_material(
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
                    "贴图：基础色 {} · 法线 {} · 高光遮罩 {} · AO {}",
                    viewer.base_color.is_some(),
                    viewer.normal.is_some(),
                    viewer.metallic_roughness.is_some(),
                    viewer.occlusion.is_some()
                )),
        ]);
        panel_section(cx, tr("材质", "Material"), list.into_any_element())
    }

    /// 骨骼数量（Skeleton/SkeletalMesh）。
    fn inspector_bone_count(&self) -> Option<usize> {
        self.skeleton().map(|skeleton| skeleton.bones.len())
    }
}
