use super::*;

impl AssetViewerWindowView {
    pub(super) fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(26.))
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(match &self.scene {
                ViewerScene::Mesh { lods } => {
                    let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                    match mesh {
                        Some(mesh) => format!(
                            "LOD {}/{} · {} {} · {} {}",
                            self.lod_index + 1,
                            lods.len(),
                            mesh.positions.len(),
                            tr("顶点", "vertices"),
                            mesh.triangle_count(),
                            tr("三角形", "triangles")
                        ),
                        None => tr("无 LOD", "No LOD").to_string(),
                    }
                }
                ViewerScene::Skeleton(skeleton) => {
                    format!("{} {}", skeleton.bones.len(), tr("根骨骼", "bones"))
                }
                ViewerScene::SkeletalMesh { lods, skeleton, .. } => {
                    let mesh = lods.get(self.lod_index).or_else(|| lods.first());
                    match mesh {
                        Some(mesh) => format!(
                            "LOD {}/{} · {} {} · {} {} · {} {}",
                            self.lod_index + 1,
                            lods.len(),
                            mesh.positions.len(),
                            tr("顶点", "vertices"),
                            mesh.triangle_count(),
                            tr("三角形", "triangles"),
                            skeleton.bones.len(),
                            tr("根骨骼", "bones")
                        ),
                        None => tr("无 LOD", "No LOD").to_string(),
                    }
                }
                ViewerScene::Material { viewer } => {
                    let textures = [
                        viewer.base_color.is_some(),
                        viewer.normal.is_some(),
                        viewer.metallic_roughness.is_some(),
                    ]
                    .into_iter()
                    .filter(|present| *present)
                    .count();
                    format!(
                        "{} · {} {}",
                        tr("近似 PBR 材质球", "Approximate PBR material ball"),
                        textures,
                        tr("张贴图", "textures")
                    )
                }
            })
            .child(tr("左键拖拽旋转", "Drag to orbit").to_string())
            .children(
                self.last_backend
                    .as_ref()
                    .map(|backend| div().child(format!("· {backend}"))),
            )
            .child(format!("{}%", this_zoom_percent(self.zoom)))
    }
}

pub(super) fn this_zoom_percent(zoom: f32) -> u32 {
    (zoom * 100.0).round() as u32
}
