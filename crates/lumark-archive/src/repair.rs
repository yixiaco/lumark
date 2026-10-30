//! ZIP 家族结构修复：扫描本地文件头并重建中央目录。
//!
//! 修复只读取原始数据，不重新压缩、不解密，因此加密条目、压缩方法和 CRC
//! 都原样保留。输出始终是新文件，原归档不会被修改；无法确定边界的条目
//! （缺少中央目录且带数据描述符）会被跳过并记录原因。

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::ArchiveError;
use crate::format::detect_format;

/// 修复结果。
#[derive(Debug, Clone, Default)]
pub struct RepairReport {
    /// 成功写出的条目数。
    pub entries: usize,
    /// 复制到新归档的压缩数据字节数。
    pub bytes: u64,
    /// 因边界未知而跳过的条目。
    pub skipped: Vec<String>,
}

/// 中央目录中的元数据。
#[derive(Debug, Clone)]
struct CentralEntry {
    name: Vec<u8>,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    uncompressed: u32,
    version_needed: u16,
    version_made_by: u16,
    internal_attributes: u16,
    external_attributes: u32,
    extra: Vec<u8>,
    comment: Vec<u8>,
}

/// 本地文件头扫描结果。
struct LocalEntry {
    header_offset: u64,
    name: Vec<u8>,
    extra: Vec<u8>,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    uncompressed: u32,
    data_offset: u64,
}

/// 修复 ZIP 家族归档，写出到 `output`。
pub fn repair_zip(input: &Path, output: &Path) -> Result<RepairReport, ArchiveError> {
    let format = detect_format(input);
    if !format.is_zip_family() {
        return Err(ArchiveError::UnsupportedFormat(format.label().to_string()));
    }
    if input == output {
        return Err(ArchiveError::InvalidRequest(
            "修复输出不能覆盖原归档".into(),
        ));
    }
    if output.exists() {
        return Err(ArchiveError::InvalidRequest(format!(
            "修复输出已存在：{}",
            output.display()
        )));
    }
    let length = fs::metadata(input)?.len();
    let central = read_central(input, length);
    let mut file = File::open(input)?;
    let start = find_first_local(&mut file, length)?;
    let Some(start) = start else {
        return Err(ArchiveError::Corrupt("没有找到 ZIP 本地文件头".into()));
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(output.parent().unwrap_or_else(|| Path::new(".")))?;
    let mut report = RepairReport::default();
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let mut offset = start;
        let mut central_records = Vec::new();
        while offset < length {
            let Some(local) = read_local(&mut file, offset, length)? else {
                break;
            };
            let Some((compressed, uncompressed, crc)) =
                resolve_sizes(&local, central.get(local.name.as_slice()))
            else {
                report.skipped.push(format!(
                    "{}（缺少可确定的压缩大小）",
                    display_name(&local.name)
                ));
                offset = local.data_offset;
                if offset >= length {
                    break;
                }
                let Some(next) = find_next_local(&mut file, offset, length)? else {
                    break;
                };
                offset = next;
                continue;
            };
            let data_end = local
                .data_offset
                .checked_add(compressed)
                .ok_or_else(|| ArchiveError::Corrupt("ZIP 条目大小溢出".into()))?;
            if data_end > length
                || compressed >= u64::from(u32::MAX)
                || uncompressed >= u64::from(u32::MAX)
            {
                report
                    .skipped
                    .push(format!("{}（ZIP64 或越界）", display_name(&local.name)));
                break;
            }
            let local_offset = writer.stream_position()?;
            if local_offset > u64::from(u32::MAX) {
                return Err(ArchiveError::InvalidRequest(
                    "修复结果超过 ZIP32 偏移上限，暂不支持 ZIP64".into(),
                ));
            }
            copy_range(&mut file, &mut writer, local.header_offset, data_end)?;
            let central_entry = central.get(local.name.as_slice());
            central_records.push((
                CentralEntry {
                    name: local.name.clone(),
                    flags: local.flags,
                    method: local.method,
                    time: local.time,
                    date: local.date,
                    crc: if crc != 0 { crc } else { local.crc },
                    compressed: compressed as u32,
                    uncompressed: uncompressed as u32,
                    version_needed: central_entry.map_or(20, |entry| entry.version_needed),
                    version_made_by: central_entry.map_or(20, |entry| entry.version_made_by),
                    internal_attributes: central_entry.map_or(0, |entry| entry.internal_attributes),
                    external_attributes: central_entry.map_or(0, |entry| entry.external_attributes),
                    extra: central_entry
                        .map_or_else(|| local.extra.clone(), |entry| entry.extra.clone()),
                    comment: central_entry.map_or_else(Vec::new, |entry| entry.comment.clone()),
                },
                local_offset,
            ));
            report.entries += 1;
            report.bytes += compressed;
            if data_end >= length {
                break;
            }
            let Some(next) = find_next_local(&mut file, data_end, length)? else {
                break;
            };
            offset = next;
        }
        let central_offset = writer.stream_position()?;
        for (entry, local_offset) in &central_records {
            write_central(&mut writer, entry, *local_offset)?;
        }
        let central_size = writer.stream_position()? - central_offset;
        write_end(
            &mut writer,
            central_records.len(),
            central_offset,
            central_size,
        )?;
        writer.flush()?;
    }
    temporary
        .persist_noclobber(output)
        .map_err(|error| ArchiveError::Io(error.error))?;
    Ok(report)
}

