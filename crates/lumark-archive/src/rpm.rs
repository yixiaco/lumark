//! RPM 包只读解析：跳过 lead 和两个 header，读取压缩的 cpio 载荷。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use lzma_rust2::{LzmaReader, XzReader};

use crate::error::ArchiveError;

/// RPM lead 长度。
const RPM_LEAD_LEN: u64 = 96;
/// RPM header 魔数。
const RPM_HEADER_MAGIC: [u8; 3] = [0x8e, 0xad, 0xe8];
/// 单个 RPM header 的索引和数据上限。
const MAX_RPM_HEADER: u64 = 64 * 1024 * 1024;

/// 已打开的 RPM 包。
pub(crate) struct RpmArchive {
    /// 包文件。
    file: File,
    /// cpio 载荷起始偏移。
    payload_offset: u64,
}

impl RpmArchive {
    /// 打开 RPM 并定位压缩载荷。
    pub(crate) fn open(path: &std::path::Path) -> Result<Self, ArchiveError> {
        let mut file = File::open(path)?;
        let mut lead = [0u8; RPM_LEAD_LEN as usize];
        file.read_exact(&mut lead)?;
        if lead[0..4] != [0xed, 0xab, 0xee, 0xdb] {
            return Err(ArchiveError::Corrupt("不是 RPM 包".to_string()));
        }
        let signature_end = rpm_header_end(&mut file, RPM_LEAD_LEN)?;
        let payload_offset = rpm_header_end(&mut file, signature_end)?;
        Ok(Self {
            file,
            payload_offset,
        })
    }

    /// 打开解压后的 cpio 载荷。
    pub(crate) fn open_payload(&self) -> Result<Box<dyn Read>, ArchiveError> {
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(self.payload_offset))?;
        let mut magic = [0u8; 6];
        let read = file.read(&mut magic)?;
        file.seek(SeekFrom::Start(self.payload_offset))?;
        let reader: Box<dyn Read> = if magic[..read].starts_with(b"\x1f\x8b") {
            Box::new(GzDecoder::new(file))
        } else if magic[..read].starts_with(b"\xfd7zXZ\x00") {
            Box::new(XzReader::new(file, true))
        } else if magic[..read].starts_with(b"BZh") {
            Box::new(BzDecoder::new(file))
        } else if magic[..read].starts_with(b"\x28\xb5\x2f\xfd") {
            Box::new(zstd::stream::read::Decoder::new(file)?)
        } else if magic[..read].starts_with(b"\x5d\x00\x00") {
            Box::new(
                LzmaReader::new_mem_limit(file, u32::MAX, None)
                    .map_err(|error| ArchiveError::Corrupt(error.to_string()))?,
            )
        } else {
            Box::new(file)
        };
        Ok(reader)
    }
}

/// 计算一个 RPM header 的结束偏移，并对齐到 8 字节。
fn rpm_header_end(file: &mut File, offset: u64) -> Result<u64, ArchiveError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header)?;
    if header[0..3] != RPM_HEADER_MAGIC {
        return Err(ArchiveError::Corrupt("RPM header 魔数无效".to_string()));
    }
    let count = u32::from_be_bytes(header[8..12].try_into().unwrap()) as u64;
    let size = u32::from_be_bytes(header[12..16].try_into().unwrap()) as u64;
    if count.saturating_mul(16).saturating_add(size) > MAX_RPM_HEADER {
        return Err(ArchiveError::Corrupt("RPM header 过大".to_string()));
    }
    let end = offset
        .checked_add(16)
        .and_then(|value| value.checked_add(count * 16 + size))
        .ok_or_else(|| ArchiveError::Corrupt("RPM header 偏移溢出".to_string()))?;
    Ok((end + 7) & !7)
}

/// 一个 cpio 条目的头部信息。
pub(crate) struct CpioEntry {
    /// 归档内路径。
    pub(crate) path: String,
    /// Unix mode。
    pub(crate) mode: u32,
    /// 数据大小。
    pub(crate) size: u64,
}

impl CpioEntry {
    /// 是否目录。
    pub(crate) fn is_dir(&self) -> bool {
        self.mode & 0o170000 == 0o040000
    }

    /// 是否符号链接。
    pub(crate) fn is_symlink(&self) -> bool {
        self.mode & 0o170000 == 0o120000
    }
}

/// 读取下一个 newc/crc cpio 条目；遇到 TRAILER 或 EOF 时返回 `None`。
pub(crate) fn read_cpio_header<R: Read>(reader: &mut R) -> Result<Option<CpioEntry>, ArchiveError> {
    let mut header = [0u8; 110];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(ArchiveError::Io(error)),
    }
    if &header[0..6] != b"070701" && &header[0..6] != b"070702" {
        return Err(ArchiveError::Corrupt("RPM 载荷不是 newc cpio".to_string()));
    }
    let mode = parse_hex(&header[14..22])? as u32;
    let size = parse_hex(&header[54..62])?;
    let namesize = parse_hex(&header[94..102])? as usize;
    if namesize == 0 || namesize > 64 * 1024 {
        return Err(ArchiveError::Corrupt("RPM cpio 文件名长度异常".to_string()));
    }
    let mut name = vec![0u8; namesize];
    reader.read_exact(&mut name)?;
    while name.last() == Some(&0) {
        name.pop();
    }
    let path = String::from_utf8_lossy(&name)
        .trim_start_matches("./")
        .replace('\\', "/");
    // 110 字节头 + 文件名按 4 字节对齐，之后才是文件数据。
    let consumed = 110 + namesize;
    let padding = (4 - consumed % 4) % 4;
    if padding > 0 {
        let mut scratch = [0u8; 4];
        reader.read_exact(&mut scratch[..padding])?;
    }
    if path == "TRAILER!!!" {
        return Ok(None);
    }
    Ok(Some(CpioEntry { path, mode, size }))
}

/// 跳过 cpio 数据后的 4 字节对齐填充。
pub(crate) fn skip_cpio_padding<R: Read>(reader: &mut R, size: u64) -> Result<(), ArchiveError> {
    let padding = (4 - size % 4) % 4;
    if padding > 0 {
        let mut scratch = [0u8; 4];
        reader.read_exact(&mut scratch[..padding as usize])?;
    }
    Ok(())
}

/// 解析 8 位十六进制字段。
fn parse_hex(bytes: &[u8]) -> Result<u64, ArchiveError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ArchiveError::Corrupt("RPM cpio 字段不是 ASCII".to_string()))?;
    u64::from_str_radix(text, 16)
        .map_err(|_| ArchiveError::Corrupt(format!("RPM cpio 字段无效：{text}")))
}
