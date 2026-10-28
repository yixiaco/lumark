//! 伪装容器：把归档或普通文件包装成 MP4/JPG 媒体外观，并记录还原信息。
//!
//! 容器布局固定为三段：
//!
//! ```text
//! [0, mask_len)                       面具字节（原样，真实媒体文件）
//! [mask_len, mask_len + payload_len)  载荷（原归档/原文件字节，BLAKE3 密钥流混淆）
//! [file_len - 512, file_len)          512 字节尾部信息
//! ```
//!
//! 尾部信息使用固定 512 字节：魔数 8 字节、版本 1 字节、标志 1 字节、保留
//! 2 字节、载荷偏移 8 字节、载荷长度 8 字节、混淆盐 32 字节、文件名长度
//! 4 字节、文件名 240 字节（按盐派生密钥流异或）、保留 208 字节。载荷按
//! 128 KiB 分块、用 `BLAKE3 keyed(salt)` 派生 XOF 密钥流异或，因此文件中
//! 不会出现可识别的归档头或文件名明文，但**这不是加密**：盐随文件保存，
//! 只用于躲避头部特征检查；真正的机密性仍依赖 ZIP/7Z 密码。
//!
//! 读取侧提供两种入口：`open_payload` 返回可随机访问、按需解密的读取视图，
//! 供 ZIP/7Z/TAR/单文件流/LZH 直接浏览；`materialize` 把载荷复制到临时
//! 目录并做进程内缓存，供 ZPAQ、RAR、ALZ 和传统格式等只接受路径的后端
//! 复用现有读取逻辑。`restore` 把载荷还原成原文件，先写同目录临时文件再
//! 原子替换，失败或取消不会留下半成品。
//!
//! 尾部信息是还原的唯一入口：文件被截断、追加或字节被改写后无法还原，
//! 因此文档和界面都提示用户保留原文件。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use blake3::Hasher;

use crate::error::ArchiveError;
use crate::path::{ensure_no_symlink_components, safe_join};
use crate::service::{atomic_replace_file, unique_destination};

mod seekable;

pub use seekable::SeekableDisguiseWriter;

/// 尾部信息魔数，用于区分普通媒体文件和 Lumark 伪装文件。
pub const DISGUISE_MAGIC: [u8; 8] = *b"LMDIS001";

/// 尾部信息总长度，固定为 512 字节。
pub const DISGUISE_TRAILER_LEN: usize = 512;

/// 尾部可保存的原文件名最大 UTF-8 字节数。
pub const MAX_DISGUISE_NAME_BYTES: usize = 240;

/// 自定义面具文件的大小上限，避免把超大视频反复复制进每个伪装文件。
pub const MAX_CUSTOM_MASK_BYTES: u64 = 256 * 1024 * 1024;

/// 自定义面具文件的最小长度；过小的文件无法作为可信的媒体外观。
const MIN_CUSTOM_MASK_BYTES: u64 = 512;

/// 载荷混淆分块大小。
const OBFUSCATION_BLOCK: usize = 128 * 1024;

/// 复制载荷时的分块大小。
const COPY_CHUNK: usize = 1024 * 1024;

/// 物化缓存最多保留的条目数。
const MATERIALIZED_MAX_ENTRIES: usize = 4;

/// 物化缓存允许占用的最大字节数；单个超大载荷仍会保留，避免反复复制。
const MATERIALIZED_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// 启动时清理超过该时长的残留物化目录。
const MATERIALIZED_STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// 内置 MP4 面具：两秒、640×360、H.264 baseline、静音、faststart。
const PRESET_VIDEO: &[u8] = include_bytes!("../assets/disguise/cover.mp4");

/// 内置 JPG 面具：1280×720 渐变图。
const PRESET_IMAGE: &[u8] = include_bytes!("../assets/disguise/cover.jpg");

/// 载荷类型：压缩包（可浏览）或普通文件（只能还原）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisguisePayloadKind {
    /// 载荷是 Lumark 认识的归档，可以通过读取视图或物化副本浏览。
    Archive,
    /// 载荷是普通文件或无法识别的数据，只能还原。
    RawFile,
}

