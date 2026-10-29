//! LZH 流式写入器。
//!
//! Store（lh0）条目按 1 MiB 分块读取并直接写出；lh4/lh5/lh7 使用可续写的
//! 块编码器，跨分块保留历史窗口和位写入状态，因此单个超大文件也不需要
//! 整块读入内存。头部先写占位值，数据写完后再回填大小和 CRC-16，并在回填
//! 时同步更新头部校验和。

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use oxiarc_lzhuf::LzhMethod;

use crate::error::ArchiveError;
use crate::format::CreateReport;
use crate::lzh_stream::LzhStreamEncoder;
use crate::service::{CreateProgress, SourcePlan, ensure_not_cancelled};

/// LZH level-0 固定头部长度：方法 5 + 压缩大小 4 + 原始大小 4 + 时间 4 +
/// 属性 1 + 级别 1 + 路径长度 1。
const LZH_FIXED_HEADER_LEN: usize = 22;

/// LZH level-0 头部最大路径长度，受 8 位头部长度限制。
const LZH_MAX_PATH_BYTES: usize = u8::MAX as usize - LZH_FIXED_HEADER_LEN;

/// Store 条目的流式分块大小。
const LZH_CHUNK_BYTES: usize = 1024 * 1024;

/// 把源计划写成 LZH 归档。
pub(crate) fn create<F>(
    output: &Path,
    plan: &SourcePlan,
    method: LzhMethod,
    cancel: &Option<Arc<AtomicBool>>,
    progress: F,
) -> Result<CreateReport, ArchiveError>
where
    F: FnMut(CreateProgress),
{
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    create_to(
        File::create(output)?,
        output,
        plan,
        method,
        cancel,
        progress,
    )
}

/// 把 LZH 写入任意可 Seek 输出，供伪装单遍写入复用。
pub(crate) fn create_to<W, F>(
    mut writer: W,
    output: &Path,
    plan: &SourcePlan,
    method: LzhMethod,
    cancel: &Option<Arc<AtomicBool>>,
    mut progress: F,
) -> Result<CreateReport, ArchiveError>
where
    W: Write + Seek,
    F: FnMut(CreateProgress),
{
    let mut report = CreateReport {
        output: output.to_path_buf(),
        ..Default::default()
    };
    let entries_total = plan.files.len();
    let mut source_done = 0u64;
    for (relative, source, file_bytes) in &plan.files {
        ensure_not_cancelled(cancel)?;
        if relative.len() > LZH_MAX_PATH_BYTES {
            return Err(ArchiveError::InvalidRequest(format!(
                "LZH 条目路径超过 {LZH_MAX_PATH_BYTES} 字节：{relative}"
            )));
        }
        if *file_bytes > u32::MAX as u64 {
            return Err(ArchiveError::InvalidRequest(format!(
                "LZH 单条目超过 4 GiB 上限：{}",
                source.display()
            )));
        }
        let header_offset = writer.stream_position()?;
        let placeholder = build_header(relative.as_bytes(), method, 0, *file_bytes as u32, 0);
        write_header(&mut writer, &placeholder)?;
        let data_start = writer.stream_position()?;
        let mut input = File::open(source)?;
        let mut buffer = vec![0u8; LZH_CHUNK_BYTES];
        let mut crc = 0u16;
        let mut file_done = 0u64;
        if method.is_stored() {
            loop {
                ensure_not_cancelled(cancel)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                crc = update_crc16(crc, &buffer[..read]);
                writer.write_all(&buffer[..read])?;
                file_done = file_done.saturating_add(read as u64);
                source_done = source_done.saturating_add(read as u64);
                progress(CreateProgress {
                    current: relative.clone(),
                    bytes_done: source_done,
                    bytes_total: plan.total_bytes,
                    current_bytes_done: file_done,
                    current_bytes_total: *file_bytes,
                    output_bytes: writer.stream_position()?,
                    entries_done: report.files,
                    entries_total,
                });
            }
        } else {
            let mut encoder = LzhStreamEncoder::new(&mut writer, method);
            loop {
                ensure_not_cancelled(cancel)?;
                let read = input.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                crc = update_crc16(crc, &buffer[..read]);
                encoder.write_chunk(&buffer[..read])?;
                file_done = file_done.saturating_add(read as u64);
                source_done = source_done.saturating_add(read as u64);
                progress(CreateProgress {
                    current: relative.clone(),
                    bytes_done: source_done,
                    bytes_total: plan.total_bytes,
                    current_bytes_done: file_done,
                    current_bytes_total: *file_bytes,
                    output_bytes: data_start.saturating_add(encoder.output_bytes()),
                    entries_done: report.files,
                    entries_total,
                });
            }
            encoder.finish()?;
        }
        let end = writer.stream_position()?;
        let compressed = end.saturating_sub(data_start);
        if compressed > u32::MAX as u64 {
            return Err(ArchiveError::InvalidRequest(format!(
                "LZH 压缩数据超过 4 GiB 头字段上限：{}",
                source.display()
            )));
        }
        let final_header = build_header(
            relative.as_bytes(),
            method,
            compressed as u32,
            *file_bytes as u32,
            crc,
        );
        writer.seek(SeekFrom::Start(header_offset))?;
        write_header(&mut writer, &final_header)?;
        writer.seek(SeekFrom::Start(end))?;

        report.files += 1;
        report.bytes = source_done;
        progress(CreateProgress {
            current: relative.clone(),
            bytes_done: source_done,
            bytes_total: plan.total_bytes,
            current_bytes_done: *file_bytes,
            current_bytes_total: *file_bytes,
            output_bytes: writer.stream_position()?,
            entries_done: report.files,
            entries_total,
        });
    }
    writer.write_all(&[0])?;
    writer.flush()?;
    Ok(report)
}

/// 组装 level-0 头部（不含长度和校验字节）。
fn build_header(
    path: &[u8],
    method: LzhMethod,
    compressed_size: u32,
    original_size: u32,
    crc: u16,
) -> Vec<u8> {
    let method = method.id();
    let mut header = Vec::with_capacity(LZH_FIXED_HEADER_LEN + path.len());
    header.extend_from_slice(&method);
    header.extend_from_slice(&compressed_size.to_le_bytes());
    header.extend_from_slice(&original_size.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.push(0x20);
    header.push(0);
    header.push(path.len() as u8);
    header.extend_from_slice(path);
    header.extend_from_slice(&crc.to_le_bytes());
    header
}

/// 写入头部长度、头部校验和和头部正文。
fn write_header<W: Write>(writer: &mut W, header: &[u8]) -> io::Result<()> {
    let checksum = header.iter().fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    writer.write_all(&[header.len() as u8, checksum])?;
    writer.write_all(header)
}

/// 增量更新 LZH CRC-16/ARC。
fn update_crc16(mut crc: u16, data: &[u8]) -> u16 {
    for byte in data {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}
