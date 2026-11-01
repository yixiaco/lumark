//! 平铺视图的图片缩略图缓存。
//!
//! 缩略图只在平铺视图里按需生成：文件系统直接解码原图，归档条目先用归档
//! 服务解压到窗口临时目录；两者都缩放到固定尺寸后写入同一缓存，避免每帧
//! 重复解码。生成失败或超过大小上限时保留文件类型图标。

use std::path::PathBuf;

use super::*;
use lumark_archive::{ArchiveService, ExtractionOptions};
use zeroize::Zeroizing;

/// 平铺缩略图的最大边长，对应 120px 卡片里 48px 的图标区域。
const THUMBNAIL_SIZE: u32 = 96;

/// 允许生成缩略图的单个文件大小上限。
const THUMBNAIL_LIMIT_BYTES: u64 = 48 * 1024 * 1024;

/// 同时生成缩略图的数量上限，避免一次滚动触发过多后台解压。
const THUMBNAIL_CONCURRENCY: usize = 16;

impl LumarkApp {
    /// 返回平铺卡片使用的缩略图路径，缺失时排队后台生成。
    pub(crate) fn tile_thumbnail(
        &self,
        entry: &BrowserEntry,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        if entry.is_dir || !is_image_name(&entry.name) || entry.size > THUMBNAIL_LIMIT_BYTES {
            return None;
        }
        let key = self.thumbnail_key(entry);
        if let Some(cached) = self.tile_thumbnails.borrow().get(&key) {
            return cached.clone();
        }
        if self.tile_thumb_pending.borrow().contains(&key) {
            return None;
        }
        if self.tile_thumb_pending.borrow().len() >= THUMBNAIL_CONCURRENCY {
            return None;
        }
        self.tile_thumb_pending.borrow_mut().insert(key.clone());
        self.spawn_thumbnail(entry.clone(), key, cx);
        None
    }

    /// 计算缩略图缓存 key；同一路径和大小复用同一结果。
    fn thumbnail_key(&self, entry: &BrowserEntry) -> String {
        let location = match &self.browser.location {
            BrowserLocation::Archive { archive, .. } => format!("archive:{}", archive.display()),
            BrowserLocation::FileSystem(_) => "fs".to_string(),
            BrowserLocation::GameArchives { .. } => "archives".to_string(),
        };
        format!("{location}|{}|{}", entry.path.display(), entry.size)
    }

    /// 在后台生成缩略图并写回缓存。
    fn spawn_thumbnail(&self, entry: BrowserEntry, key: String, cx: &mut Context<Self>) {
        let output = self
            .temp_root
            .join("thumbnails")
            .join(format!("{}.png", Uuid::new_v4()));
        let archive = match &self.browser.location {
            BrowserLocation::Archive { archive, .. } => Some(archive.clone()),
            _ => None,
        };
        let password: Option<Zeroizing<String>> = archive
            .is_some()
            .then(|| self.archive_operation_password())
            .flatten();
        cx.spawn(async move |this, cx| {
            let output_for_task = output.clone();
            let result =
                cx.background_executor()
                    .spawn(async move {
                        generate_thumbnail(entry.path, archive, password, output_for_task)
                    })
                    .await;
            let _ = this.update(cx, |app, cx| {
                app.tile_thumb_pending.borrow_mut().remove(&key);
                app.tile_thumbnails.borrow_mut().insert(key, result);
                cx.notify();
            });
        })
        .detach();
    }
}

/// 生成缩略图：归档条目先解压，再缩放保存；失败返回 `None`。
fn generate_thumbnail(
    path: PathBuf,
    archive: Option<PathBuf>,
    password: Option<Zeroizing<String>>,
    output: PathBuf,
) -> Option<PathBuf> {
    if let Some(parent) = output.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    let source = match archive {
        Some(archive) => {
            let relative = path.to_string_lossy().replace('\\', "/");
            let temp = output
                .parent()
                .map(|parent| parent.join(format!("src-{}", Uuid::new_v4())))
                .unwrap_or_else(|| output.clone());
            let target = temp.join(&relative);
            let options = ExtractionOptions {
                output_dir: temp.clone(),
                selected_paths: vec![relative],
                overwrite: true,
                password,
                ..Default::default()
            };
            let extracted = ArchiveService::extract(&archive, &options, |_| {}).is_ok();
            let _ = std::fs::remove_dir_all(&temp);
            if !extracted {
                return None;
            }
            target
        }
        None => path,
    };
    let image = image::open(&source).ok()?;
    let thumbnail = image.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    if thumbnail.save(&output).is_err() {
        return None;
    }
    Some(output)
}
