//! 归档条目和流式归档写入器。
use super::*;

fn start_block_for_method(compressor: *mut sys::Compressor, method: &str) -> Result<()> {
    let method_trim = method.trim();
    if method_trim.is_empty() {
        return Err(ZpaqError::Ffi("method string is empty".into()));
    }
    let rc = if let Ok(level) = method_trim.parse::<i32>() {
        if !(1..=5).contains(&level) {
            return Err(ZpaqError::Ffi(
                "numeric method level must be in 1..=5".into(),
            ));
        }
        unsafe { sys::zpaq_compressor_start_block_level(compressor, level) }
    } else {
        let method_c = CString::new(method_trim).map_err(|_| ZpaqError::NulInString)?;
        unsafe { sys::zpaq_compressor_start_block_method(compressor, method_c.as_ptr()) }
    };

    if rc == 0 {
        Ok(())
    } else {
        Err(err_from_last())
    }
}

/// One in-archive file entry represented as raw bytes.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveEntry<'a> {
    /// File path to store in the archive.
    pub path: &'a str,
    /// File contents.
    pub data: &'a [u8],
    /// Optional per-segment comment.
    pub comment: Option<&'a str>,
}

/// Creates a ZPAQ stream archive in memory from raw byte entries.
///
/// This performs no scratch-file I/O and writes each entry with its `path`
/// stored as the segment filename.
pub fn archive_from_entries(entries: &[ArchiveEntry<'_>], method: &str) -> Result<Vec<u8>> {
    clear_last_error();
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let compressor = unsafe { sys::zpaq_compressor_new() };
    if compressor.is_null() {
        return Err(err_from_last());
    }

    let out_shared = SharedVecWriter::new();
    let out_writer = FfiWriter::new(out_shared.clone())?;

    let set_out = unsafe { sys::zpaq_compressor_set_output(compressor, out_writer.raw) };
    if set_out != 0 {
        unsafe { sys::zpaq_compressor_free(compressor) };
        return Err(err_from_last());
    }

    let rc_tag = unsafe { sys::zpaq_compressor_write_tag(compressor) };
    if rc_tag != 0 {
        unsafe { sys::zpaq_compressor_free(compressor) };
        return Err(err_from_last());
    }

    start_block_for_method(compressor, method)?;

    for entry in entries {
        let filename_c = CString::new(entry.path).map_err(|_| ZpaqError::NulInString)?;
        let comment_c = match entry.comment {
            Some(text) => Some(CString::new(text).map_err(|_| ZpaqError::NulInString)?),
            None => None,
        };

        let rc_seg = unsafe {
            sys::zpaq_compressor_start_segment(
                compressor,
                filename_c.as_ptr(),
                comment_c
                    .as_ref()
                    .map(|s| s.as_ptr())
                    .unwrap_or(ptr::null()),
            )
        };
        if rc_seg != 0 {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(err_from_last());
        }

        let input = FfiReader::new(std::io::Cursor::new(entry.data))?;
        let rc_in = unsafe { sys::zpaq_compressor_set_input(compressor, input.raw) };
        if rc_in != 0 {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(err_from_last());
        }

        loop {
            let rc = unsafe { sys::zpaq_compressor_compress(compressor, 1 << 20) };
            if rc < 0 {
                unsafe { sys::zpaq_compressor_free(compressor) };
                return Err(err_from_last());
            }
            if rc == 0 {
                break;
            }
        }

        let rc_end_seg = unsafe { sys::zpaq_compressor_end_segment(compressor, ptr::null()) };
        if rc_end_seg != 0 {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(err_from_last());
        }
    }

    let rc_end_block = unsafe { sys::zpaq_compressor_end_block(compressor) };
    unsafe { sys::zpaq_compressor_free(compressor) };
    if rc_end_block != 0 {
        return Err(err_from_last());
    }

    drop(out_writer);
    Ok(out_shared.bytes())
}

/// Appends raw byte entries to an archive file path without creating scratch files.
pub fn archive_append_entries_file(
    archive_path: &str,
    entries: &[ArchiveEntry<'_>],
    method: &str,
) -> Result<()> {
    let payload = archive_from_entries(entries, method)?;
    if payload.is_empty() {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(archive_path)
        .map_err(|e| ZpaqError::Ffi(format!("open archive for append failed: {e}")))?;
    file.write_all(&payload)
        .map_err(|e| ZpaqError::Ffi(format!("append archive write failed: {e}")))
}

/// Reads the newest segment whose stored filename matches `path` from an
/// in-memory archive stream.
fn archive_read_file_bytes_single_stream(archive: &[u8], path: &str) -> Result<Option<Vec<u8>>> {
    clear_last_error();

    let reader = FfiReader::new(std::io::Cursor::new(archive))?;
    let decompresser = unsafe { sys::zpaq_decompresser_new() };
    if decompresser.is_null() {
        return Err(err_from_last());
    }

    let set_in = unsafe { sys::zpaq_decompresser_set_input(decompresser, reader.raw) };
    if set_in != 0 {
        unsafe { sys::zpaq_decompresser_free(decompresser) };
        return Err(err_from_last());
    }

    let mut found: Option<Vec<u8>> = None;
    let mut mem_out = 0.0f64;

    loop {
        let rc_block = unsafe { sys::zpaq_decompresser_find_block(decompresser, &mut mem_out) };
        if rc_block < 0 {
            unsafe { sys::zpaq_decompresser_free(decompresser) };
            return Err(err_from_last());
        }
        if rc_block == 0 {
            break;
        }

        loop {
            let filename_shared = SharedVecWriter::new();
            let filename_writer = FfiWriter::new(filename_shared.clone())?;
            let rc_filename =
                unsafe { sys::zpaq_decompresser_find_filename(decompresser, filename_writer.raw) };
            if rc_filename < 0 {
                unsafe { sys::zpaq_decompresser_free(decompresser) };
                return Err(err_from_last());
            }
            if rc_filename == 0 {
                break;
            }
            drop(filename_writer);

            let comment_writer = FfiWriter::new(SinkWriter)?;
            let rc_comment =
                unsafe { sys::zpaq_decompresser_read_comment(decompresser, comment_writer.raw) };
            if rc_comment != 0 {
                unsafe { sys::zpaq_decompresser_free(decompresser) };
                return Err(err_from_last());
            }
            drop(comment_writer);

            let mut filename_bytes = filename_shared.bytes();
            while filename_bytes.last().copied() == Some(0) {
                filename_bytes.pop();
            }
            let filename = String::from_utf8_lossy(&filename_bytes);
            let is_target = filename == path;

            let output_shared = SharedVecWriter::new();
            let output_sink: Box<dyn Write + Send> = if is_target {
                Box::new(output_shared.clone())
            } else {
                Box::new(SinkWriter)
            };
            let output_writer = FfiWriter::new(output_sink)?;

            let rc_set_out =
                unsafe { sys::zpaq_decompresser_set_output(decompresser, output_writer.raw) };
            if rc_set_out != 0 {
                unsafe { sys::zpaq_decompresser_free(decompresser) };
                return Err(err_from_last());
            }

            loop {
                let rc_decompress =
                    unsafe { sys::zpaq_decompresser_decompress(decompresser, 1 << 20) };
                if rc_decompress < 0 {
                    unsafe { sys::zpaq_decompresser_free(decompresser) };
                    return Err(err_from_last());
                }
                if rc_decompress == 0 {
                    break;
                }
            }

            let mut segment_end = [0u8; 21];
            let rc_end = unsafe {
                sys::zpaq_decompresser_read_segment_end(decompresser, segment_end.as_mut_ptr())
            };
            if rc_end != 0 {
                unsafe { sys::zpaq_decompresser_free(decompresser) };
                return Err(err_from_last());
            }

            drop(output_writer);

            if is_target {
                found = Some(output_shared.bytes());
            }
        }
    }

    unsafe { sys::zpaq_decompresser_free(decompresser) };
    Ok(found)
}

/// Reads the newest segment whose stored filename matches `path` from an
/// archive byte slice.
///
/// Supports concatenated ZPAQ streams (e.g. repeated append operations).
pub fn archive_read_file_bytes(archive: &[u8], path: &str) -> Result<Vec<u8>> {
    const ZPAQ_TAG: [u8; 13] = [
        0x37, 0x6b, 0x53, 0x74, 0xa0, 0x31, 0x83, 0xd3, 0x8c, 0xb2, 0x28, 0xb0, 0xd3,
    ];

    if archive.is_empty() {
        return Err(ZpaqError::Ffi("archive is empty".into()));
    }

    let mut starts = Vec::new();
    for index in 0..=archive.len().saturating_sub(ZPAQ_TAG.len()) {
        if archive[index..].starts_with(&ZPAQ_TAG) {
            starts.push(index);
        }
    }
    if starts.is_empty() {
        return Err(ZpaqError::Ffi("no ZPAQ stream header found".into()));
    }

    let mut latest = None;
    for (position, start) in starts.iter().copied().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(archive.len());
        if end <= start {
            continue;
        }
        if let Some(bytes) = archive_read_file_bytes_single_stream(&archive[start..end], path)? {
            latest = Some(bytes);
        }
    }

    latest.ok_or_else(|| ZpaqError::Ffi(format!("file path not found in archive: {path}")))
}

/// Reads bytes for `path` from an archive file.
pub fn archive_read_file_bytes_from_file(archive_path: &str, path: &str) -> Result<Vec<u8>> {
    let archive = std::fs::read(archive_path)
        .map_err(|e| ZpaqError::Ffi(format!("read archive file failed: {e}")))?;
    archive_read_file_bytes(&archive, path)
}

/// 把 ZPAQ 流写入加密容器：前 32 字节为盐，其后为 AES-CTR 密文。
struct EncryptedWriter<W: Write> {
    /// 底层输出。
    writer: W,
    /// libzpaq AES-CTR 状态。
    aes: *mut sys::AES_CTR,
    /// 下一个明文字节在归档中的绝对偏移。
    offset: u64,
}

unsafe impl<W: Write + Send> Send for EncryptedWriter<W> {}

impl<W: Write> EncryptedWriter<W> {
    /// 写入盐并建立 AES-CTR 状态。
    fn new(mut writer: W, password: &str) -> Result<Self> {
        let salt = random_bytes(32)?;
        let mut salt_bytes = [0u8; 32];
        salt_bytes.copy_from_slice(&salt);
        let password_hash = sha256(password.as_bytes())?;
        let key = stretch_key(password_hash, salt_bytes)?;
        let aes = unsafe {
            sys::zpaq_aes_ctr_new(
                key.as_ptr() as *const c_char,
                32,
                salt_bytes.as_ptr() as *const c_char,
            )
        };
        if aes.is_null() {
            return Err(err_from_last());
        }
        writer
            .write_all(&salt_bytes)
            .map_err(|e| ZpaqError::Ffi(format!("write ZPAQ salt failed: {e}")))?;
        Ok(Self {
            writer,
            aes,
            offset: 32,
        })
    }
}

impl<W: Write> Write for EncryptedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut encrypted = buf.to_vec();
        let rc = unsafe {
            sys::zpaq_aes_ctr_encrypt_slice(
                self.aes,
                encrypted.as_mut_ptr() as *mut c_char,
                encrypted.len() as c_int,
                self.offset,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::other("ZPAQ AES-CTR 加密失败"));
        }
        self.writer.write_all(&encrypted)?;
        self.offset = self.offset.saturating_add(encrypted.len() as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> Drop for EncryptedWriter<W> {
    fn drop(&mut self) {
        unsafe { sys::zpaq_aes_ctr_free(self.aes) };
    }
}

/// 直接写入任意 `Write` 的 ZPAQ 流式归档写入器。
///
/// 与 `archive_from_entries` 不同，本写入器不会把所有条目聚合到内存，而是
/// 一次只接收一个条目，压缩结果立即写入底层 writer。生成的归档仍是标准
/// ZPAQ 流格式，`zpaq list` / `zpaq extract` 可读取其中的多个 segment。
pub struct ArchiveWriter<'a> {
    /// libzpaq 压缩器；其输出回调指向 `writer`。
    compressor: *mut sys::Compressor,
    /// 持有输出 writer 和 FFI 回调上下文，必须晚于压缩器释放。
    writer: Option<FfiWriter<Box<dyn Write + Send + 'a>>>,
    /// 是否已正常结束块，防止 Drop 重复释放。
    finished: bool,
}

unsafe impl Send for ArchiveWriter<'_> {}

impl<'a> ArchiveWriter<'a> {
    /// 创建流式归档，写入标签、开启一个块，等待 `add_entry`。
    pub fn new<W: Write + Send + 'a>(writer: W, method: &str) -> Result<Self> {
        let compressor = unsafe { sys::zpaq_compressor_new() };
        if compressor.is_null() {
            return Err(err_from_last());
        }
        let ffi_writer = match FfiWriter::new(Box::new(writer) as Box<dyn Write + Send + 'a>) {
            Ok(value) => value,
            Err(error) => {
                unsafe { sys::zpaq_compressor_free(compressor) };
                return Err(error);
            }
        };
        let set_output = unsafe { sys::zpaq_compressor_set_output(compressor, ffi_writer.raw) };
        if set_output != 0 {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(err_from_last());
        }
        let tag = unsafe { sys::zpaq_compressor_write_tag(compressor) };
        if tag != 0 {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(err_from_last());
        }
        if let Err(error) = start_block_for_method(compressor, method) {
            unsafe { sys::zpaq_compressor_free(compressor) };
            return Err(error);
        }
        Ok(Self {
            compressor,
            writer: Some(ffi_writer),
            finished: false,
        })
    }

    /// 创建带密码的流式归档；盐写入底层输出，后续内容使用 AES-CTR 加密。
    pub fn new_encrypted<W: Write + Send + 'a>(
        writer: W,
        method: &str,
        password: &str,
    ) -> Result<Self> {
        Self::new(EncryptedWriter::new(writer, password)?, method)
    }

    /// 追加一个文件条目，数据从 `reader` 流式压缩。
    pub fn add_entry<R: Read + Send>(
        &mut self,
        path: &str,
        comment: Option<&str>,
        reader: R,
    ) -> Result<()> {
        let path = CString::new(path).map_err(|_| ZpaqError::NulInString)?;
        let comment = match comment {
            Some(value) => Some(CString::new(value).map_err(|_| ZpaqError::NulInString)?),
            None => None,
        };
        let rc = unsafe {
            sys::zpaq_compressor_start_segment(
                self.compressor,
                path.as_ptr(),
                comment.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            )
        };
        if rc != 0 {
            return Err(err_from_last());
        }
        let input = FfiReader::new(reader)?;
        let rc = unsafe { sys::zpaq_compressor_set_input(self.compressor, input.raw) };
        if rc != 0 {
            return Err(err_from_last());
        }
        loop {
            let rc = unsafe { sys::zpaq_compressor_compress(self.compressor, 1 << 20) };
            if rc < 0 {
                return Err(err_from_last());
            }
            if rc == 0 {
                break;
            }
        }
        let rc = unsafe { sys::zpaq_compressor_end_segment(self.compressor, ptr::null()) };
        if rc != 0 {
            return Err(err_from_last());
        }
        Ok(())
    }

    /// 结束当前块并释放压缩器；输出 writer 在此后刷新。
    pub fn finish(mut self) -> Result<()> {
        let rc = unsafe { sys::zpaq_compressor_end_block(self.compressor) };
        unsafe { sys::zpaq_compressor_free(self.compressor) };
        self.compressor = ptr::null_mut();
        self.finished = true;
        if rc == 0 {
            Ok(())
        } else {
            Err(err_from_last())
        }
    }
}

