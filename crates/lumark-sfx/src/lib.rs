//! 自解压运行时模块。
//!
//! 生成端把本程序作为前缀、ZIP 数据作为后缀，并在文件末尾写入 24 字节页脚
//! （`LMSFX001` 魔数 + 数据段偏移 + 数据段长度，均为小端 `u64`）。运行本
//! 程序时从自身文件读取 ZIP 数据，解压到可执行文件旁边的同名文件夹。
//!
//! 页脚格式由 `lumark-archive` 写入，两边必须保持一致。

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 页脚魔数，用于区分自解压包和普通可执行文件。
pub const SFX_MAGIC: [u8; 8] = *b"LMSFX001";

/// 页脚总长度：魔数 8 字节 + 偏移 8 字节 + 长度 8 字节。
pub const FOOTER_LEN: usize = 24;

/// 自解压数据段在文件中的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfxFooter {
    /// ZIP 数据段起始偏移，等于运行时模块的字节数。
    pub offset: u64,
    /// ZIP 数据段长度。
    pub length: u64,
}

/// 解析文件末尾的页脚；长度不足或魔数不匹配时返回 `None`。
pub fn parse_footer(tail: &[u8]) -> Option<SfxFooter> {
    let footer = tail.get(tail.len().checked_sub(FOOTER_LEN)?..)?;
    if footer[..8] != SFX_MAGIC {
        return None;
    }
    let offset = u64::from_le_bytes(footer[8..16].try_into().ok()?);
    let length = u64::from_le_bytes(footer[16..24].try_into().ok()?);
    Some(SfxFooter { offset, length })
}

/// 读取已打开文件末尾的页脚。
pub fn read_footer(file: &mut File) -> io::Result<Option<SfxFooter>> {
    let total = file.metadata()?.len();
    if total < FOOTER_LEN as u64 {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut tail = [0u8; FOOTER_LEN];
    file.read_exact(&mut tail)?;
    Ok(parse_footer(&tail))
}

/// 自解压包的默认解压目录：与可执行文件同名的文件夹。
pub fn default_target(exe: &Path) -> PathBuf {
    let parent = exe.parent().unwrap_or_else(|| Path::new("."));
    let stem = exe
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("lumark-sfx");
    parent.join(stem)
}

/// 把页脚指向的 ZIP 数据解压到目标目录，返回解压出的文件数。
///
/// 只读取数据段并按需解压，不把整个自解压包读入内存；条目路径必须是相对
/// 路径，出现 `..`、盘符或绝对路径时整体失败，避免写出目标目录。
pub fn extract_payload(file: File, footer: SfxFooter, target: &Path) -> Result<usize, String> {
    let mut reader = PayloadReader {
        file,
        start: footer.offset,
        len: footer.length,
        pos: 0,
    };
    let mut archive =
        zip::ZipArchive::new(&mut reader).map_err(|error| format!("自解压数据损坏：{error}"))?;
    fs::create_dir_all(target).map_err(|error| format!("无法创建解压目录：{error}"))?;
    let mut files = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("无法读取第 {index} 个条目：{error}"))?;
        let name = entry
            .name()
            .map_err(|error| format!("无法读取第 {index} 个条目名：{error}"))?
            .into_owned();
        let Some(relative) = safe_relative(&name) else {
            return Err(format!("条目路径不安全：{name}"));
        };
        let destination = target.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("无法创建目录 {}：{error}", destination.display()))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建目录 {}：{error}", parent.display()))?;
        }
        let mut output = File::create(&destination)
            .map_err(|error| format!("无法写入 {}：{error}", destination.display()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| format!("解压 {} 失败：{error}", destination.display()))?;
        files += 1;
    }
    Ok(files)
}

/// 把归档条目名转换成目标目录内的安全相对路径。
fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for part in name.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        relative.push(part);
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// 只暴露文件中数据段的读写视图，供 ZIP 解码器顺序读取。
struct PayloadReader {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl Read for PayloadReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let remaining = (self.len - self.pos) as usize;
        let limit = buffer.len().min(remaining);
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let read = self.file.read(&mut buffer[..limit])?;
        self.pos += read as u64;
        Ok(read)
    }
}

impl Seek for PayloadReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::Current(value) => self.pos as i64 + value,
            SeekFrom::End(value) => self.len as i64 + value,
        };
        if target < 0 || target as u64 > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "自解压数据段越界",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 组装测试页脚，与 `lumark-archive` 的写入格式保持一致。
    fn footer_bytes(offset: u64, length: u64) -> Vec<u8> {
        let mut footer = Vec::new();
        footer.extend_from_slice(&SFX_MAGIC);
        footer.extend_from_slice(&offset.to_le_bytes());
        footer.extend_from_slice(&length.to_le_bytes());
        footer
    }

    /// 页脚解析应读回偏移和长度，魔数不符时返回 None。
    #[test]
    fn footer_round_trips() {
        let bytes = footer_bytes(4096, 128);
        assert_eq!(
            parse_footer(&bytes),
            Some(SfxFooter {
                offset: 4096,
                length: 128
            })
        );
        let mut corrupted = bytes.clone();
        corrupted[0] = b'X';
        assert_eq!(parse_footer(&corrupted), None);
        assert_eq!(parse_footer(&bytes[..10]), None);
    }

    /// 默认解压目录取可执行文件同目录的同名文件夹。
    #[test]
    fn default_target_uses_exe_stem() {
        let target = default_target(Path::new(r"C:\downloads\photos.exe"));
        assert_eq!(target, PathBuf::from(r"C:\downloads\photos"));
    }

    /// 解压应跳过运行时模块本身，只写出 ZIP 内容到目标目录。
    #[test]
    fn extraction_reads_payload_after_stub() {
        let directory = tempfile::tempdir().unwrap();
        let payload_path = directory.path().join("payload.zip");
        {
            let file = File::create(&payload_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("hello.txt", options).unwrap();
            writer.write_all(b"hello sfx").unwrap();
            writer.finish().unwrap();
        }
        let payload = fs::read(&payload_path).unwrap();
        let stub = b"MZfake-stub-bytes";
        let sfx_path = directory.path().join("sample.exe");
        {
            let mut file = File::create(&sfx_path).unwrap();
            file.write_all(stub).unwrap();
            file.write_all(&payload).unwrap();
            file.write_all(&footer_bytes(stub.len() as u64, payload.len() as u64))
                .unwrap();
        }
        let mut file = File::open(&sfx_path).unwrap();
        let footer = read_footer(&mut file).unwrap().expect("页脚");
        assert_eq!(footer.offset, stub.len() as u64);
        let target = directory.path().join("out");
        let files = extract_payload(file, footer, &target).unwrap();
        assert_eq!(files, 1);
        assert_eq!(
            fs::read_to_string(target.join("hello.txt")).unwrap(),
            "hello sfx"
        );
    }

    /// 带 `..` 的条目名必须被拒绝，不能写出目标目录。
    #[test]
    fn unsafe_entry_names_are_rejected() {
        assert!(safe_relative("../escape.txt").is_none());
        assert!(safe_relative("C:/absolute.txt").is_none());
        assert!(safe_relative("folder/../../escape.txt").is_none());
        assert_eq!(
            safe_relative("folder/file.txt"),
            Some(PathBuf::from("folder").join("file.txt"))
        );
    }
}
