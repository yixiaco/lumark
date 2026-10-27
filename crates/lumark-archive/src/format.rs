use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Brotli,
    Lz4,
    Rar,
    Iso,
    Udf,
    Cab,
    Arj,
    Ace,
    Arc,
    Alz,
    UnixCompress,
    Lzh,
    Cpio,
    Wim,
    Isz,
    Deb,
    Asar,
    Zpaq,
    Uu,
    Img,
    Bin,
    Enc,
    Lzip,
    Unknown,
}

impl ArchiveFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP 压缩文件",
            Self::SevenZip => "7Z 压缩文件",
            Self::Tar => "TAR 归档",
            Self::TarGz => "TAR.GZ 归档",
            Self::TarBz2 => "TAR.BZ2 归档",
            Self::TarXz => "TAR.XZ 归档",
            Self::TarZst => "TAR.ZST 归档",
            Self::Gzip => "GZIP 文件",
            Self::Bzip2 => "BZIP2 文件",
            Self::Xz => "XZ 文件",
            Self::Zstd => "ZSTD 文件",
            Self::Brotli => "BROTLI 文件",
            Self::Lz4 => "LZ4 文件",
            Self::Rar => "RAR 压缩文件",
            Self::Iso => "ISO 镜像",
            Self::Udf => "UDF 镜像",
            Self::Cab => "CAB 压缩文件",
            Self::Arj => "ARJ 压缩文件",
            Self::Ace => "ACE 压缩文件",
            Self::Arc => "ARC 压缩文件",
            Self::Alz => "ALZ 压缩文件",
            Self::UnixCompress => "Z 压缩文件",
            Self::Lzh => "LZH 压缩文件",
            Self::Cpio => "CPIO 归档",
            Self::Wim => "WIM 镜像",
            Self::Isz => "ISZ 镜像",
            Self::Deb => "DEB 包",
            Self::Asar => "ASAR 归档",
            Self::Zpaq => "ZPAQ 压缩文件",
            Self::Uu => "UU 编码文件",
            Self::Img => "IMG 镜像",
            Self::Bin => "BIN 镜像",
            Self::Enc => "AES 加密容器",
            Self::Lzip => "LZIP 文件",
            Self::Unknown => "文件",
        }
    }

    pub fn is_writable(self) -> bool {
        matches!(
            self,
            Self::Zip
                | Self::SevenZip
                | Self::Tar
                | Self::TarGz
                | Self::TarBz2
                | Self::TarXz
                | Self::TarZst
                | Self::Gzip
                | Self::Bzip2
                | Self::Xz
                | Self::Zstd
                | Self::Brotli
                | Self::Lz4
                | Self::Zpaq
        )
    }

    pub fn supports_password(self) -> bool {
        matches!(
            self,
            Self::Zip | Self::SevenZip | Self::Rar | Self::Zpaq | Self::Enc
        )
    }

    pub fn supports_comment(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZip => "7z",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarBz2 => "tar.bz2",
            Self::TarXz => "tar.xz",
            Self::TarZst => "tar.zst",
            Self::Gzip => "gz",
            Self::Bzip2 => "bz2",
            Self::Xz => "xz",
            Self::Zstd => "zst",
            Self::Brotli => "br",
            Self::Lz4 => "lz4",
            Self::Rar => "rar",
            Self::Iso => "iso",
            Self::Udf => "udf",
            Self::Cab => "cab",
            Self::Arj => "arj",
            Self::Ace => "ace",
            Self::Arc => "arc",
            Self::Alz => "alz",
            Self::UnixCompress => "Z",
            Self::Lzh => "lzh",
            Self::Cpio => "cpio",
            Self::Wim => "wim",
            Self::Isz => "isz",
            Self::Deb => "deb",
            Self::Asar => "asar",
            Self::Zpaq => "zpaq",
            Self::Uu => "uu",
            Self::Img => "img",
            Self::Bin => "bin",
            Self::Enc => "enc",
            Self::Lzip => "lz",
            Self::Unknown => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub id: u64,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub compressed_size: Option<u64>,
    pub modified: Option<DateTime<Utc>>,
    pub crc32: Option<u32>,
    pub method: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ArchiveInfo {
    pub format: Option<ArchiveFormat>,
    pub id: u64,
    pub entries: Vec<ArchiveEntry>,
    pub encrypted: bool,
    pub header_encrypted: bool,
    pub solid: bool,
    pub comment: Option<String>,
    pub total_size: u64,
    pub compressed_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionPreset {
    Speed,
    Balanced,
    Size,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PathMode {
    #[default]
    Relative,
    NoRoot,
    Flatten,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ZipEncryption {
    #[default]
    Aes256,
    ZipCrypto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverwritePolicy {
    #[default]
    Ask,
    Overwrite,
    Skip,
    Rename,
}

/// 向归档添加同名文件时的处理方式。
///
/// 前三种模式只影响同名条目的替换判断，删除和重命名始终按参数执行；
/// 归档条目缺少时间戳时按“需要更新”处理，避免静默丢文件。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UpdateMode {
    /// 同名条目一律用新文件替换。
    #[default]
    Replace,
    /// 只有比归档内条目更新的文件才替换，新文件照常添加。
    Newer,
    /// 只更新归档中已存在的条目，不添加新条目。
    Freshen,
    /// 用目录内容同步归档：新增缺少的文件、更新较新文件，并删除磁盘上
    /// 已经不存在的条目。
    Sync,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ZipCompression {
    #[default]
    Deflate,
    Bzip2,
    Lzma,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SevenZCompression {
    #[default]
    Lzma2,
    Lzma,
    Ppmd,
    Bzip2,
}

/// ZPAQ 压缩级别，对应 zpaq 引擎的数字方法 `1..=5`。
///
/// 4/5 级压缩率更高但耗时明显增加，界面默认只到 3 级。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ZpaqCompression {
    /// 1 级：最快，压缩率最低。
    Fast,
    /// 2 级：默认平衡档。
    #[default]
    Balanced,
    /// 3 级：更高压缩率。
    Better,
    /// 4 级：接近极限，耗时明显增加。
    Maximum,
    /// 5 级：极限压缩，耗时最长。
    Ultra,
}

impl ZpaqCompression {
    /// 返回传给 zpaq 引擎的数字方法字符串。
    pub fn method(self) -> &'static str {
        match self {
            Self::Fast => "1",
            Self::Balanced => "2",
            Self::Better => "3",
            Self::Maximum => "4",
            Self::Ultra => "5",
        }
    }

    /// 按界面预设推导默认级别；体积优先只到 3 级，避免默认任务过慢。
    pub fn from_preset(preset: CompressionPreset) -> Self {
        match preset {
            CompressionPreset::Speed => Self::Fast,
            CompressionPreset::Balanced => Self::Balanced,
            CompressionPreset::Size => Self::Better,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArchiveTimeSource {
    #[default]
    Current,
    Original,
    Latest,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileTimeFilter {
    #[default]
    Any,
    Before,
    After,
    Range,
}

#[derive(Debug, Clone)]
pub struct CompressionOptions {
    pub format: ArchiveFormat,
    pub level: Option<i64>,
    pub preset: CompressionPreset,
    pub solid: bool,
    /// 7z：设置密码时同时加密归档文件头和条目数据流。
    /// ZIP 忽略此标志。
    pub encrypt_header: bool,
    pub dictionary_size: Option<u32>,
    pub threads: u32,
    pub zip_compression: ZipCompression,
    pub sevenz_compression: SevenZCompression,
    /// ZPAQ 数字压缩级别；其他格式忽略。
    pub zpaq_compression: ZpaqCompression,
    pub word_size: Option<u32>,
    pub solid_block_size: Option<u64>,
    pub archive_time: ArchiveTimeSource,
    pub file_time_filter: FileTimeFilter,
    pub modified_after: Option<DateTime<Utc>>,
    pub modified_before: Option<DateTime<Utc>>,
    pub path_mode: PathMode,
    pub password: Option<Zeroizing<String>>,
    pub zip_encryption: ZipEncryption,
    pub comment: Option<String>,
    pub split_size: Option<u64>,
    pub test_after: bool,
    pub delete_source: bool,
    /// 生成自解压 EXE；只由 `ArchiveService::create_sfx` 处理，`create` 忽略。
    pub sfx: bool,
    pub exclude_patterns: Vec<String>,
    pub store_only_patterns: Vec<String>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            format: ArchiveFormat::Zip,
            level: Some(6),
            preset: CompressionPreset::Balanced,
            solid: false,
            encrypt_header: true,
            dictionary_size: None,
            threads: 1,
            zip_compression: ZipCompression::Deflate,
            sevenz_compression: SevenZCompression::Lzma2,
            zpaq_compression: ZpaqCompression::Balanced,
            word_size: None,
            solid_block_size: None,
            archive_time: ArchiveTimeSource::Original,
            file_time_filter: FileTimeFilter::Any,
            modified_after: None,
            modified_before: None,
            path_mode: PathMode::Relative,
            password: None,
            zip_encryption: ZipEncryption::Aes256,
            comment: None,
            split_size: None,
            test_after: false,
            delete_source: false,
            sfx: false,
            exclude_patterns: Vec::new(),
            store_only_patterns: Vec::new(),
            cancel: None,
        }
    }
}

/// 单次解压的资源预算，用于识别压缩炸弹。
///
/// 默认值只拦截极端情况（总量 128 GiB、条目 100 万），正常备份不会被误伤；
/// 调用方可以按磁盘剩余空间或业务场景收紧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionBudget {
    /// 单次解压允许写出的最大总字节数。
    pub max_bytes: u64,
    /// 单次解压允许处理的最大条目数。
    pub max_entries: usize,
}

impl Default for ExtractionBudget {
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024 * 1024 * 1024,
            max_entries: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExtractionOptions {
    pub output_dir: std::path::PathBuf,
    pub selected_paths: Vec<String>,
    pub overwrite: bool,
    pub overwrite_policy: OverwritePolicy,
    pub password: Option<Zeroizing<String>>,
    pub preserve_paths: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    /// 压缩炸弹预算；默认值足够宽松，可按需收紧。
    pub budget: ExtractionBudget,
}

#[derive(Debug, Clone, Default)]
pub struct ExtractReport {
    pub files: usize,
    pub bytes: u64,
    pub skipped: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CreateReport {
    pub files: usize,
    pub bytes: u64,
    pub output: std::path::PathBuf,
    pub cleanup_errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EncryptionInfo {
    pub encrypted: bool,
    pub header_encrypted: bool,
    pub format: Option<ArchiveFormat>,
}

pub fn detect_format(path: &Path) -> ArchiveFormat {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return ArchiveFormat::TarGz;
    }
    if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") || name.ends_with(".tbz") {
        return ArchiveFormat::TarBz2;
    }
    if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        return ArchiveFormat::TarXz;
    }
    if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        return ArchiveFormat::TarZst;
    }
    if name.ends_with(".zip")
        || name.ends_with(".zipx")
        || name.ends_with(".jar")
        || name.ends_with(".war")
        || name.ends_with(".apk")
        || name.ends_with(".ipa")
        || name.ends_with(".xpi")
    {
        return ArchiveFormat::Zip;
    }
    if name.ends_with(".7z") {
        return ArchiveFormat::SevenZip;
    }
    if name.ends_with(".tar") {
        return ArchiveFormat::Tar;
    }
    if name.ends_with(".gz") || name.ends_with(".gzip") {
        return ArchiveFormat::Gzip;
    }
    if name.ends_with(".bz2") || name.ends_with(".bzip2") {
        return ArchiveFormat::Bzip2;
    }
    if name.ends_with(".xz") {
        return ArchiveFormat::Xz;
    }
    if name.ends_with(".zst") || name.ends_with(".zstd") {
        return ArchiveFormat::Zstd;
    }
    if name.ends_with(".br") || name.ends_with(".brotli") {
        return ArchiveFormat::Brotli;
    }
    if name.ends_with(".lz4") {
        return ArchiveFormat::Lz4;
    }
    if name.ends_with(".rar") {
        return ArchiveFormat::Rar;
    }
    if name.ends_with(".iso") {
        return ArchiveFormat::Iso;
    }
    if name.ends_with(".udf") {
        return ArchiveFormat::Udf;
    }
    if name.ends_with(".cab") {
        return ArchiveFormat::Cab;
    }
    if name.ends_with(".arj") {
        return ArchiveFormat::Arj;
    }
    if name.ends_with(".ace") {
        return ArchiveFormat::Ace;
    }
    if name.ends_with(".arc") || name.ends_with(".pak") {
        return ArchiveFormat::Arc;
    }
    if name.ends_with(".alz") {
        return ArchiveFormat::Alz;
    }
    if name.ends_with(".z") {
        return ArchiveFormat::UnixCompress;
    }
    if name.ends_with(".lzh") || name.ends_with(".lha") {
        return ArchiveFormat::Lzh;
    }
    if name.ends_with(".cpio") {
        return ArchiveFormat::Cpio;
    }
    if name.ends_with(".wim") || name.ends_with(".swm") {
        return ArchiveFormat::Wim;
    }
    if name.ends_with(".isz") {
        return ArchiveFormat::Isz;
    }
    if name.ends_with(".deb") {
        return ArchiveFormat::Deb;
    }
    if name.ends_with(".asar") {
        return ArchiveFormat::Asar;
    }
    if name.ends_with(".zpaq") || name.ends_with(".zpq") {
        return ArchiveFormat::Zpaq;
    }
    if name.ends_with(".uu") || name.ends_with(".uue") || name.ends_with(".xxe") {
        return ArchiveFormat::Uu;
    }
    if name.ends_with(".img") || name.ends_with(".ima") {
        return ArchiveFormat::Img;
    }
    if name.ends_with(".bin") {
        return ArchiveFormat::Bin;
    }
    if name.ends_with(".enc") {
        return ArchiveFormat::Enc;
    }
    if name.ends_with(".lz") || name.ends_with(".lzma") {
        return ArchiveFormat::Lzip;
    }
    ArchiveFormat::Unknown
}

pub fn detect_magic(bytes: &[u8]) -> ArchiveFormat {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return ArchiveFormat::Zip;
    }
    if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return ArchiveFormat::SevenZip;
    }
    // 加密 ZPAQ 以随机 salt 开头，只能靠扩展名识别；明文流式归档统一
    // 以 JIDAC 标签 `7kSt` 或旧版 `zPQ` 版本头开始。
    if bytes.starts_with(b"7kSt")
        || (bytes.starts_with(b"zPQ") && bytes.get(3).is_some_and(|version| *version >= 1))
    {
        return ArchiveFormat::Zpaq;
    }
    if bytes.starts_with(b"Rar!\x1a\x07") {
        return ArchiveFormat::Rar;
    }
    if bytes.starts_with(b"\x1f\x8b") {
        return ArchiveFormat::Gzip;
    }
    if bytes.starts_with(b"\x1f\x9d") {
        return ArchiveFormat::UnixCompress;
    }
    if bytes.starts_with(b"BZh") {
        return ArchiveFormat::Bzip2;
    }
    if bytes.starts_with(b"\xfd7zXZ\x00") {
        return ArchiveFormat::Xz;
    }
    if bytes.starts_with(b"\x28\xb5\x2f\xfd") {
        return ArchiveFormat::Zstd;
    }
    if bytes.starts_with(b"MSCF") {
        return ArchiveFormat::Cab;
    }
    if bytes.starts_with(b"!<arch>\n") {
        return ArchiveFormat::Deb;
    }
    if bytes.starts_with(b"070701") || bytes.starts_with(b"070702") {
        return ArchiveFormat::Cpio;
    }
    ArchiveFormat::Unknown
}
