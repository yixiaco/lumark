use std::path::{Component, Path, PathBuf};

use crate::error::ArchiveError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchivePath {
    components: Vec<String>,
}

impl ArchivePath {
    pub fn parse(value: &str) -> Result<Self, ArchiveError> {
        let normalized = value.replace('\\', "/");
        if normalized.is_empty() {
            return Err(ArchiveError::UnsafePath("empty path".into()));
        }
        if normalized.starts_with('/') || normalized.starts_with("//") {
            return Err(ArchiveError::UnsafePath(value.into()));
        }
        let mut components = Vec::new();
        for part in normalized.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err(ArchiveError::UnsafePath(value.into()));
            }
            validate_component(part)?;
            components.push(part.to_string());
        }
        if components.is_empty() {
            return Err(ArchiveError::UnsafePath(value.into()));
        }
        Ok(Self { components })
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn as_relative_path(&self) -> PathBuf {
        let mut path = PathBuf::new();
        for component in &self.components {
            path.push(component);
        }
        path
    }

    pub fn display(&self) -> String {
        self.components.join("/")
    }

    pub fn file_name(&self) -> Option<&str> {
        self.components.last().map(String::as_str)
    }
}

pub fn safe_join(dest: &Path, entry_name: &str) -> Result<PathBuf, ArchiveError> {
    let archive_path = ArchivePath::parse(entry_name)?;
    let mut result = dest.to_path_buf();
    for component in archive_path.components() {
        result.push(component);
    }
    Ok(result)
}

/// Reject writes that would traverse a symlink already present in the target tree.
/// This prevents an archive from escaping through a pre-existing link even when
/// every member path is lexically safe.
pub fn ensure_no_symlink_components(root: &Path, destination: &Path) -> Result<(), ArchiveError> {
    let relative = destination
        .strip_prefix(root)
        .map_err(|_| ArchiveError::UnsafePath(destination.display().to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = std::fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            return Err(ArchiveError::UnsafePath(format!(
                "目标路径包含符号链接：{}",
                current.display()
            )));
        }
    }
    Ok(())
}

fn validate_component(component: &str) -> Result<(), ArchiveError> {
    if component.contains('\0') || component.contains(':') {
        return Err(ArchiveError::UnsafePath(component.into()));
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return Err(ArchiveError::UnsafePath(component.into()));
    }
    let upper = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&upper.as_str()) {
        return Err(ArchiveError::UnsafePath(component.into()));
    }
    if component.starts_with("\\\\?\\") || component.contains("GLOBALROOT") {
        return Err(ArchiveError::UnsafePath(component.into()));
    }
    Ok(())
}

pub fn normalize_path_for_match(value: &str) -> String {
    let normalized = value.replace('/', "\\");
    normalized.trim_end_matches('\\').to_lowercase()
}

pub fn is_archive_extension(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "zip"
            | "7z"
            | "tar"
            | "gz"
            | "tgz"
            | "bz2"
            | "tbz"
            | "tbz2"
            | "xz"
            | "txz"
            | "zst"
            | "tzst"
            | "br"
            | "lz4"
            | "rar"
            | "iso"
            | "udf"
            | "cab"
            | "arj"
            | "ace"
            | "arc"
            | "alz"
            | "lzh"
            | "lha"
            | "cpio"
            | "wim"
            | "swm"
            | "isz"
            | "deb"
            | "asar"
            | "zpaq"
            | "uu"
            | "uue"
            | "xxe"
            | "img"
            | "bin"
            | "enc"
            | "lz"
            | "lzma"
    )
}

pub fn path_depth(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count()
}