/// 判断是否为支持的 ZIP 家族格式。
pub fn supports_repair(path: &Path) -> bool {
    detect_format(path).is_zip_family()
}

/// 从中央目录读取元数据；损坏时返回已解析部分。
fn read_central(path: &Path, length: u64) -> HashMap<Vec<u8>, CentralEntry> {
    let mut map = HashMap::new();
    let Ok(mut file) = File::open(path) else {
        return map;
    };
    let tail_length = length.min(65_557);
    if tail_length < 22 || file.seek(SeekFrom::End(-(tail_length as i64))).is_err() {
        return map;
    }
    let mut tail = vec![0u8; tail_length as usize];
    if file.read_exact(&mut tail).is_err() {
        return map;
    }
    let Some(position) = tail.windows(4).rposition(|window| window == b"PK\x05\x06") else {
        return map;
    };
    let Some(end) = parse_end(&tail[position..]) else {
        return map;
    };
    if end.size as u64 > 512 * 1024 * 1024 || end.count > 1_000_000 {
        return map;
    }
    if end.offset as u64 + end.size as u64 > length {
        return map;
    }
    if file.seek(SeekFrom::Start(end.offset as u64)).is_err() {
        return map;
    }
    let mut data = vec![0u8; end.size as usize];
    if file.read_exact(&mut data).is_err() {
        return map;
    }
    let mut cursor = 0usize;
    for _ in 0..end.count {
        let Some(entry) = parse_central(&data[cursor..]) else {
            break;
        };
        let record_length =
            46 + entry.name.len() + entry.comment.len() + central_extra_length(&data[cursor..]);
        map.entry(entry.name.clone()).or_insert(entry);
        if record_length == 0 {
            break;
        }
        cursor += record_length;
        if cursor >= data.len() {
            break;
        }
    }
    map
}

/// 中央目录额外字段长度。
fn central_extra_length(data: &[u8]) -> usize {
    if data.len() < 30 {
        return 0;
    }
    u16::from_le_bytes([data[30], data[31]]) as usize
}

