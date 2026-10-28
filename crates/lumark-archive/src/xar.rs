//! XAR/PKG 只读解析。
//!
//! XAR 由 28 字节头、zlib 压缩的 XML 目录和紧随其后的数据堆组成；每个文件
//! 的 `<data>` 记录堆内偏移、压缩长度和编码。这里只实现读取、列表、测试和
//! 解压，不写入 XAR。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use bzip2::read::BzDecoder;
use flate2::read::ZlibDecoder;
use quick_xml::Reader;
use quick_xml::events::Event;
use xz2::read::XzDecoder;

use crate::error::ArchiveError;

/// XAR 头部长度。
const XAR_HEADER_LEN: u64 = 28;
/// 允许解压的目录 XML 上限。
const MAX_TOC_BYTES: u64 = 64 * 1024 * 1024;

/// 单个 XAR 条目的元数据。
pub(crate) struct XarEntry {
    /// 归档内路径。
    pub(crate) path: String,
    /// 是否目录。
    pub(crate) is_dir: bool,
    /// 是否符号链接。
    pub(crate) is_symlink: bool,
    /// 原始大小。
    pub(crate) size: u64,
    /// 压缩后字节数。
    pub(crate) compressed_size: u64,
    /// 压缩方法展示名。
    pub(crate) method: String,
    /// 数据在堆内的偏移。
    pub(crate) data_offset: u64,
    /// 数据编码。
    encoding: XarEncoding,
    /// 符号链接目标。
    pub(crate) link: Option<String>,
}

/// XAR 数据编码。
#[derive(Clone, Copy, Default)]
enum XarEncoding {
    /// 未压缩。
    #[default]
    None,
    /// zlib。
    Zlib,
    /// bzip2。
    Bzip2,
    /// xz。
    Xz,
}

/// 已打开的 XAR 归档。
pub(crate) struct XarArchive {
    /// 归档文件。
    file: File,
    /// 数据堆起始偏移。
    heap_offset: u64,
    /// 条目列表。
    entries: Vec<XarEntry>,
}

impl XarArchive {
    /// 打开并解析 XAR 头部和目录。
    pub(crate) fn open(path: &Path) -> Result<Self, ArchiveError> {
        let mut file = File::open(path)?;
        let mut header = [0u8; XAR_HEADER_LEN as usize];
        file.read_exact(&mut header)?;
        if &header[0..4] != b"xar!" {
            return Err(ArchiveError::Corrupt("不是 XAR 文件".to_string()));
        }
        let header_size = u16::from_be_bytes([header[4], header[5]]) as u64;
        let toc_compressed = u64::from_be_bytes(header[8..16].try_into().unwrap());
        let toc_uncompressed = u64::from_be_bytes(header[16..24].try_into().unwrap());
        if header_size < XAR_HEADER_LEN
            || toc_compressed == 0
            || toc_compressed > MAX_TOC_BYTES
            || toc_uncompressed > MAX_TOC_BYTES
        {
            return Err(ArchiveError::Corrupt("XAR 目录长度异常".to_string()));
        }
        file.seek(SeekFrom::Start(header_size))?;
        let mut compressed = Vec::with_capacity(toc_compressed as usize);
        file.by_ref()
            .take(toc_compressed)
            .read_to_end(&mut compressed)?;
        let mut xml = Vec::new();
        ZlibDecoder::new(compressed.as_slice())
            .take(MAX_TOC_BYTES)
            .read_to_end(&mut xml)?;
        if xml.is_empty() {
            return Err(ArchiveError::Corrupt("XAR 目录为空".to_string()));
        }
        let entries = parse_toc(&xml)?;
        let heap_offset = header_size
            .checked_add(toc_compressed)
            .ok_or_else(|| ArchiveError::Corrupt("XAR 堆偏移溢出".to_string()))?;
        let file_len = file.metadata()?.len();
        if heap_offset > file_len {
            return Err(ArchiveError::Corrupt("XAR 堆偏移超出文件".to_string()));
        }
        Ok(Self {
            file,
            heap_offset,
            entries,
        })
    }

    /// 返回全部条目。
    pub(crate) fn entries(&self) -> &[XarEntry] {
        &self.entries
    }

    /// 打开条目数据的解压读取器。
    pub(crate) fn open_entry(&self, entry: &XarEntry) -> Result<Box<dyn Read>, ArchiveError> {
        if entry.is_dir {
            return Ok(Box::new(io::empty()));
        }
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(
            self.heap_offset
                .checked_add(entry.data_offset)
                .ok_or_else(|| ArchiveError::Corrupt("XAR 数据偏移溢出".to_string()))?,
        ))?;
        let limited = file.take(entry.compressed_size);
        let reader: Box<dyn Read> = match entry.encoding {
            XarEncoding::None => Box::new(limited),
            XarEncoding::Zlib => Box::new(ZlibDecoder::new(limited)),
            XarEncoding::Bzip2 => Box::new(BzDecoder::new(limited)),
            XarEncoding::Xz => Box::new(XzDecoder::new(limited)),
        };
        Ok(reader)
    }
}

