//! 检查器骨骼标签：层级缩进、搜索与数量统计。

use super::*;

impl AssetViewerWindowView {
    /// 骨骼树（按父索引计算缩进）。
    pub(super) fn render_bones(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(skeleton) = self.skeleton() else {
            return div()
                .p_3()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(tr("当前场景没有骨骼", "No skeleton in this scene"))
                .into_any_element();
        };
        let query = self
            .bone_filter
            .as_ref()
            .map(|input| input.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let depths = bone_depths(skeleton);
        let matched: Vec<_> = skeleton
            .bones
            .iter()
            .enumerate()
            .filter(|(_, bone)| bone_matches(&bone.name, &query))
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
        list = list.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{} {} · {} {}",
                    skeleton.bones.len(),
                    tr("根骨骼", "bones"),
                    matched.len(),
                    tr("匹配", "matched")
                )),
        );
        for (index, bone) in matched.iter().take(500) {
            let depth = depths.get(*index).copied().unwrap_or(0).min(16);
            list = list.child(
                div()
                    .id(format!("model-bone-{index}"))
                    .ml(px(depth as f32 * 10.0))
                    .text_xs()
                    .text_color(if depth == 0 {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(format!("{} · 父 {}", bone.name, bone.parent_index)),
            );
        }
        if matched.len() > 500 {
            list = list.child(
                div()
                    .text_xs()
                    .child(format!("… 共 {} 根匹配骨骼", matched.len())),
            );
        }
        panel_section(cx, tr("骨骼", "Bones"), list.into_any_element())
    }
}

/// 每根骨骼的层级深度；父索引异常时按根处理。
fn bone_depths(skeleton: &lumark_unreal::Skeleton) -> Vec<usize> {
    let mut depths = vec![0usize; skeleton.bones.len()];
    for (index, _) in skeleton.bones.iter().enumerate() {
        let mut depth = 0usize;
        let mut parent = skeleton.bones[index].parent_index;
        // 上限防止异常数据形成环。
        while parent >= 0 && depth < 64 {
            let parent_index = parent as usize;
            if parent_index >= skeleton.bones.len() || parent_index == index {
                break;
            }
            depth += 1;
            parent = skeleton.bones[parent_index].parent_index;
        }
        depths[index] = depth;
    }
    depths
}