/// 从伪装文件尾部解析出的还原信息。
#[derive(Debug, Clone)]
pub struct DisguiseInfo {
    /// 面具长度，同时也是载荷偏移。
    pub mask_len: u64,
    /// 载荷长度。
    pub payload_len: u64,
    /// 还原时使用的原文件名（只包含文件名，不含目录）。
    pub original_name: String,
    /// 载荷类型。
    pub payload_kind: DisguisePayloadKind,
    /// 载荷是否经过混淆；当前写入版本恒为真，保留字段用于兼容后续版本。
    pub(crate) obfuscated: bool,
    /// 混淆盐；物化缓存用它区分内容已变化的文件。
    pub(crate) salt: [u8; 32],
}

/// 伪装使用的面具来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisguiseMask {
    /// 内置 MP4 视频面具。
    PresetVideo,
    /// 内置 JPG 图片面具。
    PresetImage,
    /// 用户选择的媒体文件；输出扩展名跟随该文件。
    File(PathBuf),
}

impl DisguiseMask {
    /// 返回面具对应的输出扩展名（不含点）。
    ///
    /// 预设固定为 `mp4`/`jpg`；自定义面具取文件名最后一个点后的 ASCII
    /// 字母数字扩展名，缺失或格式异常时返回可操作的错误。
    pub fn extension(&self) -> Result<String, ArchiveError> {
        match self {
            Self::PresetVideo => Ok("mp4".to_string()),
            Self::PresetImage => Ok("jpg".to_string()),
            Self::File(path) => {
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_ascii_lowercase())
                    .filter(|value| {
                        (1..=16).contains(&value.len())
                            && value.chars().all(|item| item.is_ascii_alphanumeric())
                    })
                    .ok_or_else(|| {
                        ArchiveError::InvalidRequest(format!(
                            "面具文件缺少可用扩展名：{}",
                            path.display()
                        ))
                    })?;
                Ok(extension)
            }
        }
    }

    /// 校验面具是否可以安全地用作伪装前缀。
    ///
    /// 自定义面具必须是普通文件、长度在 512 字节到 256 MiB 之间，并且本身
    /// 不能是伪装文件，避免生成嵌套的容器。
    pub fn validate(&self) -> Result<(), ArchiveError> {
        match self {
            Self::PresetVideo | Self::PresetImage => Ok(()),
            Self::File(path) => {
                let metadata = fs::metadata(path).map_err(|error| {
                    ArchiveError::InvalidRequest(format!(
                        "无法读取面具文件 {}：{error}",
                        path.display()
                    ))
                })?;
                if !metadata.is_file() {
                    return Err(ArchiveError::InvalidRequest(format!(
                        "面具路径不是文件：{}",
                        path.display()
                    )));
                }
                if metadata.len() < MIN_CUSTOM_MASK_BYTES {
                    return Err(ArchiveError::InvalidRequest(format!(
                        "面具文件过小，无法作为媒体外观：{}",
                        path.display()
                    )));
                }
                if metadata.len() > MAX_CUSTOM_MASK_BYTES {
                    return Err(ArchiveError::InvalidRequest(format!(
                        "面具文件超过 {} MB 上限：{}",
                        MAX_CUSTOM_MASK_BYTES / (1024 * 1024),
                        path.display()
                    )));
                }
                self.extension()?;
                if probe(path)?.is_some() {
                    return Err(ArchiveError::InvalidRequest(format!(
                        "面具文件本身是 Lumark 伪装文件：{}",
                        path.display()
                    )));
                }
                Ok(())
            }
        }
    }

    /// 返回面具的展示名称，供界面和状态栏使用。
    pub fn display_name(&self) -> String {
        match self {
            Self::PresetVideo => "视频（MP4）".to_string(),
            Self::PresetImage => "图片（JPG）".to_string(),
            Self::File(path) => path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| path.display().to_string()),
        }
    }

    /// 把面具写入目标文件，返回写入长度。
    fn write_to<W: Write>(
        &self,
        target: &mut W,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Result<u64, ArchiveError> {
        self.validate()?;
        match self {
            Self::PresetVideo => {
                target.write_all(PRESET_VIDEO)?;
                Ok(PRESET_VIDEO.len() as u64)
            }
            Self::PresetImage => {
                target.write_all(PRESET_IMAGE)?;
                Ok(PRESET_IMAGE.len() as u64)
            }
            Self::File(path) => {
                let mut input = File::open(path)?;
                let mut buffer = vec![0u8; COPY_CHUNK];
                let mut copied = 0u64;
                loop {
                    ensure_not_cancelled(cancel)?;
                    let read = input.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    target.write_all(&buffer[..read])?;
                    copied += read as u64;
                }
                if copied < MIN_CUSTOM_MASK_BYTES {
                    return Err(ArchiveError::InvalidRequest(format!(
                        "面具文件长度异常：{}",
                        path.display()
                    )));
                }
                Ok(copied)
            }
        }
    }
}