/// 解析 XAR XML 目录。
fn parse_toc(xml: &[u8]) -> Result<Vec<XarEntry>, ArchiveError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack: Vec<XarBuilder> = Vec::new();
    let mut entries = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let tag = event.local_name().as_ref().to_vec();
                match tag.as_slice() {
                    b"file" => stack.push(XarBuilder::default()),
                    b"data" => {
                        if let Some(current) = stack.last_mut() {
                            current.in_data = true;
                        }
                    }
                    b"encoding" => {
                        if let Some(current) = stack.last_mut() {
                            for attribute in event.attributes().flatten() {
                                if attribute.key.local_name().as_ref() == b"style" {
                                    current.encoding =
                                        parse_encoding(&String::from_utf8_lossy(&attribute.value));
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(current) = stack.last_mut() {
                            current.capture = Some(tag);
                        }
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                if event.local_name().as_ref() == b"encoding"
                    && let Some(current) = stack.last_mut()
                {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.local_name().as_ref() == b"style" {
                            current.encoding =
                                parse_encoding(&String::from_utf8_lossy(&attribute.value));
                        }
                    }
                }
            }
            Ok(Event::Text(event)) => {
                if let Some(current) = stack.last_mut() {
                    let text = String::from_utf8_lossy(&event.into_inner()).to_string();
                    match current.capture.as_deref() {
                        Some(b"name") => current.name.push_str(&text),
                        Some(b"type") => current.kind.push_str(&text),
                        Some(b"size") => current.size_text.push_str(&text),
                        Some(b"offset") if current.in_data => current.offset_text.push_str(&text),
                        Some(b"length") if current.in_data => current.length_text.push_str(&text),
                        Some(b"link") => current.link.push_str(&text),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let tag = event.local_name().as_ref().to_vec();
                if tag == b"data" {
                    if let Some(current) = stack.last_mut() {
                        current.in_data = false;
                    }
                } else if tag == b"file" {
                    let builder = stack
                        .pop()
                        .ok_or_else(|| ArchiveError::Corrupt("XAR file 标签不平衡".to_string()))?;
                    let path = if let Some(parent) = stack.last() {
                        format!("{}/{}", parent.name.trim_matches('/'), builder.name)
                    } else {
                        builder.name
                    };
                    let size = builder.size_text.trim().parse::<u64>().unwrap_or(0);
                    let compressed_size = builder.length_text.trim().parse::<u64>().unwrap_or(size);
                    let data_offset = builder.offset_text.trim().parse::<u64>().unwrap_or(0);
                    let kind = builder.kind.trim();
                    let is_dir = kind == "directory";
                    let is_symlink = kind == "symlink";
                    entries.push(XarEntry {
                        path,
                        is_dir,
                        is_symlink,
                        size,
                        compressed_size,
                        method: encoding_label(builder.encoding).to_string(),
                        data_offset,
                        encoding: builder.encoding,
                        link: (!builder.link.trim().is_empty()).then_some(builder.link),
                    });
                } else if let Some(current) = stack.last_mut()
                    && current.capture.as_deref() == Some(tag.as_slice())
                {
                    current.capture = None;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(ArchiveError::Corrupt(format!(
                    "XAR 目录 XML 解析失败：{error}"
                )));
            }
        }
        buffer.clear();
    }
    if !stack.is_empty() {
        return Err(ArchiveError::Corrupt(
            "XAR 目录 file 标签未闭合".to_string(),
        ));
    }
    Ok(entries)
}

/// 解析 `<encoding style>`。
fn parse_encoding(style: &str) -> XarEncoding {
    match style.trim() {
        "application/x-bzip2" => XarEncoding::Bzip2,
        "application/x-xz" => XarEncoding::Xz,
        "" | "application/octet-stream" => XarEncoding::None,
        _ => XarEncoding::Zlib,
    }
}

/// 返回编码展示名。
fn encoding_label(encoding: XarEncoding) -> &'static str {
    match encoding {
        XarEncoding::None => "Store",
        XarEncoding::Zlib => "zlib",
        XarEncoding::Bzip2 => "BZip2",
        XarEncoding::Xz => "XZ",
    }
}

/// 解析中的 file 节点。
#[derive(Default)]
struct XarBuilder {
    /// 文件名。
    name: String,
    /// 类型文本。
    kind: String,
    /// 大小文本。
    size_text: String,
    /// 数据偏移文本。
    offset_text: String,
    /// 数据长度文本。
    length_text: String,
    /// 链接目标。
    link: String,
    /// 当前编码。
    encoding: XarEncoding,
    /// 是否处于 data 段。
    in_data: bool,
    /// 当前捕获的标签。
    capture: Option<Vec<u8>>,
}
