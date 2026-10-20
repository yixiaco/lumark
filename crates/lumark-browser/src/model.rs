use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use globset::GlobBuilder;
use regex::RegexBuilder;
use walkdir::WalkDir;

use lumark_archive::{ArchiveEntry, ArchiveFormat, ArchiveInfo, ArchiveService};

use crate::selection::SelectionModel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserLocation {
    FileSystem(PathBuf),
    Archive { archive: PathBuf, inner: String },
}

#[derive(Debug, Clone)]
pub struct BrowserEntry {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub kind: String,
    pub archive_entry: Option<ArchiveEntry>,
    pub encrypted: bool,
}

impl BrowserEntry {
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
pub struct SearchQuery {
    pub pattern: String,
    pub recursive: bool,
    pub case_sensitive: bool,
    pub mode: SearchMode,
    pub extensions: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub modified_after: Option<DateTime<Utc>>,
    pub modified_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Contains,
    Wildcard,
    Regex,
}

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("无法读取目录：{0}")]
    Io(#[from] std::io::Error),
    #[error("无法打开归档：{0}")]
    Archive(String),
}

pub struct BrowserModel {
    pub location: BrowserLocation,
    pub entries: Vec<BrowserEntry>,
    pub selection: SelectionModel,
    archive_entries: Vec<ArchiveEntry>,
    archive_format: Option<ArchiveFormat>,
    archive_encrypted: bool,
    back_stack: Vec<BrowserLocation>,
    forward_stack: Vec<BrowserLocation>,
}

impl BrowserModel {
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

    pub fn refresh(&mut self) -> Result<(), BrowserError> {
        match &self.location {
            BrowserLocation::FileSystem(path) => self.load_filesystem(path.clone()),
            BrowserLocation::Archive { archive, inner } => {
                self.load_archive_entries(archive.clone(), inner.clone())
            }
        }
    }

    pub fn navigate(&mut self, path: impl Into<PathBuf>) -> Result<(), BrowserError> {
        let next = BrowserLocation::FileSystem(path.into());
        self.record_navigation(next);
        self.selection.clear();
        self.refresh()
    }

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

    pub fn go_parent(&mut self) -> Result<(), BrowserError> {
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
                    let parent = archive
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from("."));
                    self.navigate(parent)
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

    pub fn archive_format(&self) -> Option<ArchiveFormat> {
        self.archive_format
    }

    pub fn archive_encrypted(&self) -> bool {
        self.archive_encrypted
    }

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

    pub fn go_back(&mut self) -> Result<(), BrowserError> {
        let Some(previous) = self.back_stack.pop() else {
            return Ok(());
        };
        self.forward_stack.push(self.location.clone());
        self.location = previous;
        self.selection.clear();
        self.reload_location()
    }

    pub fn go_forward(&mut self) -> Result<(), BrowserError> {
        let Some(next) = self.forward_stack.pop() else {
            return Ok(());
        };
        self.back_stack.push(self.location.clone());
        self.location = next;
        self.selection.clear();
        self.reload_location()
    }

    pub fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    fn reload_location(&mut self) -> Result<(), BrowserError> {
        match &self.location {
            BrowserLocation::FileSystem(path) => self.load_filesystem(path.clone()),
            BrowserLocation::Archive { archive, inner } => {
                self.load_archive_entries(archive.clone(), inner.clone())
            }
        }
    }

    fn record_navigation(&mut self, next: BrowserLocation) {
        if self.location == next {
            return;
        }
        self.back_stack.push(self.location.clone());
        self.forward_stack.clear();
        self.location = next;
    }

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

struct TextMatcher {
    pattern: String,
    case_sensitive: bool,
    mode: SearchMode,
}

impl TextMatcher {
    fn new(query: &SearchQuery) -> Self {
        Self {
            pattern: query.pattern.trim().to_string(),
            case_sensitive: query.case_sensitive,
            mode: query.mode,
        }
    }

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
}
