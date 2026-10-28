//! MSI 只读解析：从 Windows Installer 的内嵌 CAB 中读取文件。
//!
//! MSI 是 OLE 复合文档，`File` 表把 CAB 成员名映射到目标文件名，`Media`
//! 表保存内嵌 CAB 的流名。这里只处理内嵌 CAB；外部 CAB 由 MSI 安装时从
//! 媒体路径读取，不适合直接当作归档解压。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use cab::Cabinet;
use msi::{Package, Select};

use crate::error::ArchiveError;

/// 一个 MSI 文件条目。
#[derive(Clone)]
pub(crate) struct MsiEntry {
    /// CAB 内部成员名。
    pub(crate) cab_name: String,
    /// 目标文件名。
    pub(crate) path: String,
    /// 原始大小。
    pub(crate) size: u64,
}

/// MSI 的 CAB 来源。
enum CabSource {
    /// 内嵌在 MSI 中的流。
    Embedded(String),
    /// MSI 同目录的外部 CAB。
    External(PathBuf),
}

/// 已打开的 MSI 包和 CAB 来源。
pub(crate) struct MsiArchive {
    /// MSI 包。
    package: Package<File>,
    /// CAB 来源。
    cab: CabSource,
    /// 条目列表。
    entries: Vec<MsiEntry>,
}

impl MsiArchive {
    /// 打开 MSI 并读取 File/Media 表和 CAB 目录。
    pub(crate) fn open(path: &Path) -> Result<Self, ArchiveError> {
        let mut package = Package::open(File::open(path)?)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI 打开失败：{error}")))?;
        let file_names = read_file_names(&mut package)?;
        let cab = read_cabinet_source(&mut package, path)?;
        let mut entries = Vec::new();
        match &cab {
            CabSource::Embedded(name) => {
                let stream = package.read_stream(name).map_err(|error| {
                    ArchiveError::Corrupt(format!("MSI CAB 流读取失败：{error}"))
                })?;
                let cabinet = Cabinet::new(stream)
                    .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 解析失败：{error}")))?;
                collect_entries(&cabinet, &file_names, &mut entries);
            }
            CabSource::External(path) => {
                let file = File::open(path)?;
                let cabinet = Cabinet::new(file).map_err(|error| {
                    ArchiveError::Corrupt(format!("外部 CAB 解析失败：{error}"))
                })?;
                collect_entries(&cabinet, &file_names, &mut entries);
            }
        }
        Ok(Self {
            package,
            cab,
            entries,
        })
    }

    /// 返回全部条目。
    pub(crate) fn entries(&self) -> &[MsiEntry] {
        &self.entries
    }

    /// 打开指定条目的解压读取器。
    pub(crate) fn open_entry(&mut self, entry: &MsiEntry) -> Result<Box<dyn Read>, ArchiveError> {
        let bytes = match &self.cab {
            CabSource::Embedded(name) => {
                let stream = self.package.read_stream(name).map_err(|error| {
                    ArchiveError::Corrupt(format!("MSI CAB 流读取失败：{error}"))
                })?;
                let mut cabinet = Cabinet::new(stream)
                    .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 解析失败：{error}")))?;
                read_entry(&mut cabinet, entry)?
            }
            CabSource::External(path) => {
                let file = File::open(path)?;
                let mut cabinet = Cabinet::new(file).map_err(|error| {
                    ArchiveError::Corrupt(format!("外部 CAB 解析失败：{error}"))
                })?;
                read_entry(&mut cabinet, entry)?
            }
        };
        Ok(Box::new(Cursor::new(bytes)))
    }
}

/// 读取 File 表的 CAB 成员名到目标路径映射。
///
/// 标准 MSI 通过 `File.Component_ -> Component.Directory_ -> Directory`
/// 还原目录层级；缺少 Component/Directory 表的精简包回退到根目录。
fn read_file_names(package: &mut Package<File>) -> Result<HashMap<String, String>, ArchiveError> {
    let Some((key_index, name_index, component_index)) = file_columns(package) else {
        return Err(ArchiveError::Corrupt("MSI 缺少 File 表".to_string()));
    };
    let components = component_directories(package);
    let directories = directory_table(package);
    let mut names = HashMap::new();
    for row in package
        .select_rows(Select::table("File"))
        .map_err(|error| ArchiveError::Corrupt(format!("MSI File 表读取失败：{error}")))?
    {
        let key = row[key_index].as_str().unwrap_or_default().to_string();
        let file_name = long_name(row[name_index].as_str().unwrap_or_default());
        let directory = component_index
            .and_then(|index| row[index].as_str())
            .and_then(|component| components.get(component))
            .map(|directory| directory_path(directory, &directories))
            .unwrap_or_default();
        let path = if directory.is_empty() {
            file_name
        } else {
            format!("{directory}/{file_name}")
        };
        names.insert(key, path);
    }
    Ok(names)
}

/// 返回 File 表的 File、FileName 和可选 Component_ 列索引。
fn file_columns(package: &Package<File>) -> Option<(usize, usize, Option<usize>)> {
    let table = package.get_table("File")?;
    let key = table
        .columns()
        .iter()
        .position(|column| column.name() == "File")?;
    let name = table
        .columns()
        .iter()
        .position(|column| column.name() == "FileName")?;
    let component = table
        .columns()
        .iter()
        .position(|column| column.name() == "Component_");
    Some((key, name, component))
}

