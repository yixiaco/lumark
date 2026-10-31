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
    pub(crate) fn sync_details_preview(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.selected_entry() else {
            self.details_preview = None;
            self.details_mesh = None;
            self.details_skeleton = None;
            return;
        };
        self.details_mesh = None;
        self.details_skeleton = None;
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
                let password_present = self.archive_password.is_some();
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
                let password: Option<Zeroizing<String>> = self
                    .archive_password
                    .as_ref()
                    .map(|password| Zeroizing::new(password.to_string()));
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
        let key = format!(
            "ue:{}:{}:{}:{:?}",
            archive.display(),
            entry.path.display(),
            entry.size,
            mappings_path
        );
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
                    UePreviewResult::Other { name, class_name } => DetailsPreview::Unavailable {
                        key,
                        message: format!(
                            "{}（{}）；模型/骨骼/动画查看器将在后续版本提供",
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
}
