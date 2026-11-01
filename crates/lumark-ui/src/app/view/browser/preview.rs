//! 详情面板图片预览状态和加载。
//!
//! 预览只在选中图片时触发：文件系统直接交给 GPUI 解码，归档条目在后台
//! 解压到窗口临时目录；选择变化时用 key 丢弃过期结果，不阻塞渲染。

use std::path::PathBuf;

use super::preview_ue::{UePreviewResult, load_ue_asset_preview};
use super::*;
use lumark_archive::{ArchiveService, ExtractionOptions};
use uuid::Uuid;
use zeroize::Zeroizing;

/// 详情面板预览状态。
#[derive(Clone)]
pub(crate) enum DetailsPreview {
    /// 正在准备图片。
    Loading { key: String },
    /// 图片已就绪。
    Ready {
        key: String,
        path: PathBuf,
        name: String,
        /// 附加说明（例如 UE 纹理格式和尺寸）。
        subtitle: Option<String>,
        /// 图片之外的只读字段行（例如旧式包属性）。
        rows: Vec<(String, String)>,
    },
    /// 预览失败。
    Error { key: String, message: String },
    /// 资产元数据（动画/骨骼/静态网格），暂无 3D 查看器。
    Metadata {
        key: String,
        title: String,
        subtitle: String,
        rows: Vec<(String, String)>,
        /// 动画时长（秒），用于详情面板时间轴。
        timeline_seconds: Option<f32>,
    },
    /// 当前条目不支持预览。
    Unavailable { key: String, message: String },
}

impl DetailsPreview {
    /// 返回预览对应的选择 key。
    pub(crate) fn key(&self) -> &str {
        match self {
            Self::Loading { key }
            | Self::Ready { key, .. }
            | Self::Error { key, .. }
            | Self::Metadata { key, .. }
            | Self::Unavailable { key, .. } => key,
        }
    }
}

impl LumarkApp {
    /// 在渲染前同步选中条目和预览状态，必要时启动后台解压。
    ///
    /// 预览解析结果同时携带查看器场景数据（网格/骨骼/材质）；渲染期间会
    /// 重新同步预览，若预览 key 未变则恢复这些缓存，避免双击时退回图片查看器。
    pub(crate) fn sync_details_preview(&mut self, cx: &mut Context<Self>) {
        let cached_key = self
            .details_preview
            .as_ref()
            .map(|preview| preview.key().to_string());
        let cached_mesh = self.details_mesh.take();
        let cached_skeleton = self.details_skeleton.take();
        let cached_material = self.details_material.take();
        self.sync_details_preview_inner(cx);
        if self
            .details_preview
            .as_ref()
            .map(|preview| preview.key().to_string())
            == cached_key
        {
            self.details_mesh = cached_mesh;
            self.details_skeleton = cached_skeleton;
            self.details_material = cached_material;
        }
    }