/// 压缩流程使用的伪装参数。
#[derive(Debug, Clone)]
pub struct DisguiseOptions {
    /// 面具来源。
    pub mask: DisguiseMask,
    /// 还原时写回的文件名；`None` 时由调用方按输出名推导。
    pub original_name: Option<String>,
}

/// 批量伪装普通文件的参数。
#[derive(Debug, Clone)]
pub struct DisguiseFileOptions {
    /// 面具来源。
    pub mask: DisguiseMask,
    /// 输出目录；每个文件生成 `<stem>.<面具扩展名>`。
    pub output_dir: PathBuf,
    /// 是否在成功后删除源文件。
    pub delete_source: bool,
    /// 取消标志。
    pub cancel: Option<Arc<AtomicBool>>,
}

/// 批量伪装结果。
#[derive(Debug, Clone, Default)]
pub struct DisguiseReport {
    /// 生成的伪装文件。
    pub outputs: Vec<PathBuf>,
    /// 成功数量。
    pub files: usize,
    /// 已读取的源字节数。
    pub bytes: u64,
    /// 删除源文件失败的提示。
    pub cleanup_errors: Vec<String>,
}

/// 还原结果。
#[derive(Debug, Clone)]
pub struct RestoreReport {
    /// 还原出的文件路径。
    pub output: PathBuf,
    /// 还原出的字节数。
    pub bytes: u64,
}

/// 可随机访问的伪装载荷视图。
///
/// 读取时把逻辑偏移映射到面具之后的真实位置，并对经过的 128 KiB 分块做
/// 异或解密；`Seek` 只移动逻辑位置，因此 ZIP/7Z 等需要回跳的读取器可以
/// 像操作普通文件一样工作。
pub struct PayloadReader {
    file: File,
    mask_len: u64,
    payload_len: u64,
    position: u64,
    obfuscated: bool,
    keystream: KeyStream,
}

impl PayloadReader {
    /// 返回载荷长度。
    pub fn len(&self) -> u64 {
        self.payload_len
    }

    /// 返回载荷是否为空。
    pub fn is_empty(&self) -> bool {
        self.payload_len == 0
    }
}

impl Read for PayloadReader {
    /// 从解混淆载荷视图读取并推进偏移。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.payload_len {
            return Ok(0);
        }
        let remaining = (self.payload_len - self.position) as usize;
        let limit = buffer.len().min(remaining);
        self.file
            .seek(SeekFrom::Start(self.mask_len + self.position))?;
        let read = self.file.read(&mut buffer[..limit])?;
        if self.obfuscated {
            self.keystream.apply(self.position, &mut buffer[..read]);
        }
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for PayloadReader {
    /// 在解密载荷范围内定位读取位置。
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::Current(value) => self.position as i64 + value,
            SeekFrom::End(value) => self.payload_len as i64 + value,
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "伪装载荷定位越界",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}

