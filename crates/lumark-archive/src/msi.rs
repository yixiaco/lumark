//! MSI 只读解析：从 Windows Installer 的内嵌 CAB 中读取文件。
//!
//! MSI 是 OLE 复合文档，`File` 表把 CAB 成员名映射到目标文件名，`Media`
//! 表保存内嵌 CAB 的流名。这里只处理内嵌 CAB；外部 CAB 由 MSI 安装时从
//! 媒体路径读取，不适合直接当作归档解压。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

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

/// 已打开的 MSI 包和 CAB 流名。
pub(crate) struct MsiArchive {
    /// MSI 包。
    package: Package<File>,
    /// 内嵌 CAB 流名。
    cab_stream: String,
    /// 条目列表。
    entries: Vec<MsiEntry>,
}

impl MsiArchive {
    /// 打开 MSI 并读取 File/Media 表和 CAB 目录。
    pub(crate) fn open(path: &Path) -> Result<Self, ArchiveError> {
        let mut package = Package::open(File::open(path)?)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI 打开失败：{error}")))?;
        let file_names = read_file_names(&mut package)?;
        let cab_stream = read_cabinet_stream(&mut package)?;
        let stream = package
            .read_stream(&cab_stream)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 流读取失败：{error}")))?;
        let cab = Cabinet::new(stream)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 解析失败：{error}")))?;
        let mut entries = Vec::new();
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
        Ok(Self {
            package,
            cab_stream,
            entries,
        })
    }

    /// 返回全部条目。
    pub(crate) fn entries(&self) -> &[MsiEntry] {
        &self.entries
    }

    /// 打开指定条目的解压读取器。
    pub(crate) fn open_entry(&mut self, entry: &MsiEntry) -> Result<Box<dyn Read>, ArchiveError> {
        let stream = self
            .package
            .read_stream(&self.cab_stream)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 流读取失败：{error}")))?;
        let mut cab = Cabinet::new(stream)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 解析失败：{error}")))?;
        let mut reader = cab
            .read_file(&entry.cab_name)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 条目读取失败：{error}")))?;
        let mut bytes = Vec::with_capacity(entry.size as usize);
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| ArchiveError::Corrupt(format!("MSI CAB 条目读取失败：{error}")))?;
        Ok(Box::new(Cursor::new(bytes)))
    }
}

/// 读取 File 表的 CAB 成员名到目标文件名映射。
fn read_file_names(package: &mut Package<File>) -> Result<HashMap<String, String>, ArchiveError> {
    let table = package
        .get_table("File")
        .ok_or_else(|| ArchiveError::Corrupt("MSI 缺少 File 表".to_string()))?;
    let key_index = table
        .columns()
        .iter()
        .position(|column| column.name() == "File")
        .ok_or_else(|| ArchiveError::Corrupt("MSI File 表缺少 File 列".to_string()))?;
    let name_index = table
        .columns()
        .iter()
        .position(|column| column.name() == "FileName")
        .ok_or_else(|| ArchiveError::Corrupt("MSI File 表缺少 FileName 列".to_string()))?;
    let mut names = HashMap::new();
    for row in package
        .select_rows(Select::table("File"))
        .map_err(|error| ArchiveError::Corrupt(format!("MSI File 表读取失败：{error}")))?
    {
        let key = row[key_index].as_str().unwrap_or_default().to_string();
        let file_name = row[name_index].as_str().unwrap_or_default().to_string();
        let target = file_name
            .rsplit('|')
            .next()
            .unwrap_or(&file_name)
            .to_string();
        names.insert(key, target);
    }
    Ok(names)
}

/// 读取 Media 表中内嵌 CAB 的流名。
fn read_cabinet_stream(package: &mut Package<File>) -> Result<String, ArchiveError> {
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
            return Ok(name.to_string());
        }
        if !cabinet.is_empty() {
            return Err(ArchiveError::InvalidRequest(
                "MSI 使用外部 CAB，暂不支持直接解压".to_string(),
            ));
        }
    }
    Err(ArchiveError::InvalidRequest(
        "MSI 没有内嵌 CAB，暂不支持直接解压".to_string(),
    ))
}