impl Drop for ArchiveWriter<'_> {
    fn drop(&mut self) {
        if !self.finished && !self.compressor.is_null() {
            unsafe { sys::zpaq_compressor_free(self.compressor) };
            self.compressor = ptr::null_mut();
        }
        // writer 字段最后释放，确保压缩器回调上下文仍然有效。
        let _ = self.writer.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 流式写入两个条目后应能读回，且不依赖内存聚合。
    #[test]
    fn archive_writer_streams_entries() {
        let mut output = Vec::new();
        {
            let mut writer = ArchiveWriter::new(&mut output, "1").unwrap();
            writer
                .add_entry("first.txt", None, std::io::Cursor::new(b"first"))
                .unwrap();
            writer
                .add_entry("second.txt", None, std::io::Cursor::new(b"second"))
                .unwrap();
            writer.finish().unwrap();
        }
        assert_eq!(
            archive_read_file_bytes(&output, "first.txt").unwrap(),
            b"first"
        );
        assert_eq!(
            archive_read_file_bytes(&output, "second.txt").unwrap(),
            b"second"
        );
    }

    /// 加密流式归档应能被官方 zpaq list 用相同密码读取。
    #[test]
    fn encrypted_archive_writer_is_listable() {
        let path = std::env::temp_dir().join(format!(
            "lumark-zpaq-stream-{}-{}.zpaq",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut writer = ArchiveWriter::new_encrypted(file, "1", "secret").unwrap();
            writer
                .add_entry("secret.txt", None, std::io::Cursor::new(b"hidden"))
                .unwrap();
            writer.finish().unwrap();
        }
        let path_string = path.to_string_lossy().into_owned();
        let listing = zpaq_command(&["list", &path_string, "-key", "secret"]).unwrap();
        assert!(
            listing.stdout.contains("secret.txt"),
            "listing: {}",
            listing.stdout
        );
        let _ = std::fs::remove_file(path);
    }
}