/// 按逻辑位置生成并缓存一个分块的密钥流。
///
/// ZIP/7Z 读取器经常做小块随机读取，如果每次都重新生成完整分块会明显
/// 放大 CPU；这里只保留最近一个分块。
struct KeyStream {
    salt: [u8; 32],
    block: u64,
    bytes: Vec<u8>,
}

impl KeyStream {
    /// 为指定盐创建空的密钥流缓存。
    fn new(salt: [u8; 32]) -> Self {
        Self {
            salt,
            block: u64::MAX,
            bytes: vec![0u8; OBFUSCATION_BLOCK],
        }
    }

    /// 对从 `position` 开始的字节做原地异或。
    fn apply(&mut self, position: u64, buffer: &mut [u8]) {
        let block_size = OBFUSCATION_BLOCK as u64;
        let mut offset = 0usize;
        while offset < buffer.len() {
            let absolute = position + offset as u64;
            let block = absolute / block_size;
            let within = (absolute % block_size) as usize;
            if self.block != block {
                let mut hasher = Hasher::new_keyed(&self.salt);
                hasher.update(b"lumark-disguise-payload-v1");
                hasher.update(&block.to_le_bytes());
                let mut reader = hasher.finalize_xof();
                reader.fill(&mut self.bytes);
                self.block = block;
            }
            let take = (buffer.len() - offset).min(OBFUSCATION_BLOCK - within);
            for index in 0..take {
                buffer[offset + index] ^= self.bytes[within + index];
            }
            offset += take;
        }
    }
}

/// 判断是否已经收到取消请求。
fn ensure_not_cancelled(cancel: Option<&Arc<AtomicBool>>) -> Result<(), ArchiveError> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(ArchiveError::Cancelled);
    }
    Ok(())
}

/// 返回一次性使用的随机盐。
fn random_salt() -> [u8; 32] {
    rand::random()
}

/// 生成文件名混淆密钥流。
fn name_stream(salt: &[u8; 32]) -> [u8; MAX_DISGUISE_NAME_BYTES] {
    let mut output = [0u8; MAX_DISGUISE_NAME_BYTES];
    let mut hasher = Hasher::new_keyed(salt);
    hasher.update(b"lumark-disguise-name-v1");
    let mut reader = hasher.finalize_xof();
    reader.fill(&mut output);
    output
}

/// 校验原文件名只包含一个安全组件。
fn safe_file_name(name: &str) -> Result<String, ArchiveError> {
    let joined = safe_join(Path::new("."), name)?;
    let file_name = joined
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ArchiveError::UnsafePath(name.to_string()))?;
    if file_name != name {
        return Err(ArchiveError::UnsafePath(name.to_string()));
    }
    if name.is_empty() || name.len() > MAX_DISGUISE_NAME_BYTES {
        return Err(ArchiveError::InvalidRequest(format!(
            "伪装文件名长度必须在 1..={MAX_DISGUISE_NAME_BYTES} 字节之间"
        )));
    }
    Ok(name.to_string())
}

/// 组装 512 字节尾部信息。
fn build_trailer(
    mask_len: u64,
    payload_len: u64,
    original_name: &str,
    kind: DisguisePayloadKind,
    salt: &[u8; 32],
) -> [u8; DISGUISE_TRAILER_LEN] {
    let mut trailer = [0u8; DISGUISE_TRAILER_LEN];
    trailer[0..8].copy_from_slice(&DISGUISE_MAGIC);
    trailer[8] = 1;
    // bit0 载荷已混淆；bit1 载荷是归档。
    trailer[9] = 0b0000_0001
        | match kind {
            DisguisePayloadKind::Archive => 0b0000_0010,
            DisguisePayloadKind::RawFile => 0,
        };
    trailer[12..20].copy_from_slice(&mask_len.to_le_bytes());
    trailer[20..28].copy_from_slice(&payload_len.to_le_bytes());
    trailer[28..60].copy_from_slice(salt);
    trailer[60..64].copy_from_slice(&(original_name.len() as u32).to_le_bytes());
    let mask = name_stream(salt);
    for (index, byte) in original_name.as_bytes().iter().enumerate() {
        trailer[64 + index] = byte ^ mask[index];
    }
    trailer
}

