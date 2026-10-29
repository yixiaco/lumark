//! 查看器和嵌套归档共用的路径判定与临时目录所有权。

use std::path::{Path, PathBuf};

use super::*;

/// 由当前窗口拥有、在窗口关闭或转移失败时删除的临时目录。
///
/// 只接受调用方创建的唯一目录；`disarm` 在目录所有权转交给子窗口后调用，
/// 避免两个窗口同时清理同一个目录。
pub(crate) struct OwnedTempDir {
    path: PathBuf,
    armed: bool,
}

impl OwnedTempDir {
    /// 创建 armed 守卫。
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// 转移所有权，之后不再删除目录。
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// 判断归档条目名称是否是压缩包，用于决定是否在新窗口中打开。
///
/// 只按扩展名预判；无扩展名或自解压文件仍交给系统默认应用。
pub(crate) fn is_nested_archive_name(name: &str) -> bool {
    lumark_archive::format::detect_format(Path::new(name)) != ArchiveFormat::Unknown
}

/// 判断名称是否为内置图片查看器支持的图片格式。
///
/// 使用 GPUI `Img` 的扩展名清单，避免维护一份会与解码能力漂移的副本。
pub(crate) fn is_image_name(name: &str) -> bool {
    let Some(extension) = Path::new(name).extension().and_then(|value| value.to_str()) else {
        return false;
    };
    gpui_kit::Img::extensions()
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

/// 识别需要显式打开确认的可执行文件和脚本扩展名。
pub(crate) fn is_executable_like(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            [
                "exe", "com", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "wsh", "msi", "scr",
                "cpl", "hta", "reg",
            ]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 嵌套压缩包按扩展名识别，普通文件不触发新窗口。
    #[test]
    fn nested_archive_names_are_detected_by_extension() {
        for name in [
            "inner.zip",
            "inner.zipx",
            "inner.7z",
            "inner.rar",
            "inner.tar.gz",
            "inner.sz",
            "inner.bz3",
        ] {
            assert!(is_nested_archive_name(name), "{name}");
        }
        for name in ["readme.txt", "photo.png", "setup.exe", "no-extension"] {
            assert!(!is_nested_archive_name(name), "{name}");
        }
    }

    /// 图片查看器识别常见位图和矢量扩展名。
    #[test]
    fn image_names_are_detected_by_extension() {
        for name in ["a.png", "a.JPG", "a.gif", "a.webp", "a.svg", "a.avif"] {
            assert!(is_image_name(name), "{name}");
        }
        for name in ["a.txt", "a.zip", "a.exe"] {
            assert!(!is_image_name(name), "{name}");
        }
    }

    /// 临时目录守卫在 armed 时删除，转移所有权后不再删除。
    #[test]
    fn owned_temp_dir_disarms_before_child_window_takes_over() {
        let directory = tempdir().unwrap();
        let owned = directory.path().join("nested");
        std::fs::create_dir_all(&owned).unwrap();
        {
            let _guard = OwnedTempDir::new(owned.clone());
        }
        assert!(!owned.exists());

        std::fs::create_dir_all(&owned).unwrap();
        {
            let mut guard = OwnedTempDir::new(owned.clone());
            guard.disarm();
        }
        assert!(owned.exists());
    }
}
