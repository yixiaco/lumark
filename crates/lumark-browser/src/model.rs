//! 文件系统和归档共用的浏览器导航与搜索模型。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use globset::GlobBuilder;
use regex::RegexBuilder;
use walkdir::WalkDir;

use lumark_archive::{ArchiveEntry, ArchiveFormat, ArchiveInfo, ArchiveService};

use crate::selection::SelectionModel;

#[derive(Debug, Clone, PartialEq, Eq)]
/// 浏览器当前所在位置：真实文件系统目录或归档内部路径。
pub enum BrowserLocation {
    /// 真实文件系统目录。
    FileSystem(PathBuf),
    /// 归档内部路径。
    Archive { archive: PathBuf, inner: String },
}

#[derive(Debug, Clone)]
/// 文件或归档条目在列表中的统一表示。
pub struct BrowserEntry {
    /// 条目稳定 ID。
    pub id: String,
    /// 展示名称。
    pub name: String,
    /// 真实路径或归档内路径。
    pub path: PathBuf,
    /// 是否为目录。
    pub is_dir: bool,
    /// 原始大小。
    pub size: u64,
    /// 修改时间。
    pub modified: Option<DateTime<Utc>>,
    /// 文件类型或扩展名。
    pub kind: String,
    /// 是否为归档内条目。
    pub archive_entry: Option<ArchiveEntry>,
    /// 条目是否加密。
    pub encrypted: bool,
}

impl BrowserEntry {
    /// 把归档条目转换为浏览器条目。
    pub fn from_archive(entry: ArchiveEntry) -> Self {
        Self {
            id: format!("archive:{}", entry.path),
            name: entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.path)
                .to_string(),
            path: PathBuf::from(entry.path.clone()),
            is_dir: entry.is_dir,
            size: entry.size,
            modified: entry.modified,
            kind: entry.method.clone(),
            encrypted: entry.encrypted,
            archive_entry: Some(entry),
        }
    }
}

#[derive(Debug, Clone, Default)]
/// 一次搜索的文本和过滤条件。
pub struct SearchQuery {
    /// 搜索文本。
    pub pattern: String,
    /// 是否递归搜索。
    pub recursive: bool,
    /// 是否区分大小写。
    pub case_sensitive: bool,
    /// 文本匹配方式。
    pub mode: SearchMode,
    /// 扩展名过滤。
    pub extensions: Vec<String>,
    /// 最小字节数。
    pub min_size: Option<u64>,
    /// 最大字节数。
    pub max_size: Option<u64>,
    /// 修改时间下界。
    pub modified_after: Option<DateTime<Utc>>,
    /// 修改时间上界。
    pub modified_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 搜索文本的匹配方式。
pub enum SearchMode {
    #[default]
    Contains,
    /// 通配符匹配。
    Wildcard,
    /// 正则匹配。
    Regex,
}

#[derive(Debug, thiserror::Error)]
/// 浏览器导航和归档加载错误。
pub enum BrowserError {
    #[error("无法读取目录：{0}")]
    Io(#[from] std::io::Error),
    #[error("无法打开归档：{0}")]
    Archive(String),
}

/// 文件系统与归档共享的目录导航和搜索状态。
pub struct BrowserModel {
    /// 当前位置。
    pub location: BrowserLocation,
    /// 当前目录条目。
    pub entries: Vec<BrowserEntry>,
    /// 选择状态。
    pub selection: SelectionModel,
    archive_entries: Vec<ArchiveEntry>,
    archive_format: Option<ArchiveFormat>,
    archive_encrypted: bool,
    back_stack: Vec<BrowserLocation>,
    forward_stack: Vec<BrowserLocation>,
}

impl BrowserModel {
    /// 创建从指定目录开始的浏览器模型。
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, BrowserError> {
        let path = path.into();
        let mut model = Self {
            location: BrowserLocation::FileSystem(path),
            entries: Vec::new(),
            selection: SelectionModel::default(),
            archive_entries: Vec::new(),
            archive_format: None,
            archive_encrypted: false,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        };
        model.refresh()?;
        Ok(model)
    }