/// 解析尾部信息；魔数不匹配时返回 `None`，魔数匹配但字段损坏时返回错误。
fn parse_trailer(
    trailer: &[u8; DISGUISE_TRAILER_LEN],
    file_len: u64,
) -> Result<Option<DisguiseInfo>, ArchiveError> {
    if trailer[0..8] != DISGUISE_MAGIC[..] {
        return Ok(None);
    }
    let version = trailer[8];
    if version != 1 {
        return Err(ArchiveError::Corrupt(format!(
            "不支持的伪装版本：{version}"
        )));
    }
    let flags = trailer[9];
    if flags & !0b0000_0011 != 0 {
        return Err(ArchiveError::Corrupt("伪装尾部标志位无效".into()));
    }
    if trailer[10] != 0 || trailer[11] != 0 {
        return Err(ArchiveError::Corrupt("伪装尾部保留字段无效".into()));
    }
    let mask_len = u64::from_le_bytes(
        trailer[12..20]
            .try_into()
            .map_err(|_| ArchiveError::Corrupt("伪装载荷偏移损坏".into()))?,
    );
    let payload_len = u64::from_le_bytes(
        trailer[20..28]
            .try_into()
            .map_err(|_| ArchiveError::Corrupt("伪装载荷长度损坏".into()))?,
    );
    let salt: [u8; 32] = trailer[28..60]
        .try_into()
        .map_err(|_| ArchiveError::Corrupt("伪装盐损坏".into()))?;
    let name_len = u32::from_le_bytes(
        trailer[60..64]
            .try_into()
            .map_err(|_| ArchiveError::Corrupt("伪装文件名长度损坏".into()))?,
    ) as usize;
    if name_len == 0 || name_len > MAX_DISGUISE_NAME_BYTES {
        return Err(ArchiveError::Corrupt("伪装文件名长度越界".into()));
    }
    let total = mask_len
        .checked_add(payload_len)
        .and_then(|value| value.checked_add(DISGUISE_TRAILER_LEN as u64))
        .ok_or_else(|| ArchiveError::Corrupt("伪装载荷越界".into()))?;
    if total != file_len {
        return Err(ArchiveError::Corrupt("伪装文件长度与尾部信息不一致".into()));
    }
    let mut name_bytes = trailer[64..64 + MAX_DISGUISE_NAME_BYTES].to_vec();
    let mask = name_stream(&salt);
    for (index, byte) in name_bytes.iter_mut().enumerate() {
        *byte ^= mask[index];
    }
    name_bytes.truncate(name_len);
    let original_name = String::from_utf8(name_bytes)
        .map_err(|_| ArchiveError::Corrupt("伪装文件名不是有效 UTF-8".into()))?;
    let original_name = safe_file_name(&original_name)?;
    let payload_kind = if flags & 0b0000_0010 != 0 {
        DisguisePayloadKind::Archive
    } else {
        DisguisePayloadKind::RawFile
    };
    Ok(Some(DisguiseInfo {
        mask_len,
        payload_len,
        original_name,
        payload_kind,
        obfuscated: flags & 0b0000_0001 != 0,
        salt,
    }))
}

/// 探测路径是否为 Lumark 伪装文件。
///
/// 文件短于尾部信息长度或魔数不匹配时返回 `None`；魔数匹配但字段损坏时
/// 返回带有原因的错误，让界面可以区分“不是伪装文件”和“伪装文件已损坏”。
pub fn probe(path: &Path) -> Result<Option<DisguiseInfo>, ArchiveError> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len < DISGUISE_TRAILER_LEN as u64 {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-(DISGUISE_TRAILER_LEN as i64)))?;
    let mut trailer = [0u8; DISGUISE_TRAILER_LEN];
    file.read_exact(&mut trailer)?;
    parse_trailer(&trailer, len)
}

