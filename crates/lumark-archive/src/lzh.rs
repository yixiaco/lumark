//! LZH 流式写入器。
//!
//! Store（lh0）条目按 1 MiB 分块读取并直接写出，单个超大文件不需要整块
//! 读入内存；lh4/lh5/lh7 仍依赖上游编码器一次性接收完整数据，因此保留
//! 512 MiB 内存上限。头部先写占位值，数据写完后再回填大小和 CRC-16，
//! 并在回填时同步更新头部校验和。

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use oxiarc_lzhuf::{LzhEncoder, LzhMethod};

use crate::error::ArchiveError;
use crate::format::CreateReport;
use crate::service::{CreateProgress, SourcePlan, ensure_not_cancelled};

/// LZH level-0 固定头部长度：方法 5 + 压缩大小 4 + 原始大小 4 + 时间 4 +
/// 属性 1 + 级别 1 + 路径长度 1。
const LZH_FIXED_HEADER_LEN: usize = 22;

/// LZH level-0 头部最大路径长度，受 8 位头部长度限制。
const LZH_MAX_PATH_BYTES: usize = u8::MAX as usize - LZH_FIXED_HEADER_LEN;

/// Store 条目的流式分块大小。
const LZH_CHUNK_BYTES: usize = 1024 * 1024;

/// 非 Store 方法仍需要整块编码，超过该上限时提前拒绝，避免 OOM。
const LZH_COMPRESSED_MAX_BYTES: u64 = 512 * 1024 * 1024;

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
        if !method.is_stored() && *file_bytes > LZH_COMPRESSED_MAX_BYTES {
            return Err(ArchiveError::InvalidRequest(format!(
                "LZH 压缩方法仍需要把单个文件读入内存，暂不支持超过 {} MiB 的条目；请选择 Store 方法：{}",
                LZH_COMPRESSED_MAX_BYTES / (1024 * 1024),
                source.display()
            )));
        }

        let header_offset = writer.stream_position()?;
        let (crc, compressed) = if method.is_stored() {
            let header = build_header(
                relative.as_bytes(),
                method,
                *file_bytes as u32,
                *file_bytes as u32,
                0,
            );
            write_header(&mut writer, &header)?;
            let mut input = File::open(source)?;
            let mut buffer = vec![0u8; LZH_CHUNK_BYTES];
            let mut crc = 0u16;
            let mut file_done = 0u64;
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
            (crc, *file_bytes)
        } else {
            // 非 Store 方法一次性编码，避免上游编码器跨调用产生无效流。
            let data = fs::read(source)?;
            let crc = crc16(&data);
            let mut encoder = LzhEncoder::new(method);
            let mut compressed = Vec::new();
            encoder
                .encode(&data, &mut compressed, true)
                .map_err(|error| ArchiveError::Corrupt(error.to_string()))?;
            let header = build_header(
                relative.as_bytes(),
                method,
                compressed.len() as u32,
                *file_bytes as u32,
                crc,
            );
            write_header(&mut writer, &header)?;
            writer.write_all(&compressed)?;
            source_done = source_done.saturating_add(data.len() as u64);
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
            (crc, compressed.len() as u64)
        };

        // Store 分支的头部先写了 0 CRC/占位压缩大小，这里回填并更新校验和。
        if method.is_stored() {
            let end = writer.stream_position()?;
            let mut final_header = build_header(
                relative.as_bytes(),
                method,
                compressed as u32,
                *file_bytes as u32,
                crc,
            );
            // 以写入时的头部长度为准重建，防止路径字节在回填时变化。
            final_header[5..9].copy_from_slice(&(compressed as u32).to_le_bytes());
            final_header[9..13].copy_from_slice(&(*file_bytes as u32).to_le_bytes());
            let crc_offset = final_header.len() - 2;
            final_header[crc_offset..].copy_from_slice(&crc.to_le_bytes());
            let checksum = final_header
                .iter()
                .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
            writer.seek(SeekFrom::Start(header_offset + 1))?;
            writer.write_all(&[checksum])?;
            writer.write_all(&final_header)?;
            writer.seek(SeekFrom::Start(end))?;
        }

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

/// 计算 LZH 使用的 CRC-16/ARC。
fn crc16(data: &[u8]) -> u16 {
    update_crc16(0, data)
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