    /// 重新加载当前位置并保留可恢复的历史。
    pub fn refresh(&mut self) -> Result<(), BrowserError> {
        match &self.location {
            BrowserLocation::FileSystem(path) => self.load_filesystem(path.clone()),
            BrowserLocation::Archive { archive, inner } => {
                self.load_archive_entries(archive.clone(), inner.clone())
            }
        }
    }

    /// 导航到新的文件系统目录。
    pub fn navigate(&mut self, path: impl Into<PathBuf>) -> Result<(), BrowserError> {
        let next = BrowserLocation::FileSystem(path.into());
        self.record_navigation(next);
        self.selection.clear();
        self.refresh()
    }

    /// 打开归档并进入其根目录。
    pub fn open_archive(
        &mut self,
        archive: impl Into<PathBuf>,
        password: Option<&str>,
    ) -> Result<(), BrowserError> {
        let archive = archive.into();
        let info = ArchiveService::list(&archive, password)
            .map_err(|error| BrowserError::Archive(error.to_string()))?;
        self.open_archive_info(archive, info)
    }

    /// 使用已加载的归档信息进入归档。
    pub fn open_archive_info(
        &mut self,
        archive: impl Into<PathBuf>,
        info: ArchiveInfo,
    ) -> Result<(), BrowserError> {
        let archive = archive.into();
        self.archive_format = info.format;
        self.archive_encrypted = info.encrypted;
        self.archive_entries = info.entries;
        let next = BrowserLocation::Archive {
            archive,
            inner: String::new(),
        };
        self.record_navigation(next);
        self.selection.clear();
        self.load_archive_entries(
            match &self.location {
                BrowserLocation::Archive { archive, .. } => archive.clone(),
                _ => unreachable!(),
            },
            String::new(),
        )
    }

    /// 进入选中目录或归档条目。
    pub fn enter_entry(&mut self, id: &str) -> Result<(), BrowserError> {
        let Some(entry) = self.entries.iter().find(|entry| entry.id == id).cloned() else {
            return Ok(());
        };
        if !entry.is_dir {
            return Ok(());
        }
        match &self.location {
            BrowserLocation::FileSystem(path) => {
                let target = path.join(&entry.name);
                self.navigate(target)
            }
            BrowserLocation::Archive { archive, inner } => {
                let next_inner = if inner.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{inner}/{}", entry.name)
                };
                let archive = archive.clone();
                let next = BrowserLocation::Archive {
                    archive: archive.clone(),
                    inner: next_inner.clone(),
                };
                self.record_navigation(next);
                self.selection.clear();
                self.load_archive_entries(archive, next_inner)
            }
        }
    }

    /// 判断当前位置是否还能返回上一级。
    ///
    /// 文件系统以是否存在父目录为准；归档内任意子目录都能回到归档根目录，
    /// 而归档根目录只有在压缩包本身位于某个目录下时才能跳出归档。
    pub fn can_go_up(&self) -> bool {
        has_parent(&self.location)
    }

    /// 返回当前位置的父级。
    pub fn go_parent(&mut self) -> Result<(), BrowserError> {
        if !self.can_go_up() {
            return Ok(());
        }
        match &self.location {
            BrowserLocation::FileSystem(path) => {
                let Some(parent) = path.parent() else {
                    return Ok(());
                };
                self.navigate(parent.to_path_buf())
            }
            BrowserLocation::Archive { archive, inner } => {
                let archive = archive.clone();
                if inner.is_empty() {
                    let Some(parent) = archive.parent() else {
                        return Ok(());
                    };
                    self.navigate(parent.to_path_buf())
                } else {
                    let next_inner = inner
                        .rsplit_once('/')
                        .map(|(parent, _)| parent.to_string())
                        .unwrap_or_default();
                    let next = BrowserLocation::Archive {
                        archive: archive.clone(),
                        inner: next_inner.clone(),
                    };
                    self.record_navigation(next);
                    self.selection.clear();
                    self.load_archive_entries(archive, next_inner)
                }
            }
        }
    }