/// 按已经解析出的尾部信息打开载荷视图。
fn open_payload_with_info(path: &Path, info: &DisguiseInfo) -> Result<PayloadReader, ArchiveError> {
    let file = File::open(path)?;
    Ok(PayloadReader {
        file,
        mask_len: info.mask_len,
        payload_len: info.payload_len,
        position: 0,
        obfuscated: info.obfuscated,
        keystream: KeyStream::new(info.salt),
    })
}

/// 打开伪装文件的载荷读取视图。
///
/// 只读取尾部信息并保持文件句柄，不把载荷复制到内存或临时目录；调用方
/// 可以像普通文件一样 seek 和读取。
pub fn open_payload(path: &Path) -> Result<PayloadReader, ArchiveError> {
    let info = probe(path)?.ok_or_else(|| {
        ArchiveError::Corrupt(format!("不是 Lumark 伪装文件：{}", path.display()))
    })?;
    open_payload_with_info(path, &info)
}

/// 把归档写入流式伪装容器的写入器。
///
/// 首次写入时先复制面具，随后对载荷按 128 KiB 密钥流分块混淆；`finish`
/// 写出尾部并返回底层写入器。这样 TAR/单文件流可以在生成归档的同时完成
/// 包装，避免先落一份完整内层归档。
pub struct DisguiseWriter<W: Write> {
    /// 底层输出。
    inner: W,
    /// 尚未写入的面具；写入后置空。
    mask: Option<DisguiseMask>,
    /// 已写入的面具长度。
    mask_len: u64,
    /// 本次容器的一次性混淆盐。
    salt: [u8; 32],
    /// 载荷密钥流。
    keystream: KeyStream,
    /// 已写入的载荷字节数。
    payload_len: u64,
    /// 尾部记录的原文件名。
    original_name: String,
    /// 载荷类型。
    kind: DisguisePayloadKind,
    /// 取消标志。
    cancel: Option<Arc<AtomicBool>>,
}

impl<W: Write + Seek> DisguiseWriter<W> {
    /// 覆盖载荷开头的前缀字节，用于 7Z 等先写占位头、完成后再回填的格式。
    ///
    /// 前缀仍按载荷位置 0 混淆，写到面具之后的真实文件偏移；写入后恢复到底层
    /// 当前末尾，随后 `finish` 可以正常追加尾部。
    pub fn patch_payload_prefix(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.ensure_mask()?;
        let saved = self.inner.stream_position()?;
        let mut obfuscated = bytes.to_vec();
        self.keystream.apply(0, &mut obfuscated);
        self.inner.seek(SeekFrom::Start(self.mask_len))?;
        self.inner.write_all(&obfuscated)?;
        self.inner.seek(SeekFrom::Start(saved))?;
        Ok(())
    }
}

