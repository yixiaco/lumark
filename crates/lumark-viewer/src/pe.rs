//! 最小但健壮的 PE32/PE32+ 结构解析。

use std::path::Path;

use thiserror::Error;

use crate::source::{ContentKind, SourceBytes, SourceError, classify_content, open_source};

/// PE 解析错误。
#[derive(Debug, Error)]
pub enum PeError {
    /// 来源读取失败。
    #[error(transparent)]
    Source(#[from] SourceError),
    /// 文件不是 PE。
    #[error("文件不是 PE32/PE32+ 可执行文件")]
    NotPe,
    /// 结构越界或字段非法。
    #[error("PE 结构损坏：{0}")]
    Corrupt(&'static str),
}

/// PE 节表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeSection {
    /// 节名，最多 8 字节。
    pub name: String,
    /// 虚拟地址。
    pub virtual_address: u32,
    /// 虚拟大小。
    pub virtual_size: u32,
    /// 文件原始偏移。
    pub raw_offset: u32,
    /// 文件原始大小。
    pub raw_size: u32,
    /// 节特性位。
    pub characteristics: u32,
}

/// PE 文件概要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeInfo {
    /// COFF machine。
    pub machine: u16,
    /// 是否为 PE32+。
    pub is_64: bool,
    /// 入口点 RVA。
    pub entry_point: u32,
    /// 映像基址。
    pub image_base: u64,
    /// 子系统。
    pub subsystem: u16,
    /// 节表。
    pub sections: Vec<PeSection>,
    /// 证书表文件偏移和大小。
    pub certificate: Option<(u32, u32)>,
    /// 覆盖数据起点；没有覆盖数据时为 `None`。
    pub overlay_offset: Option<u64>,
    /// 文件总大小。
    pub file_size: u64,
}

impl PeInfo {
    /// 机器类型的中文显示名。
    pub fn machine_label(&self) -> &'static str {
        match self.machine {
            0x014c => "x86 (i386)",
            0x8664 => "x64 (AMD64)",
            0x01c0 => "ARM",
            0xaa64 => "ARM64",
            0x0200 => "IA64",
            _ => "未知架构",
        }
    }

    /// 位数显示名。
    pub fn bitness_label(&self) -> &'static str {
        if self.is_64 { "PE32+" } else { "PE32" }
    }

    /// 节区总可执行/可写特性文本。
    pub fn section_flags(section: &PeSection) -> String {
        let mut flags = Vec::new();
        if section.characteristics & 0x2000_0000 != 0 {
            flags.push("可执行");
        }
        if section.characteristics & 0x8000_0000 != 0 {
            flags.push("可写");
        }
        if section.characteristics & 0x4000_0000 != 0 {
            flags.push("可读");
        }
        flags.join(" / ")
    }
}

/// 已打开的 PE 文件。
pub struct PeImage {
    /// 解析后的结构。
    pub info: PeInfo,
    /// 原始字节。
    source: SourceBytes,
}

impl PeImage {
    /// 打开并解析 PE。
    pub fn open(path: &Path) -> Result<Self, PeError> {
        let source = open_source(path)?;
        if classify_content(source.as_slice(), path) != ContentKind::Pe {
            return Err(PeError::NotPe);
        }
        let info = parse_pe(source.as_slice())?;
        Ok(Self { info, source })
    }

    /// 从内存解析。
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, PeError> {
        let source = SourceBytes::Owned(bytes);
        if classify_content(source.as_slice(), Path::new("")) != ContentKind::Pe {
            return Err(PeError::NotPe);
        }
        let info = parse_pe(source.as_slice())?;
        Ok(Self { info, source })
    }

    /// 原始字节。
    pub fn bytes(&self) -> &[u8] {
        self.source.as_slice()
    }

    /// 文件大小。
    pub fn file_size(&self) -> u64 {
        self.source.len() as u64
    }
}