    /// 在当前目录或归档中执行搜索。
    pub fn search(&self, query: &SearchQuery) -> Vec<BrowserEntry> {
        if query.pattern.trim().is_empty()
            && query.extensions.is_empty()
            && query.min_size.is_none()
            && query.max_size.is_none()
            && query.modified_after.is_none()
            && query.modified_before.is_none()
        {
            return Vec::new();
        }
        let matcher = TextMatcher::new(query);
        match &self.location {
            BrowserLocation::FileSystem(path) => {
                let walker = if query.recursive {
                    WalkDir::new(path)
                } else {
                    WalkDir::new(path).max_depth(1)
                };
                walker
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let metadata = entry.metadata().ok()?;
                        let modified = metadata.modified().ok().map(Into::into);
                        if !matcher.matches(&name)
                            || !filters_match(
                                query,
                                &name,
                                metadata.len(),
                                entry.file_type().is_dir(),
                                modified,
                            )
                        {
                            return None;
                        }
                        Some(BrowserEntry {
                            id: entry.path().to_string_lossy().to_string(),
                            name,
                            path: entry.path().to_path_buf(),
                            is_dir: entry.file_type().is_dir(),
                            size: metadata.len(),
                            modified,
                            kind: if entry.file_type().is_dir() {
                                "文件夹".to_string()
                            } else {
                                "文件".to_string()
                            },
                            archive_entry: None,
                            encrypted: false,
                        })
                    })
                    .collect()
            }
            BrowserLocation::Archive { .. } => self
                .archive_entries
                .iter()
                .filter_map(|entry| {
                    let name = entry
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&entry.path)
                        .to_string();
                    if matcher.matches(&name)
                        && filters_match(
                            query,
                            &entry.path,
                            entry.size,
                            entry.is_dir,
                            entry.modified,
                        )
                    {
                        Some(BrowserEntry::from_archive(entry.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    /// 返回当前归档格式。
    pub fn archive_format(&self) -> Option<ArchiveFormat> {
        self.archive_format
    }

    /// 返回当前归档是否加密。
    pub fn archive_encrypted(&self) -> bool {
        self.archive_encrypted
    }

    /// 返回用于路径栏展示的当前位置文本。
    pub fn current_display(&self) -> String {
        match &self.location {
            BrowserLocation::FileSystem(path) => path.display().to_string(),
            BrowserLocation::Archive { archive, inner } => {
                if inner.is_empty() {
                    archive.display().to_string()
                } else {
                    format!("{} > {}", archive.display(), inner)
                }
            }
        }
    }

    /// 返回上一个历史位置。
    pub fn go_back(&mut self) -> Result<(), BrowserError> {
        let Some(previous) = self.back_stack.pop() else {
            return Ok(());
        };
        self.forward_stack.push(self.location.clone());
        self.location = previous;
        self.selection.clear();
        self.reload_location()
    }

    /// 前进到下一个历史位置。
    pub fn go_forward(&mut self) -> Result<(), BrowserError> {
        let Some(next) = self.forward_stack.pop() else {
            return Ok(());
        };
        self.back_stack.push(self.location.clone());
        self.location = next;
        self.selection.clear();
        self.reload_location()
    }

    /// 判断是否可以后退。
    pub fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    /// 判断是否可以前进。
    pub fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    /// 按当前位置类型重新加载内容。
    fn reload_location(&mut self) -> Result<(), BrowserError> {
        match &self.location {
            BrowserLocation::FileSystem(path) => self.load_filesystem(path.clone()),
            BrowserLocation::Archive { archive, inner } => {
                self.load_archive_entries(archive.clone(), inner.clone())
            }
        }
    }

    /// 记录目标位置到前进/后退历史。
    fn record_navigation(&mut self, next: BrowserLocation) {
        if self.location == next {
            return;
        }
        self.back_stack.push(self.location.clone());
        self.forward_stack.clear();
        self.location = next;
    }

    /// 加载真实目录条目并按当前排序排列。
    fn load_filesystem(&mut self, path: PathBuf) -> Result<(), BrowserError> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let is_dir = metadata.is_dir();
            let name = entry.file_name().to_string_lossy().to_string();
            entries.push(BrowserEntry {
                id: entry.path().to_string_lossy().to_string(),
                name,
                path: entry.path(),
                is_dir,
                size: metadata.len(),
                modified: metadata.modified().ok().map(Into::into),
                kind: if is_dir {
                    "文件夹".to_string()
                } else {
                    file_kind(&entry.path())
                },
                archive_entry: None,
                encrypted: false,
            });
        }
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| natord::compare(&left.name, &right.name))
        });
        self.entries = entries;
        Ok(())
    }

