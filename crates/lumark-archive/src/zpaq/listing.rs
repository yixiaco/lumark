//! `zpaq list` 输出解析。
//!
//! 引擎按固定列宽打印条目：状态字符、19 字节日期时间、12 字节大小、
//! 5 字节属性和路径。这里只接受内部条目行（`-`），遇到无法解析的行直接
//! 报错，避免静默漏掉归档内容。

use std::collections::HashSet;

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::error::ArchiveError;
use crate::format::ArchiveEntry;

/// 日期时间在列表行中的固定范围。
const DATE_RANGE: std::ops::Range<usize> = 2..21;
/// 大小字段在列表行中的固定范围。
const SIZE_RANGE: std::ops::Range<usize> = 22..34;
/// 路径字段在列表行中的起始位置。
const PATH_START: usize = 41;

/// 一条解析后的列表条目，保留原始路径供引擎过滤参数使用。
#[derive(Debug, Clone)]
pub(super) struct ListedEntry {
    /// 统一使用 `/` 的归档内路径。
    pub(super) path: String,
    /// 引擎输出中的原始路径，外部工具可能写入 `\`。
    pub(super) raw_path: String,
    pub(super) is_dir: bool,
    pub(super) size: u64,
    pub(super) modified: Option<DateTime<Utc>>,
}

/// 把列表条目转换为服务层归档信息。
pub(super) fn archive_info(
    entries: &[ListedEntry],
    encrypted: bool,
    compressed_size: u64,
) -> crate::format::ArchiveInfo {
    let mut info = crate::format::ArchiveInfo {
        format: Some(crate::format::ArchiveFormat::Zpaq),
        encrypted,
        header_encrypted: encrypted,
        compressed_size,
        ..Default::default()
    };
    for (index, entry) in entries.iter().enumerate() {
        info.entries.push(ArchiveEntry {
            id: index as u64,
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            size: entry.size,
            compressed_size: None,
            modified: entry.modified,
            crc32: None,
            method: "ZPAQ".to_string(),
            encrypted,
        });
        if !entry.is_dir {
            info.total_size = info.total_size.saturating_add(entry.size);
        }
    }
    info
}

/// 解析完整列表输出，只保留 `zpaq list` 的文件条目行。
pub(super) fn parse(output: &str) -> Result<Vec<ListedEntry>, ArchiveError> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for line in output.lines() {
        // 头部、版本汇总和统计行都不是条目。流式归档的统计行可能以负号
        // 开头（`-0.000001 MB of ...`），因此必须要求状态字符后紧跟空格。
        if !line.starts_with("- ") {
            continue;
        }
        let entry = parse_line(line)?;
        if !seen.insert(entry.path.clone()) {
            return Err(ArchiveError::Corrupt(format!(
                "ZPAQ 列表包含重复路径：{}",
                entry.path
            )));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// 解析单条 `zpaq list` 输出行。
fn parse_line(line: &str) -> Result<ListedEntry, ArchiveError> {
    let malformed = || ArchiveError::Corrupt(format!("无法解析 zpaq list 输出：{line}"));
    let date_field = line.get(DATE_RANGE).ok_or_else(malformed)?;
    let size_field = line.get(SIZE_RANGE).ok_or_else(malformed)?.trim();
    if line.len() <= PATH_START {
        return Err(malformed());
    }

    let modified = parse_timestamp(date_field).map_err(|_| malformed())?;
    let listed_size = size_field.parse::<i64>().map_err(|_| malformed())?;
    let raw_path = line[PATH_START..].trim_start().to_string();
    if raw_path.is_empty() {
        return Err(malformed());
    }

    // 外部工具可能在 Windows 上写入反斜杠；统一成 `/` 后再做安全校验。
    let normalized = raw_path.replace('\\', "/");
    let is_dir = normalized.ends_with('/');
    let size = if is_dir { 0 } else { listed_size.max(0) as u64 };

    Ok(ListedEntry {
        path: normalized,
        raw_path,
        is_dir,
        size,
        modified,
    })
}

/// 把列表中固定的日期时间字段解析为 UTC 时间。
///
/// 引擎写入的是 UTC 秒数换算出的十进制时间，因此这里按 UTC 解释；
/// 全空白日期表示未知时间。
fn parse_timestamp(value: &str) -> Result<Option<DateTime<Utc>>, chrono::ParseError> {
    if value.trim().is_empty() || value.starts_with("0000-00-00") {
        return Ok(None);
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")?;
    Ok(Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼接一列与引擎输出完全一致的列表行。
    fn line(status: char, date: &str, size: i64, attr: &str, path: &str) -> String {
        format!("{status} {date:19} {size:>12} {attr:<5} {path}")
    }

    /// 文件行应解析出大小、时间和路径。
    #[test]
    fn parses_file_entry() {
        let output = line('-', "2024-01-02 03:04:05", 1234, "A", "dir/file.txt");
        let entries = parse(&format!("\n{output}\n")).expect("应解析成功");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "dir/file.txt");
        assert_eq!(entries[0].size, 1234);
        assert!(!entries[0].is_dir);
        assert_eq!(
            entries[0]
                .modified
                .expect("应有时间")
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            "2024-01-02 03:04:05"
        );
    }

    /// 目录行应去掉目录大小并保留结尾斜杠。
    #[test]
    fn parses_directory_entry() {
        let output = line('-', "2024-01-02 03:04:05", 999, "D", "dir/");
        let entries = parse(&output).expect("应解析成功");
        assert_eq!(entries[0].path, "dir/");
        assert_eq!(entries[0].size, 0);
        assert!(entries[0].is_dir);
    }

    /// 流式归档的大小为 -1，应按未知大小处理。
    #[test]
    fn parses_streaming_unknown_size() {
        let output = line('-', "2024-01-02 03:04:05", -1, "", "a.txt");
        let entries = parse(&output).expect("应解析成功");
        assert_eq!(entries[0].size, 0);
        assert!(!entries[0].is_dir);
    }

    /// 反斜杠路径应统一成斜杠。
    #[test]
    fn normalizes_backslash_path() {
        let output = line('-', "2024-01-02 03:04:05", 7, "A", r"dir\file.txt");
        let entries = parse(&output).expect("应解析成功");
        assert_eq!(entries[0].path, "dir/file.txt");
        assert_eq!(entries[0].raw_path, r"dir\file.txt");
    }

    /// 畸形日期不能被静默跳过。
    #[test]
    fn rejects_malformed_entry() {
        let output = line('-', "not-a-date", 1, "A", "a.txt");
        assert!(parse(&output).is_err());
    }

    /// 头部和汇总行不应被当作条目。
    #[test]
    fn ignores_header_and_summary() {
        let output = format!(
            "zpaq v7.15 journaling archiver\narchive.zpaq: 1 versions, 1 files, 1 fragments\n\
             -0.000001 MB of 0.000000 MB (1 files) shown\n{}",
            line('-', "2024-01-02 03:04:05", 1, "A", "a.txt")
        );
        let entries = parse(&output).expect("应解析成功");
        assert_eq!(entries.len(), 1);
    }
}