impl<W: Write> DisguiseWriter<W> {
    /// 创建流式伪装写入器，并提前校验面具和原文件名。
    pub fn new(
        inner: W,
        mask: DisguiseMask,
        original_name: &str,
        kind: DisguisePayloadKind,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self, ArchiveError> {
        mask.validate()?;
        let original_name = safe_file_name(original_name)?;
        let salt = random_salt();
        Ok(Self {
            inner,
            mask: Some(mask),
            mask_len: 0,
            salt,
            keystream: KeyStream::new(salt),
            payload_len: 0,
            original_name,
            kind,
            cancel,
        })
    }

    /// 首次写入前复制面具，保证载荷偏移正确。
    fn ensure_mask(&mut self) -> io::Result<()> {
        if let Some(mask) = self.mask.take() {
            self.mask_len = mask
                .write_to(&mut self.inner, self.cancel.as_ref())
                .map_err(|error| io::Error::other(error.to_string()))?;
        }
        Ok(())
    }

    /// 写出尾部信息并返回底层写入器。
    pub fn finish(mut self) -> Result<W, ArchiveError> {
        if let Some(mask) = self.mask.take() {
            self.mask_len = mask.write_to(&mut self.inner, self.cancel.as_ref())?;
        }
        let trailer = build_trailer(
            self.mask_len,
            self.payload_len,
            &self.original_name,
            self.kind,
            &self.salt,
        );
        self.inner.write_all(&trailer)?;
        self.inner.flush()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for DisguiseWriter<W> {
    /// 混淆载荷后写入底层；首次调用会先写面具。
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(io::Error::other("cancelled"));
        }
        self.ensure_mask()?;
        let mut obfuscated = buffer.to_vec();
        self.keystream.apply(self.payload_len, &mut obfuscated);
        self.inner.write_all(&obfuscated)?;
        self.payload_len = self.payload_len.saturating_add(buffer.len() as u64);
        Ok(buffer.len())
    }

    /// 刷新底层写入器。
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// 把 `source` 包装成 `output`：面具 + 混淆载荷 + 尾部信息。
///
/// 使用独立临时文件由调用方负责，`wrap` 在失败时删除 `output`。进度回调
/// 参数为（已复制载荷字节，载荷总字节）。
pub fn wrap<F>(
    source: &Path,
    output: &Path,
    mask: &DisguiseMask,
    original_name: &str,
    kind: DisguisePayloadKind,
    cancel: Option<&Arc<AtomicBool>>,
    mut progress: F,
) -> Result<u64, ArchiveError>
where
    F: FnMut(u64, u64),
{
    let original_name = safe_file_name(original_name)?;
    let mut input = File::open(source)?;
    let result = (|| -> Result<u64, ArchiveError> {
        let total = input.metadata()?.len();
        let mut target = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(output)?;
        let mask_len = mask.write_to(&mut target, cancel)?;
        let salt = random_salt();
        let mut keystream = KeyStream::new(salt);
        let mut buffer = vec![0u8; COPY_CHUNK];
        let mut copied = 0u64;
        loop {
            ensure_not_cancelled(cancel)?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            keystream.apply(copied, &mut buffer[..read]);
            target.write_all(&buffer[..read])?;
            copied += read as u64;
            progress(copied, total);
        }
        let trailer = build_trailer(mask_len, copied, &original_name, kind, &salt);
        target.write_all(&trailer)?;
        target.flush()?;
        target.sync_all()?;
        Ok(copied)
    })();
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

/// 把伪装载荷还原成原文件。
///
/// 先写入输出目录内的临时文件，成功后再原子替换目标；目标同名时自动
/// 追加 ` (1)` 等序号，不覆盖已有文件。进度回调参数为（已还原字节，
/// 载荷总字节）。
pub fn restore<F>(
    archive: &Path,
    output_dir: &Path,
    cancel: Option<&Arc<AtomicBool>>,
    mut progress: F,
) -> Result<RestoreReport, ArchiveError>
where
    F: FnMut(u64, u64),
{
    let info = probe(archive)?.ok_or_else(|| {
        ArchiveError::Corrupt(format!("不是 Lumark 伪装文件：{}", archive.display()))
    })?;
    fs::create_dir_all(output_dir)?;
    let mut destination = safe_join(output_dir, &info.original_name)?;
    if destination.exists() {
        destination = unique_destination(&destination);
    }
    ensure_no_symlink_components(output_dir, &destination)?;
    let temp = output_dir.join(format!(
        ".lumark-restore-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    let result = (|| -> Result<u64, ArchiveError> {
        let mut reader = open_payload_with_info(archive, &info)?;
        let mut output = File::create(&temp)?;
        let mut buffer = vec![0u8; COPY_CHUNK];
        let mut copied = 0u64;
        loop {
            ensure_not_cancelled(cancel)?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            copied += read as u64;
            progress(copied, info.payload_len);
        }
        output.flush()?;
        output.sync_all()?;
        drop(output);
        Ok(copied)
    })();
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
    };
    if let Err(error) = atomic_replace_file(&temp, &destination) {
        let _ = fs::remove_file(&temp);
        return Err(ArchiveError::Io(error));
    }
    Ok(RestoreReport {
        output: destination,
        bytes,
    })
}

/// 已物化的载荷缓存条目。
struct CachedMaterialized {
    key: (PathBuf, u64, SystemTime, [u8; 32]),
    file: PathBuf,
    /// 持有临时目录，条目淘汰或进程退出时删除副本。
    _directory: Arc<tempfile::TempDir>,
    last_used: Instant,
    bytes: u64,
}

/// 进程内物化缓存。
static MATERIALIZED: OnceLock<Mutex<Vec<CachedMaterialized>>> = OnceLock::new();

/// 清理一次残留的物化目录；只在首次物化时执行。
fn cleanup_stale_materialized() {
    /// 已清理的物化缓存路径，避免同一进程重复删除。
    static CLEANED: OnceLock<()> = OnceLock::new();
    CLEANED.get_or_init(|| {
        let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let cutoff = SystemTime::now()
            .checked_sub(MATERIALIZED_STALE_AFTER)
            .unwrap_or(UNIX_EPOCH);
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("lumark-disguise-") {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map(|modified| modified < cutoff)
                .unwrap_or(false);
            if stale {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    });
}

/// 把伪装载荷物化到临时目录并返回路径。
///
/// 路径型后端（ZPAQ、RAR、ALZ、传统格式）只接受文件路径，因此需要真实
/// 副本。缓存按（规范路径、文件长度、修改时间、盐）区分内容，最多保留
/// 4 项 / 16 GiB，避免同一次浏览会话反复复制大文件。
pub fn materialize(path: &Path) -> Result<PathBuf, ArchiveError> {
    let info = probe(path)?.ok_or_else(|| {
        ArchiveError::Corrupt(format!("不是 Lumark 伪装文件：{}", path.display()))
    })?;
    let metadata = fs::metadata(path)?;
    let key = (
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        metadata.len(),
        metadata.modified().unwrap_or(UNIX_EPOCH),
        info.salt,
    );
    cleanup_stale_materialized();
    let cache = MATERIALIZED.get_or_init(|| Mutex::new(Vec::new()));
    {
        let mut entries = cache.lock().expect("materialized cache");
        if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
            entry.last_used = Instant::now();
            return Ok(entry.file.clone());
        }
    }
    let name = safe_file_name(&info.original_name)?;
    let directory = tempfile::Builder::new()
        .prefix("lumark-disguise-")
        .tempdir()?;
    let file = directory.path().join(name);
    {
        let mut reader = open_payload_with_info(path, &info)?;
        let mut output = File::create(&file)?;
        io::copy(&mut reader, &mut output)?;
        output.flush()?;
        output.sync_all()?;
    }
    let entry = CachedMaterialized {
        key,
        file: file.clone(),
        _directory: Arc::new(directory),
        last_used: Instant::now(),
        bytes: info.payload_len,
    };
    let mut entries = cache.lock().expect("materialized cache");
    entries.push(entry);
    // 超出上限时从最久未使用的条目开始淘汰；刚插入的条目 last_used 最新，
    // 排序后位于末尾，不会被优先删除；只剩一项时保留，避免超大载荷因为
    // 超过总字节上限而反复复制。
    entries.sort_by_key(|entry| entry.last_used);
    loop {
        let total: u64 = entries.iter().map(|entry| entry.bytes).sum();
        if entries.len() <= MATERIALIZED_MAX_ENTRIES && total <= MATERIALIZED_MAX_BYTES {
            break;
        }
        if entries.len() <= 1 {
            break;
        }
        entries.remove(0);
    }
    drop(entries);
    Ok(file)
}

/// 清空物化缓存，供测试或用户主动释放临时空间。
pub fn clear_materialized_cache() {
    if let Some(cache) = MATERIALIZED.get() {
        cache.lock().expect("materialized cache").clear();
    }
}

#[cfg(test)]
mod tests;