    /// 加载当前归档路径下的条目。
    fn load_archive_entries(
        &mut self,
        _archive: PathBuf,
        inner: String,
    ) -> Result<(), BrowserError> {
        let prefix = if inner.is_empty() {
            String::new()
        } else {
            format!("{inner}/")
        };
        let mut items: BTreeMap<String, BrowserEntry> = BTreeMap::new();
        for entry in &self.archive_entries {
            let normalized = entry.path.replace('\\', "/");
            let Some(remainder) = normalized.strip_prefix(&prefix) else {
                continue;
            };
            if remainder.is_empty() {
                continue;
            }
            let first = remainder.split('/').next().unwrap_or(remainder);
            let is_nested = remainder.contains('/');
            let key = first.to_string();
            if is_nested || entry.is_dir {
                items.entry(key.clone()).or_insert_with(|| BrowserEntry {
                    id: format!("archive-dir:{prefix}{key}"),
                    name: key,
                    path: PathBuf::from(format!(
                        "{prefix}{}",
                        remainder.split('/').next().unwrap_or("")
                    )),
                    is_dir: true,
                    size: 0,
                    modified: entry.modified,
                    kind: "文件夹".to_string(),
                    archive_entry: None,
                    encrypted: entry.encrypted,
                });
            } else {
                items.insert(key.clone(), BrowserEntry::from_archive(entry.clone()));
            }
        }
        let mut entries: Vec<_> = items.into_values().collect();
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| natord::compare(&left.name, &right.name))
        });
        self.entries = entries;
        Ok(())
    }
}

/// 判断浏览位置是否存在可导航的上一级。
///
/// 归档内部总是能回到上一层的归档目录；归档根目录则取决于压缩包所在的
/// 文件夹，离开归档后浏览的是文件系统位置。
fn has_parent(location: &BrowserLocation) -> bool {
    match location {
        BrowserLocation::FileSystem(path) => navigable_parent(path).is_some(),
        BrowserLocation::Archive { archive, inner } => {
            !inner.is_empty() || navigable_parent(archive).is_some()
        }
    }
}

/// 返回非空的父目录。
///
/// 相对路径（例如仅传文件名打开归档）的父目录是空路径，读取它会失败，
/// 因此不视为可导航目标。
fn navigable_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

/// 把搜索文本编译为通配符或正则匹配器。
struct TextMatcher {
    pattern: String,
    case_sensitive: bool,
    mode: SearchMode,
}

impl TextMatcher {
    /// 创建从指定目录开始的浏览器模型。
    fn new(query: &SearchQuery) -> Self {
        Self {
            pattern: query.pattern.trim().to_string(),
            case_sensitive: query.case_sensitive,
            mode: query.mode,
        }
    }

    /// 判断条目名称是否匹配搜索条件。
    fn matches(&self, value: &str) -> bool {
        if self.pattern.is_empty() {
            return true;
        }
        match self.mode {
            SearchMode::Contains => {
                if self.case_sensitive {
                    value.contains(&self.pattern)
                } else {
                    value.to_lowercase().contains(&self.pattern.to_lowercase())
                }
            }
            SearchMode::Wildcard => GlobBuilder::new(&self.pattern)
                .case_insensitive(!self.case_sensitive)
                .literal_separator(false)
                .build()
                .map(|glob| glob.compile_matcher().is_match(value))
                .unwrap_or(false),
            SearchMode::Regex => RegexBuilder::new(&self.pattern)
                .case_insensitive(!self.case_sensitive)
                .build()
                .map(|regex| regex.is_match(value))
                .unwrap_or(false),
        }
    }
}

