//! 查看器标题栏与工具栏。

use gpui_kit::component::Disableable as _;

use super::animation::AnimationTimeline;
use super::status::this_zoom_percent;
use super::*;

impl AssetViewerWindowView {
    /// 标题栏：资产类型图标 + 名称 + 查看器类型。
    pub(super) fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let icon = match self.scene.as_ref() {
            ViewerScene::SkeletalMesh { .. } => AppIcon::AssetSkeletalMesh,
            ViewerScene::Skeleton(_) => AppIcon::AssetSkeleton,
            ViewerScene::Mesh { .. } => AppIcon::AssetStaticMesh,
            ViewerScene::Material { .. } => AppIcon::AssetMaterial,
        };
        TitleBar::new()
            .h(px(38.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .font_semibold()
                            .child(format!("{} · {}", self.name, self.kind_label())),
                    ),
            )
    }

    /// 工具栏：视图、渲染开关、动画、检查器与属性 JSON。
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                button("model-reset")
                    .ghost()
                    .icon(AppIcon::Refresh.element(13.))
                    .label(tr("重置视角", "Reset view"))
                    .tooltip(tr("重置旋转、缩放与平移", "Reset orbit, zoom and pan"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.yaw = 35.0;
                        this.pitch = 25.0;
                        this.zoom = 1.0;
                        this.pan = [0.0, 0.0];
                        this.request_preview(cx);
                        cx.notify();
                    })),
            )
            .child(
                button("model-zoom-out")
                    .ghost()
                    .label("−")
                    .tooltip(tr("缩小", "Zoom out"))
                    .accessibility_label(tr("缩小", "Zoom out"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom / 1.2).clamp(0.2, 6.0);
                        this.request_preview(cx);
                        cx.notify();
                    })),
            )
            .child(
                button("model-zoom-in")
                    .ghost()
                    .label("+")
                    .tooltip(tr("放大", "Zoom in"))
                    .accessibility_label(tr("放大", "Zoom in"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom * 1.2).clamp(0.2, 6.0);
                        this.request_preview(cx);
                        cx.notify();
                    })),
            )
            .child(toolbar_separator(cx))
            .when(self.scene_mesh().is_some(), |this| {
                this.child(
                    button("model-wireframe")
                        .ghost()
                        .selected(self.wireframe)
                        .label(tr("线框", "Wireframe"))
                        .tooltip(tr("线框模式", "Wireframe mode"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.wireframe = !this.wireframe;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-srgb")
                        .ghost()
                        .selected(self.srgb_textures)
                        .label("sRGB")
                        .tooltip(tr(
                            "基础色/自发光贴图 sRGB 采样",
                            "sRGB sampling for base/emissive textures",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.srgb_textures = !this.srgb_textures;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-vertex-colors")
                        .ghost()
                        .selected(self.vertex_colors)
                        .label(tr("顶点色", "Vertex colors"))
                        .tooltip(tr(
                            "把网格顶点色乘到基础色（FModel 默认关闭）",
                            "Multiply base color by vertex colors (off by default in FModel)",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.vertex_colors = !this.vertex_colors;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-shading")
                        .ghost()
                        .label(format!(
                            "{}: {}",
                            tr("着色", "Shading"),
                            self.shading_label()
                        ))
                        .tooltip(tr("切换着色模式", "Cycle shading mode"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shading = match this.shading {
                                ShadingMode::Lit => ShadingMode::Unlit,
                                ShadingMode::Unlit => ShadingMode::Normals,
                                ShadingMode::Normals => ShadingMode::Lit,
                            };
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-grid-toggle")
                        .ghost()
                        .selected(self.show_grid)
                        .label(tr("网格", "Grid"))
                        .tooltip(tr("显示/隐藏地面网格", "Toggle ground grid"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_grid = !this.show_grid;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-effects-toggle")
                        .ghost()
                        .selected(self.snooper_effects)
                        .label(tr("效果", "Effects"))
                        .tooltip(tr("开关Snooper环境效果", "Toggle Snooper effects"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.snooper_effects = !this.snooper_effects;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-resolution-toggle")
                        .ghost()
                        .label(self.render_resolution.label())
                        .tooltip(tr("切换查看器渲染分辨率", "Cycle render resolution"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.render_resolution = this.render_resolution.next();
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
            })
            .when(self.has_skin(), |this| {
                this.child(
                    button("model-bones-toggle")
                        .ghost()
                        .selected(self.show_bones)
                        .label(tr("骨骼线", "Bones"))
                        .tooltip(tr("显示/隐藏骨骼线段", "Toggle bone lines"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_bones = !this.show_bones;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
                .child(
                    button("model-skin-pose")
                        .ghost()
                        .selected(self.skin_pose)
                        .label(tr("蒙皮演示", "Skin pose"))
                        .tooltip(tr(
                            "对主要蒙皮骨骼施加局部旋转，演示 LBS 蒙皮",
                            "Rotate the dominant skinned bone to preview LBS deformation",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.skin_pose = !this.skin_pose;
                            this.request_preview(cx);
                            cx.notify();
                        })),
                )
            })
            .children(
                self.has_animation()
                    .then(|| self.render_animation_controls(cx)),
            )
            .child(toolbar_separator(cx))
            .child(
                button("model-properties")
                    .ghost()
                    .icon(AppIcon::Doc.element(13.))
                    .label(tr("属性 JSON", "Properties JSON"))
                    .tooltip(tr(
                        "查看资产导出表与属性树 JSON",
                        "View export table and property tree JSON",
                    ))
                    .disabled(self.properties_json.is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_properties_viewer(window, cx);
                    })),
            )
            .child(
                button("model-inspector-toggle")
                    .ghost()
                    .selected(self.inspector_open)
                    .label(tr("检查器", "Inspector"))
                    .tooltip(tr(
                        "显示/隐藏场景检查器",
                        "Show or hide the scene inspector",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inspector_open = !this.inspector_open;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{}%", this_zoom_percent(self.zoom))),
            )
    }

    /// 动画播放/步进/时间轴。
    fn render_animation_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (length, total) = self.animation_info().unwrap_or((0.0, 1));
        let timeline = AnimationTimeline::new(length, total, self.animation_time);
        let label = if self.animation_playing {
            tr("暂停动画", "Pause animation")
        } else {
            tr("播放动画", "Play animation")
        };
        h_flex()
            .gap_1()
            .items_center()
            .child(
                button("model-animation")
                    .ghost()
                    .selected(self.animation_playing)
                    .label(label)
                    .tooltip(tr(
                        "播放/暂停当前 AnimSequence",
                        "Play/pause the current AnimSequence",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_animation(cx);
                    })),
            )
            // 这些是固定高度工具栏里的纯图标按钮，按项目规范允许 xsmall。
            .child(
                button("model-animation-start")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Refresh.element(12.))
                    .tooltip(tr("回到开头", "Go to start"))
                    .accessibility_label(tr("回到动画开头", "Go to animation start"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_animation(cx);
                    })),
            )
            .child(
                button("model-animation-prev")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::ArrowLeft.element(12.))
                    .tooltip(tr("上一帧", "Previous frame"))
                    .accessibility_label(tr("上一帧", "Previous frame"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step_animation(-1, cx);
                    })),
            )
            .child(
                button("model-animation-next")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::ArrowRight.element(12.))
                    .tooltip(tr("下一帧", "Next frame"))
                    .accessibility_label(tr("下一帧", "Next frame"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step_animation(1, cx);
                    })),
            )
            .child(
                div().id("model-animation-slider").w(px(140.)).children(
                    self.timeline_slider
                        .as_ref()
                        .map(|slider| Slider::new(slider).horizontal().w_full()),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(timeline.label()),
            )
    }

    /// 着色模式显示名。
    fn shading_label(&self) -> &'static str {
        match self.shading {
            ShadingMode::Lit => tr("光照", "Lit"),
            ShadingMode::Unlit => tr("无光", "Unlit"),
            ShadingMode::Normals => tr("法线", "Normals"),
        }
    }
}

/// 工具栏分组竖线。
fn toolbar_separator(cx: &Context<AssetViewerWindowView>) -> impl IntoElement {
    div().w(px(1.)).h(px(20.)).mx_1().bg(cx.theme().border)
}