/// 解析中央目录条目。
fn parse_central(data: &[u8]) -> Option<CentralEntry> {
    if data.len() < 46 || &data[..4] != b"PK\x01\x02" {
        return None;
    }
    let version_made_by = u16::from_le_bytes([data[4], data[5]]);
    let version_needed = u16::from_le_bytes([data[6], data[7]]);
    let flags = u16::from_le_bytes([data[8], data[9]]);
    let method = u16::from_le_bytes([data[10], data[11]]);
    let time = u16::from_le_bytes([data[12], data[13]]);
    let date = u16::from_le_bytes([data[14], data[15]]);
    let crc = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let compressed = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
    let uncompressed = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
    let name_length = u16::from_le_bytes([data[28], data[29]]) as usize;
    let extra_length = u16::from_le_bytes([data[30], data[31]]) as usize;
    let comment_length = u16::from_le_bytes([data[32], data[33]]) as usize;
    let internal_attributes = u16::from_le_bytes([data[36], data[37]]);
    let external_attributes = u32::from_le_bytes([data[38], data[39], data[40], data[41]]);
    let end = 46usize
        .checked_add(name_length)?
        .checked_add(extra_length)?
        .checked_add(comment_length)?;
    if end > data.len() {
        return None;
    }
    Some(CentralEntry {
        name: data[46..46 + name_length].to_vec(),
        flags,
        method,
        time,
        date,
        crc,
        compressed,
        uncompressed,
        version_needed,
        version_made_by,
        internal_attributes,
        external_attributes,
        extra: data[46 + name_length..46 + name_length + extra_length].to_vec(),
        comment: data[46 + name_length + extra_length..end].to_vec(),
    })
}

/// EOCD 字段。
struct EndRecord {
    count: usize,
    offset: u32,
    size: u32,
}

/// 解析 EOCD；注释长度不足时仍接受。
fn parse_end(data: &[u8]) -> Option<EndRecord> {
    if data.len() < 22 || &data[..4] != b"PK\x05\x06" {
        return None;
    }
    let count = u16::from_le_bytes([data[10], data[11]]) as usize;
    let offset = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    Some(EndRecord {
        count,
        offset,
        size,
    })
}

/// 找到第一个本地文件头；支持 SFX 前缀。
fn find_first_local(file: &mut File, length: u64) -> Result<Option<u64>, ArchiveError> {
    let start = find_next_local(file, 0, length)?;
    Ok(start)
}

/// 从 `from` 开始寻找下一个本地文件头。
fn find_next_local(file: &mut File, from: u64, length: u64) -> Result<Option<u64>, ArchiveError> {
    if from >= length {
        return Ok(None);
    }
    let mut position = from;
    let mut buffer = [0u8; 64 * 1024];
    while position < length {
        file.seek(SeekFrom::Start(position))?;
        let read = file.read(&mut buffer)?;
        if read < 4 {
            return Ok(None);
        }
        if let Some(index) = buffer[..read]
            .windows(4)
            .position(|window| window == b"PK\x03\x04")
        {
            return Ok(Some(position + index as u64));
        }
        position += (read - 3) as u64;
    }
    Ok(None)
}

/// 读取一个本地文件头。
fn read_local(
    file: &mut File,
    offset: u64,
    length: u64,
) -> Result<Option<LocalEntry>, ArchiveError> {
    if offset + 30 > length {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut fixed = [0u8; 30];
    file.read_exact(&mut fixed)?;
    if &fixed[..4] != b"PK\x03\x04" {
        return Ok(None);
    }
    let flags = u16::from_le_bytes([fixed[6], fixed[7]]);
    let method = u16::from_le_bytes([fixed[8], fixed[9]]);
    let time = u16::from_le_bytes([fixed[10], fixed[11]]);
    let date = u16::from_le_bytes([fixed[12], fixed[13]]);
    let crc = u32::from_le_bytes([fixed[14], fixed[15], fixed[16], fixed[17]]);
    let compressed = u32::from_le_bytes([fixed[18], fixed[19], fixed[20], fixed[21]]);
    let uncompressed = u32::from_le_bytes([fixed[22], fixed[23], fixed[24], fixed[25]]);
    let name_length = u16::from_le_bytes([fixed[26], fixed[27]]) as usize;
    let extra_length = u16::from_le_bytes([fixed[28], fixed[29]]) as usize;
    if name_length == 0 || name_length > 65_535 {
        return Ok(None);
    }
    let data_offset = offset
        .checked_add(30)
        .and_then(|value| value.checked_add(name_length as u64))
        .and_then(|value| value.checked_add(extra_length as u64))
        .ok_or_else(|| ArchiveError::Corrupt("ZIP 本地头长度溢出".into()))?;
    if data_offset > length {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset + 30))?;
    let mut name = vec![0u8; name_length];
    file.read_exact(&mut name)?;
    let mut extra = vec![0u8; extra_length];
    file.read_exact(&mut extra)?;
    Ok(Some(LocalEntry {
        header_offset: offset,
        name,
        extra,
        flags,
        method,
        time,
        date,
        crc,
        compressed,
        uncompressed,
        data_offset,
    }))
}