/// 读取 Component 表的组件到目录键映射。
fn component_directories(package: &mut Package<File>) -> HashMap<String, String> {
    let Some(table) = package.get_table("Component") else {
        return HashMap::new();
    };
    let Some(component_index) = table
        .columns()
        .iter()
        .position(|column| column.name() == "Component")
    else {
        return HashMap::new();
    };
    let Some(directory_index) = table
        .columns()
        .iter()
        .position(|column| column.name() == "Directory_")
    else {
        return HashMap::new();
    };
    let mut components = HashMap::new();
    if let Ok(rows) = package.select_rows(Select::table("Component")) {
        for row in rows {
            components.insert(
                row[component_index]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                row[directory_index]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
    }
    components
}

/// 读取 Directory 表的目录键到 `(父目录, 默认目录名)` 映射。
fn directory_table(package: &mut Package<File>) -> HashMap<String, (String, String)> {
    let Some(table) = package.get_table("Directory") else {
        return HashMap::new();
    };
    let Some(key_index) = table
        .columns()
        .iter()
        .position(|column| column.name() == "Directory")
    else {
        return HashMap::new();
    };
    let parent_index = table
        .columns()
        .iter()
        .position(|column| column.name() == "Directory_Parent");
    let Some(name_index) = table
        .columns()
        .iter()
        .position(|column| column.name() == "DefaultDir")
    else {
        return HashMap::new();
    };
    let mut directories = HashMap::new();
    if let Ok(rows) = package.select_rows(Select::table("Directory")) {
        for row in rows {
            directories.insert(
                row[key_index].as_str().unwrap_or_default().to_string(),
                (
                    parent_index
                        .and_then(|index| row[index].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    long_name(row[name_index].as_str().unwrap_or_default()),
                ),
            );
        }
    }
    directories
}

/// 从目录键递归拼接路径。
fn directory_path(key: &str, directories: &HashMap<String, (String, String)>) -> String {
    let mut segments = Vec::new();
    let mut current = key.to_string();
    for _ in 0..32 {
        let Some((parent, name)) = directories.get(&current) else {
            break;
        };
        if !name.is_empty() && name != "." {
            segments.push(name.clone());
        }
        if parent.is_empty()
            || parent == "TARGETDIR"
            || parent.eq_ignore_ascii_case("TARGETDIR")
            || *parent == current
        {
            break;
        }
        current = parent.clone();
    }
    segments.reverse();
    segments.join("/")
}

/// 取 MSI 名称的 long 部分，并清理目录标记。
fn long_name(value: &str) -> String {
    value
        .rsplit('|')
        .next()
        .unwrap_or(value)
        .trim()
        .trim_end_matches(':')
        .to_string()
}

/// 从 Media 表读取内嵌或外部 CAB 来源。
fn read_cabinet_source(
    package: &mut Package<File>,
    msi_path: &Path,
) -> Result<CabSource, ArchiveError> {
    let table = package
        .get_table("Media")
        .ok_or_else(|| ArchiveError::Corrupt("MSI 缺少 Media 表".to_string()))?;
    let cabinet_index = table
        .columns()
        .iter()
        .position(|column| column.name() == "Cabinet")
        .ok_or_else(|| ArchiveError::Corrupt("MSI Media 表缺少 Cabinet 列".to_string()))?;
    for row in package
        .select_rows(Select::table("Media"))
        .map_err(|error| ArchiveError::Corrupt(format!("MSI Media 表读取失败：{error}")))?
    {
        let cabinet = row[cabinet_index].as_str().unwrap_or_default().to_string();
        if let Some(name) = cabinet.strip_prefix('#') {
            return Ok(CabSource::Embedded(name.to_string()));
        }
        if !cabinet.is_empty() {
            let base = msi_path.parent().unwrap_or_else(|| Path::new("."));
            return Ok(CabSource::External(base.join(cabinet)));
        }
    }
    Err(ArchiveError::InvalidRequest(
        "MSI 没有可用的 CAB".to_string(),
    ))
}

/// 把一个 CAB 的条目收集到列表。
fn collect_entries<R: Read + Seek>(
    cab: &Cabinet<R>,
    file_names: &HashMap<String, String>,
    entries: &mut Vec<MsiEntry>,
) {
    for folder in cab.folder_entries() {
        for file in folder.file_entries() {
            let cab_name = file.name().to_string();
            let path = file_names
                .get(&cab_name)
                .cloned()
                .unwrap_or_else(|| cab_name.clone());
            entries.push(MsiEntry {
                cab_name,
                path,
                size: u64::from(file.uncompressed_size()),
            });
        }
    }
}

/// 从已打开的 CAB 读取单个条目。
fn read_entry<R: Read + Seek>(
    cab: &mut Cabinet<R>,
    entry: &MsiEntry,
) -> Result<Vec<u8>, ArchiveError> {
    let mut reader = cab
        .read_file(&entry.cab_name)
        .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 条目读取失败：{error}")))?;
    let mut bytes = Vec::with_capacity(entry.size as usize);
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 条目读取失败：{error}")))?;
    Ok(bytes)
}