/// 解析 PE 头、可选头和节表。
pub fn parse_pe(bytes: &[u8]) -> Result<PeInfo, PeError> {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return Err(PeError::NotPe);
    }
    let pe_offset = read_u32(bytes, 0x3c)? as usize;
    if pe_offset
        .checked_add(24)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(PeError::Corrupt("PE 头偏移越界"));
    }
    if &bytes[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Err(PeError::NotPe);
    }
    let coff = pe_offset + 4;
    let machine = read_u16(bytes, coff)?;
    let section_count = read_u16(bytes, coff + 2)? as usize;
    let optional_size = read_u16(bytes, coff + 16)? as usize;
    let optional = coff + 20;
    let optional_end = optional
        .checked_add(optional_size)
        .filter(|end| *end <= bytes.len())
        .ok_or(PeError::Corrupt("可选头越界"))?;
    let magic = read_u16(bytes, optional)?;
    let is_64 = match magic {
        0x10b => false,
        0x20b => true,
        _ => return Err(PeError::Corrupt("未知 PE 可选头 magic")),
    };
    let entry_point = read_u32(bytes, optional + 16)?;
    let image_base = if is_64 {
        read_u64(bytes, optional + 24)?
    } else {
        u64::from(read_u32(bytes, optional + 28)?)
    };
    let subsystem = read_u16(bytes, optional + 68)?;
    let directory_count_offset = optional + if is_64 { 108 } else { 92 };
    let directory_count = read_u32(bytes, directory_count_offset)? as usize;
    let directory_offset = directory_count_offset + 4;
    let certificate = if directory_count > 4 {
        let offset = read_u32(bytes, directory_offset + 4 * 8)?;
        let size = read_u32(bytes, directory_offset + 4 * 8 + 4)?;
        (offset != 0 && size != 0).then_some((offset, size))
    } else {
        None
    };

    let section_table = optional_end;
    let mut sections = Vec::with_capacity(section_count.min(256));
    for index in 0..section_count.min(256) {
        let offset = section_table
            .checked_add(index * 40)
            .filter(|offset| offset + 40 <= bytes.len())
            .ok_or(PeError::Corrupt("节表越界"))?;
        let mut name = bytes[offset..offset + 8].to_vec();
        if let Some(end) = name.iter().position(|byte| *byte == 0) {
            name.truncate(end);
        }
        sections.push(PeSection {
            name: String::from_utf8_lossy(&name).into_owned(),
            virtual_size: read_u32(bytes, offset + 8)?,
            virtual_address: read_u32(bytes, offset + 12)?,
            raw_size: read_u32(bytes, offset + 16)?,
            raw_offset: read_u32(bytes, offset + 20)?,
            characteristics: read_u32(bytes, offset + 36)?,
        });
    }

    let mut end_of_image = 0u64;
    for section in &sections {
        end_of_image =
            end_of_image.max(u64::from(section.raw_offset) + u64::from(section.raw_size));
    }
    if let Some((certificate_offset, certificate_size)) = certificate {
        end_of_image =
            end_of_image.max(u64::from(certificate_offset) + u64::from(certificate_size));
    }
    let file_size = bytes.len() as u64;
    let overlay_offset = (end_of_image < file_size && end_of_image > 0).then_some(end_of_image);

    Ok(PeInfo {
        machine,
        is_64,
        entry_point,
        image_base,
        subsystem,
        sections,
        certificate,
        overlay_offset,
        file_size,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PeError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or(PeError::Corrupt("读取 u16 越界"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PeError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(PeError::Corrupt("读取 u32 越界"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, PeError> {
    let slice = bytes
        .get(offset..offset + 8)
        .ok_or(PeError::Corrupt("读取 u64 越界"))?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_pe(is_64: bool) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x200];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        bytes[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        let optional_size: u16 = if is_64 { 0xf0 } else { 0xe0 };
        bytes[coff + 16..coff + 18].copy_from_slice(&optional_size.to_le_bytes());
        let optional = coff + 20;
        let magic: u16 = if is_64 { 0x20b } else { 0x10b };
        bytes[optional..optional + 2].copy_from_slice(&magic.to_le_bytes());
        bytes[optional + 16..optional + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        if is_64 {
            bytes[optional + 24..optional + 32].copy_from_slice(&0x140000000u64.to_le_bytes());
        } else {
            bytes[optional + 28..optional + 32].copy_from_slice(&0x400000u32.to_le_bytes());
        }
        bytes[optional + 68..optional + 70].copy_from_slice(&3u16.to_le_bytes());
        let section = optional + optional_size as usize;
        bytes[section..section + 5].copy_from_slice(b".text");
        bytes[section + 8..section + 12].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[section + 16..section + 20].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_pe32_plus_section() {
        let image = PeImage::from_bytes(minimal_pe(true)).unwrap();
        assert!(image.info.is_64);
        assert_eq!(image.info.machine_label(), "x64 (AMD64)");
        assert_eq!(image.info.sections[0].name, ".text");
        assert_eq!(image.info.sections[0].raw_offset, 0x200);
        assert_eq!(image.info.overlay_offset, None);
    }

    #[test]
    fn parses_pe32() {
        let image = PeImage::from_bytes(minimal_pe(false)).unwrap();
        assert!(!image.info.is_64);
        assert_eq!(image.info.bitness_label(), "PE32");
        assert_eq!(image.info.image_base, 0x400000);
    }

    #[test]
    fn rejects_non_pe() {
        assert!(matches!(
            PeImage::from_bytes(b"hello".to_vec()),
            Err(PeError::NotPe)
        ));
    }
}