/// 决定条目的压缩/原始大小与 CRC。
fn resolve_sizes(local: &LocalEntry, central: Option<&CentralEntry>) -> Option<(u64, u64, u32)> {
    let data_descriptor = local.flags & 0x08 != 0;
    if !data_descriptor && local.compressed != u32::MAX {
        return Some((
            u64::from(local.compressed),
            u64::from(local.uncompressed),
            local.crc,
        ));
    }
    let central = central?;
    Some((
        u64::from(central.compressed),
        u64::from(central.uncompressed),
        central.crc,
    ))
}

/// 从输入复制一段数据到输出。
fn copy_range<W: Write>(
    input: &mut File,
    output: &mut W,
    start: u64,
    end: u64,
) -> Result<(), ArchiveError> {
    input.seek(SeekFrom::Start(start))?;
    let mut remaining = end.saturating_sub(start);
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let take = remaining.min(buffer.len() as u64) as usize;
        let read = input.read(&mut buffer[..take])?;
        if read == 0 {
            return Err(ArchiveError::Corrupt("修复时输入被截断".into()));
        }
        output.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    Ok(())
}

/// 写入中央目录记录。
fn write_central<W: Write>(
    writer: &mut W,
    entry: &CentralEntry,
    local_offset: u64,
) -> Result<(), ArchiveError> {
    writer.write_all(b"PK\x01\x02")?;
    writer.write_all(&entry.version_made_by.to_le_bytes())?;
    writer.write_all(&entry.version_needed.to_le_bytes())?;
    writer.write_all(&entry.flags.to_le_bytes())?;
    writer.write_all(&entry.method.to_le_bytes())?;
    writer.write_all(&entry.time.to_le_bytes())?;
    writer.write_all(&entry.date.to_le_bytes())?;
    writer.write_all(&entry.crc.to_le_bytes())?;
    writer.write_all(&entry.compressed.to_le_bytes())?;
    writer.write_all(&entry.uncompressed.to_le_bytes())?;
    writer.write_all(&(entry.name.len() as u16).to_le_bytes())?;
    writer.write_all(&(entry.extra.len() as u16).to_le_bytes())?;
    writer.write_all(&(entry.comment.len() as u16).to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(&entry.internal_attributes.to_le_bytes())?;
    writer.write_all(&entry.external_attributes.to_le_bytes())?;
    writer.write_all(&(local_offset as u32).to_le_bytes())?;
    writer.write_all(&entry.name)?;
    writer.write_all(&entry.extra)?;
    writer.write_all(&entry.comment)?;
    Ok(())
}

/// 写入 EOCD。
fn write_end<W: Write>(
    writer: &mut W,
    count: usize,
    central_offset: u64,
    central_size: u64,
) -> Result<(), ArchiveError> {
    if count > u16::MAX as usize
        || central_offset > u32::MAX as u64
        || central_size > u32::MAX as u64
    {
        return Err(ArchiveError::InvalidRequest(
            "修复结果超过 ZIP32 上限，暂不支持 ZIP64".into(),
        ));
    }
    writer.write_all(b"PK\x05\x06")?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    writer.write_all(&(count as u16).to_le_bytes())?;
    writer.write_all(&(count as u16).to_le_bytes())?;
    writer.write_all(&(central_size as u32).to_le_bytes())?;
    writer.write_all(&(central_offset as u32).to_le_bytes())?;
    writer.write_all(&0u16.to_le_bytes())?;
    Ok(())
}

/// 把条目名转成可读文本。
fn display_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name).to_string()
}