    /// 重新同步详情预览的内部实现。
    fn sync_details_preview_inner(&mut self, cx: &mut Context<Self>) {
        self.clear_stale_pending_asset_viewer();
        let Some(entry) = self.selected_entry() else {
            self.details_preview = None;
            self.details_mesh = None;
            self.details_skeleton = None;
            self.details_material = None;
            return;
        };
        self.details_mesh = None;
        self.details_skeleton = None;
        self.details_material = None;
        if !entry.is_dir
            && self.game_session.is_some()
            && is_ue_package_name(&entry.name)
            && matches!(self.browser.location, BrowserLocation::Archive { .. })
        {
            self.sync_ue_asset_preview(&entry, cx);
            return;
        }
        if entry.is_dir || !is_image_name(&entry.name) {
            self.details_preview = None;
            return;
        }
        match self.browser.location.clone() {
            BrowserLocation::FileSystem(_) => {
                let key = format!("file:{}:{}", entry.path.display(), entry.size);
                if self
                    .details_preview
                    .as_ref()
                    .is_some_and(|preview| preview.key() == key)
                {
                    return;
                }
                self.details_preview = Some(DetailsPreview::Ready {
                    key,
                    path: entry.path.clone(),
                    name: entry.name.clone(),
                    subtitle: None,
                    rows: Vec::new(),
                });
            }
            BrowserLocation::Archive { archive, .. } => {
                let password = self.archive_operation_password();
                let password_present = password.is_some();
                let key = format!(
                    "archive:{}:{}:{}:{password_present}",
                    archive.display(),
                    entry.path.display(),
                    entry.size
                );
                if self
                    .details_preview
                    .as_ref()
                    .is_some_and(|preview| preview.key() == key)
                {
                    return;
                }
                if entry.size > 64 * 1024 * 1024 {
                    self.details_preview = Some(DetailsPreview::Unavailable {
                        key,
                        message: tr(
                            "图片超过 64 MB，不在详情面板中解码",
                            "Image exceeds 64 MB; not decoded in the details panel",
                        )
                        .to_string(),
                    });
                    return;
                }
                if self.browser.archive_encrypted() && !password_present {
                    self.details_preview = Some(DetailsPreview::Unavailable {
                        key,
                        message: tr(
                            "加密条目：解锁密码后可预览",
                            "Encrypted entry: unlock it to preview",
                        )
                        .to_string(),
                    });
                    return;
                }
                self.details_preview = Some(DetailsPreview::Loading { key: key.clone() });
                let output = self
                    .temp_root
                    .join("preview")
                    .join(Uuid::new_v4().to_string());
                let relative = entry.path.to_string_lossy().replace('\\', "/");
                let target = match lumark_archive::safe_join(&output, &relative) {
                    Ok(path) => path,
                    Err(error) => {
                        self.details_preview = Some(DetailsPreview::Error {
                            key,
                            message: error.to_string(),
                        });
                        return;
                    }
                };
                let archive_for_task = archive.clone();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let options = ExtractionOptions {
                                output_dir: output,
                                selected_paths: vec![relative],
                                overwrite: true,
                                password,
                                ..Default::default()
                            };
                            ArchiveService::extract(&archive_for_task, &options, |_| {})
                                .map(|_| target)
                                .map_err(|error| error.to_string())
                        })
                        .await;
                    let _ = this.update(cx, |app, cx| {
                        if app
                            .details_preview
                            .as_ref()
                            .is_none_or(|preview| preview.key() != key)
                        {
                            return;
                        }
                        match result {
                            Ok(path) => {
                                app.details_preview = Some(DetailsPreview::Ready {
                                    key,
                                    path,
                                    name: entry.name.clone(),
                                    subtitle: None,
                                    rows: Vec::new(),
                                });
                            }
                            Err(error) => {
                                app.details_preview = Some(DetailsPreview::Error {
                                    key,
                                    message: error,
                                });
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            BrowserLocation::GameArchives { .. } => {
                self.details_preview = None;
            }
        }
    }

    /// 为游戏容器中的 `.uasset` 加载纹理缩略图或资产元数据。
    fn sync_ue_asset_preview(&mut self, entry: &BrowserEntry, cx: &mut Context<Self>) {
        let Some(session) = self.game_session.as_ref() else {
            return;
        };
        let BrowserLocation::Archive { archive, .. } = &self.browser.location else {
            return;
        };
        let archive = archive.clone();
        let extension = archive
            .extension()
            .and_then(|value| value.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(extension.as_str(), "utoc" | "pak") {
            self.details_preview = Some(DetailsPreview::Unavailable {
                key: format!("ue:{}:{}", archive.display(), entry.path.display()),
                message: tr(
                    "资产查看暂只支持 IoStore (.utoc) 与 PAK (.pak) 容器",
                    "Asset viewing currently supports IoStore (.utoc) and PAK (.pak) containers",
                )
                .to_string(),
            });
            return;
        }
        // global.utoc 用于解析类名；PAK 游戏可能没有 IoStore 全局容器。
        let global = archive
            .parent()
            .map(|parent| parent.join("global.utoc"))
            .unwrap_or_default();
        let mappings_path = session.mappings.clone();
        let oodle = session.oodle.clone();
        let aes_key: Option<Zeroizing<String>> = session
            .key
            .as_ref()
            .map(|key| Zeroizing::new(key.to_string()));
        let key = Self::ue_preview_key(&archive, entry, mappings_path.as_deref());
        if self
            .details_preview
            .as_ref()
            .is_some_and(|preview| preview.key() == key)
        {
            return;
        }
        self.details_preview = Some(DetailsPreview::Loading { key: key.clone() });
        let output = self
            .temp_root
            .join("preview")
            .join(format!("ue-{}.png", Uuid::new_v4()));
        let relative = entry.path.to_string_lossy().replace('\\', "/");
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    load_ue_asset_preview(
                        &archive,
                        &global,
                        &relative,
                        mappings_path.as_deref(),
                        oodle,
                        aes_key.as_ref().map(|key| key.as_str()),
                        &output,
                    )
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app
                    .details_preview
                    .as_ref()
                    .is_none_or(|preview| preview.key() != key)
                {
                    return;
                }
                app.details_preview = Some(match result {
                    UePreviewResult::Texture {
                        path,
                        name,
                        subtitle,
                        rows,
                    } => DetailsPreview::Ready {
                        key,
                        path,
                        name,
                        subtitle: Some(subtitle),
                        rows,
                    },
                    UePreviewResult::Material {
                        path,
                        name,
                        subtitle,
                        rows,
                        viewer,
                    } => {
                        app.details_material = viewer;
                        DetailsPreview::Ready {
                            key,
                            path,
                            name,
                            subtitle: Some(subtitle),
                            rows,
                        }
                    }
                    UePreviewResult::Other { name, class_name } => DetailsPreview::Unavailable {
                        key,
                        message: format!(
                            "{}（{}）暂不支持内置预览；纹理/模型/骨骼可在详情面板打开查看器",
                            name, class_name
                        ),
                    },
                    UePreviewResult::Model {
                        path,
                        name,
                        subtitle,
                        mesh,
                        lods,
                        skeleton,
                        skins,
                        rows,
                    } => {
                        app.details_mesh = Some(mesh);
                        app.details_lods = lods;
                        app.details_skeleton = skeleton;
                        app.details_skins = skins;
                        DetailsPreview::Ready {
                            key,
                            path,
                            name,
                            subtitle: Some(subtitle),
                            rows,
                        }
                    }
                    UePreviewResult::Skeleton {
                        path,
                        name,
                        subtitle,
                        skeleton,
                        rows,
                    } => {
                        app.details_skeleton = Some(skeleton);
                        DetailsPreview::Ready {
                            key,
                            path,
                            name,
                            subtitle: Some(subtitle),
                            rows,
                        }
                    }
                    UePreviewResult::Metadata {
                        name,
                        class_name,
                        rows,
                        timeline_seconds,
                        animation,
                    } => {
                        app.details_animation = animation;
                        DetailsPreview::Metadata {
                            key,
                            title: name,
                            subtitle: class_name,
                            rows,
                            timeline_seconds,
                        }
                    }
                    UePreviewResult::Unavailable(message) => {
                        DetailsPreview::Unavailable { key, message }
                    }
                    UePreviewResult::Error(message) => DetailsPreview::Error { key, message },
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// 选择变化时丢弃已经过期的资产查看器请求，避免旧请求在重新选中时误开。
    fn clear_stale_pending_asset_viewer(&mut self) {
        let Some(pending) = self.pending_asset_viewer.as_ref() else {
            return;
        };
        let target = self.selected_entry().filter(|entry| !entry.is_dir).zip(
            match self.browser.location.clone() {
                BrowserLocation::Archive { archive, .. } => Some(archive),
                _ => None,
            },
        );
        let Some((entry, archive)) = target else {
            self.pending_asset_viewer = None;
            return;
        };
        let mappings = self
            .game_session
            .as_ref()
            .and_then(|session| session.mappings.clone());
        if Self::ue_preview_key(&archive, &entry, mappings.as_deref()) != pending.key {
            self.pending_asset_viewer = None;
        }
    }

    /// 计算 UE 资产详情预览的缓存 key；双击排队和后台解析必须使用同一口径。
    fn ue_preview_key(
        archive: &std::path::Path,
        entry: &BrowserEntry,
        mappings: Option<&std::path::Path>,
    ) -> String {
        format!(
            "ue:{}:{}:{}:{:?}",
            archive.display(),
            entry.path.display(),
            entry.size,
            mappings
        )
    }

    /// 打开游戏容器内所选资产的内置查看器。
    ///
    /// 双击和右键“使用内置查看器打开”共用这个入口：已经解析出场景或纹理时
    /// 立即打开，仍在后台解析时挂起，由渲染后的
    /// [`LumarkApp::flush_pending_asset_viewer`] 打开，两个入口不再给出不同提示。
    pub(crate) fn open_ue_asset_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.status = tr("请先选择一个资产", "Select an asset first").to_string();
            return;
        };
        if entry.is_dir {
            self.status = tr("请选择一个资产文件", "Select an asset file").to_string();
            return;
        }
        if let Some(scene) = self.details_viewer_scene() {
            self.pending_asset_viewer = None;
            self.open_asset_viewer(scene, entry.name.clone(), window, cx);
            return;
        }
        if let Some(DetailsPreview::Ready { path, name, .. }) = self.details_preview.as_ref() {
            let path = path.clone();
            let name = name.clone();
            self.pending_asset_viewer = None;
            self.open_image_viewer(path, name, window, cx);
            return;
        }
        let BrowserLocation::Archive { archive, .. } = self.browser.location.clone() else {
            return;
        };
        let mappings = self
            .game_session
            .as_ref()
            .and_then(|session| session.mappings.clone());
        let key = Self::ue_preview_key(&archive, &entry, mappings.as_deref());
        self.pending_asset_viewer = Some(PendingAssetViewer { key });
        self.status = tr(
            "正在解析资产，完成后会自动打开内置查看器",
            "Parsing the asset; the built-in viewer will open when ready",
        )
        .to_string();
        cx.notify();
    }

    /// 判断挂起的资产查看器是否已经可以打开；供渲染结束后延迟调用。
    pub(crate) fn pending_asset_viewer_ready(&self) -> bool {
        let Some(pending) = self.pending_asset_viewer.as_ref() else {
            return false;
        };
        self.details_preview.as_ref().is_some_and(|preview| {
            preview.key() == pending.key && !matches!(preview, DetailsPreview::Loading { .. })
        })
    }

    /// 在渲染后推进挂起的资产查看器请求。
    ///
    /// 选择变化、预览失败或该资产没有可用查看器时丢弃请求，避免后台结果
    /// 落到别的资产上或让状态栏一直停在“正在解析”。
    pub(crate) fn flush_pending_asset_viewer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_asset_viewer.clone() else {
            return;
        };
        let Some(entry) = self.selected_entry().filter(|entry| !entry.is_dir) else {
            self.pending_asset_viewer = None;
            return;
        };
        let BrowserLocation::Archive { archive, .. } = self.browser.location.clone() else {
            self.pending_asset_viewer = None;
            return;
        };
        let mappings = self
            .game_session
            .as_ref()
            .and_then(|session| session.mappings.clone());
        if Self::ue_preview_key(&archive, &entry, mappings.as_deref()) != pending.key {
            self.pending_asset_viewer = None;
            return;
        }
        let Some(preview) = self.details_preview.clone() else {
            return;
        };
        if preview.key() != pending.key {
            return;
        }
        self.pending_asset_viewer = None;
        match preview {
            DetailsPreview::Loading { .. } => {
                self.pending_asset_viewer = Some(pending);
            }
            DetailsPreview::Ready { path, name, .. } => {
                if let Some(scene) = self.details_viewer_scene() {
                    self.open_asset_viewer(scene, name, window, cx);
                } else {
                    self.open_image_viewer(path, name, window, cx);
                }
            }
            DetailsPreview::Metadata {
                title, subtitle, ..
            } => {
                self.status = format!(
                    "{}{}（{}）",
                    tr(
                        "暂无可用的 3D/纹理查看器：",
                        "No 3D/texture viewer for this asset: "
                    ),
                    title,
                    subtitle
                );
            }
            DetailsPreview::Error { message, .. } => {
                self.status = format!("{}：{message}", tr("资产解析失败", "Asset parsing failed"));
            }
            DetailsPreview::Unavailable { message, .. } => {
                self.status = message;
            }
        }
        cx.notify();
    }
}
