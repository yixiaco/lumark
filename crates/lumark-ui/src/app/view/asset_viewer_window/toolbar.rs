use super::animation::AnimationTimeline;
use super::status::this_zoom_percent;
use super::*;

impl AssetViewerWindowView {
    pub(super) fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
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
                            .child(AppIcon::Game.element(14.)),
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
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.yaw = 35.0;
                        this.pitch = 25.0;
                        this.zoom = 1.0;
                        this.render_preview();
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
                        this.render_preview();
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
                        this.render_preview();
                        cx.notify();
                    })),
            )
            .child(
                button("model-srgb")
                    .ghost()
                    .xsmall()
                    .label("sRGB")
                    .tooltip(tr(
                        "基础色/自发光贴图 sRGB 采样",
                        "sRGB sampling for base/emissive textures",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.srgb_textures = !this.srgb_textures;
                        this.render_preview();
                        cx.notify();
                    })),
            )
            .children((0..self.hidden_sections.len()).map(|index| {
                let visible = !self.hidden_sections[index];
                let mut section_button = button(format!("model-section-{index}"))
                    .xsmall()
                    .label(format!("S{index}"))
                    .tooltip(tr(
                        "切换 section/材质可见性",
                        "Toggle section/material visibility",
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(hidden) = this.hidden_sections.get_mut(index) {
                            *hidden = !*hidden;
                        }
                        this.render_preview();
                        cx.notify();
                    }));
                if visible {
                    section_button = section_button.primary();
                } else {
                    section_button = section_button.ghost();
                }
                section_button
            }))
            .children(
                self.static_lods()
                    .filter(|lods| lods.len() > 1)
                    .map(|lods| {
                        button("model-lod")
                            .ghost()
                            .label(format!("LOD {}/{}", self.lod_index + 1, lods.len()))
                            .tooltip(tr("切换 LOD", "Switch LOD"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let count = this.static_lods().map_or(0, <[MeshLod]>::len);
                                if count > 1 {
                                    this.lod_index = (this.lod_index + 1) % count;
                                    this.hidden_sections =
                                        vec![
                                            false;
                                            this.scene_mesh().map_or(0, |mesh| mesh.sections.len())
                                        ];
                                    this.render_preview();
                                    cx.notify();
                                }
                            }))
                    }),
            )
            .children(self.has_animation().then(|| {
                let (length, total) = self.animation_info().unwrap_or((0.0, 1));
                let timeline = AnimationTimeline::new(length, total, self.animation_time);
                let label = if self.animation_playing {
                    tr("暂停动画", "Pause animation")
                } else {
                    tr("播放动画", "Play animation")
                };
                let mut play_button = button("model-animation")
                    .ghost()
                    .label(label)
                    .tooltip(tr(
                        "播放/暂停当前 AnimSequence",
                        "Play/pause the current AnimSequence",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_animation(cx);
                    }));
                if self.animation_playing {
                    play_button = play_button.primary();
                }
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(play_button)
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
            }))
            .when(self.has_skin(), |this| {
                let mut pose_button = button("model-skin-pose")
                    .ghost()
                    .label(tr("蒙皮演示", "Skin pose"))
                    .tooltip(tr(
                        "对主要蒙皮骨骼施加局部旋转，演示 LBS 蒙皮",
                        "Rotate the dominant skinned bone to preview LBS deformation",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skin_pose = !this.skin_pose;
                        this.render_preview();
                        cx.notify();
                    }));
                if self.skin_pose {
                    pose_button = pose_button.primary();
                }
                this.child(pose_button)
            })
            .when(self.scene_mesh().is_some(), |this| {
                let shading_label = match self.shading {
                    lumark_render::ShadingMode::Lit => tr("光照", "Lit"),
                    lumark_render::ShadingMode::Unlit => tr("无光", "Unlit"),
                    lumark_render::ShadingMode::Normals => tr("法线", "Normals"),
                };
                this.child(
                    button("model-shading")
                        .ghost()
                        .label(format!("{}: {shading_label}", tr("着色", "Shading")))
                        .tooltip(tr("切换着色模式", "Cycle shading mode"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shading = match this.shading {
                                lumark_render::ShadingMode::Lit => {
                                    lumark_render::ShadingMode::Unlit
                                }
                                lumark_render::ShadingMode::Unlit => {
                                    lumark_render::ShadingMode::Normals
                                }
                                lumark_render::ShadingMode::Normals => {
                                    lumark_render::ShadingMode::Lit
                                }
                            };
                            this.render_preview();
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
                            this.render_preview();
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
                            this.render_preview();
                            cx.notify();
                        })),
                )
            })
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
                    .child(format!("{}%", (this_zoom_percent(self.zoom)))),
            )
    }
}