/// 判断扩展名、类型和大小过滤是否通过。
fn filters_match(
    query: &SearchQuery,
    name: &str,
    size: u64,
    is_dir: bool,
    modified: Option<DateTime<Utc>>,
) -> bool {
    if !query.extensions.is_empty() && !is_dir {
        let extension = Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !query.extensions.iter().any(|candidate| {
            candidate
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        }) {
            return false;
        }
    }
    if query.min_size.is_some_and(|minimum| size < minimum) {
        return false;
    }
    if query.max_size.is_some_and(|maximum| size > maximum) {
        return false;
    }
    if let Some(after) = query.modified_after
        && modified.is_none_or(|value| value < after)
    {
        return false;
    }
    if let Some(before) = query.modified_before
        && modified.is_none_or(|value| value > before)
    {
        return false;
    }
    true
}

/// 按扩展名返回用于筛选的类型名称。
fn file_kind(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| format!("{} 文件", extension.to_ascii_uppercase()))
        .unwrap_or_else(|| "文件".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证通配符和扩展名过滤。
    fn wildcard_and_extension_filters_are_applied() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("report-2026.zip"), b"12345").unwrap();
        std::fs::write(directory.path().join("report-2025.txt"), b"12345").unwrap();
        std::fs::write(directory.path().join("other.zip"), b"1").unwrap();
        let browser = BrowserModel::new(directory.path()).unwrap();
        let results = browser.search(&SearchQuery {
            pattern: "report-*.zip".to_string(),
            mode: SearchMode::Wildcard,
            extensions: vec!["zip".to_string()],
            min_size: Some(4),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "report-2026.zip");
    }

    #[test]
    /// 验证正则搜索默认忽略大小写。
    fn regex_search_is_case_insensitive_by_default() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("Photo.JPG"), b"x").unwrap();
        let browser = BrowserModel::new(directory.path()).unwrap();
        let results = browser.search(&SearchQuery {
            pattern: r"^photo\.jpg$".to_string(),
            mode: SearchMode::Regex,
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
    }

    #[test]
    /// 验证前进后退历史在目录间移动。
    fn history_moves_between_directories() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let mut browser = BrowserModel::new(&first).unwrap();
        browser.navigate(&second).unwrap();
        assert!(browser.can_go_back());
        browser.go_back().unwrap();
        assert_eq!(browser.location, BrowserLocation::FileSystem(first));
        assert!(browser.can_go_forward());
        browser.go_forward().unwrap();
        assert_eq!(browser.location, BrowserLocation::FileSystem(second));
    }

    /// 验证普通目录可以返回，盘符根目录则没有上一级。
    #[test]
    fn parent_navigation_stops_at_drive_root() {
        let directory = tempfile::tempdir().unwrap();
        let browser = BrowserModel::new(directory.path()).unwrap();
        assert!(browser.can_go_up());

        let root = std::env::current_dir()
            .unwrap()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        let mut browser = BrowserModel::new(&root).expect("读取盘符根目录");
        assert!(!browser.can_go_up());
        browser.go_parent().unwrap();
        assert_eq!(browser.location, BrowserLocation::FileSystem(root));
    }

    /// 验证归档内部总能返回，归档根目录只在压缩包有实际父目录时才能跳出。
    #[test]
    fn archive_parent_depends_on_archive_location() {
        let nested = BrowserLocation::Archive {
            archive: PathBuf::from(r"C:\downloads\demo.zip"),
            inner: "docs".to_string(),
        };
        assert!(has_parent(&nested));

        let archive_root = BrowserLocation::Archive {
            archive: PathBuf::from(r"C:\downloads\demo.zip"),
            inner: String::new(),
        };
        assert!(has_parent(&archive_root));

        let relative = BrowserLocation::Archive {
            archive: PathBuf::from("demo.zip"),
            inner: String::new(),
        };
        assert!(!has_parent(&relative));
    }
}
