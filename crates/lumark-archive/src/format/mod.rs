//! 归档格式、选项、条目和报告的数据模型。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::disguise::DisguiseOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Lumark 支持的归档格式；Unknown 表示无法识别。
pub enum ArchiveFormat {
    /// ZIP 归档。
    Zip,
    /// 7Z 归档。
    SevenZip,
    /// 未压缩 TAR。
    Tar,
    /// gzip 压缩 TAR。
    TarGz,
    /// bzip2 压缩 TAR。
    TarBz2,
    /// xz 压缩 TAR。
    TarXz,
    /// zstd 压缩 TAR。
    TarZst,
    /// Brotli 压缩 TAR。
    TarBr,
    /// LZ4 压缩 TAR。
    TarLz4,
    /// Lzip 压缩 TAR。
    TarLz,
    /// LZMA 压缩 TAR。
    TarLzma,
    /// Unix compress 压缩 TAR。
    TarZ,
    /// gzip 单文件流。
    Gzip,
    /// bzip2 单文件流。
    Bzip2,
    /// xz 单文件流。
    Xz,
    /// zstd 单文件流。
    Zstd,
    /// Brotli 单文件流。
    Brotli,
    /// LZ4 单文件流。
    Lz4,
    /// RAR 只读归档。
    Rar,
    /// ISO 光盘映像。
    Iso,
    /// UDF 光盘映像。
    Udf,
    /// Windows CAB。
    Cab,
    /// ARJ 归档。
    Arj,
    /// ACE 归档。
    Ace,
    /// ARC/PAK 归档。
    Arc,
    /// ALZ 归档。
    Alz,
    /// ZOO 归档。
    Zoo,
    /// HA 归档。
    Ha,
    /// HYP 归档。
    Hyp,
    /// SQ 归档。
    Sq,
    /// SQZ 归档。
    Sqz,
    /// UC2 归档。
    Uc2,
    /// ICE 归档。
    Ice,
    /// PACKICE 归档。
    PackIce,
    /// Unix .Z 单文件流。
    UnixCompress,
    /// LZH 归档。
    Lzh,
    /// CPIO 归档。
    Cpio,
    /// WIM 映像。
    Wim,
    /// ISZ 映像。
    Isz,
    /// XAR/PKG 归档。
    Xar,
    /// RPM 包。
    Rpm,
    /// Windows Installer 安装包。
    Msi,
    /// Debian 包。
    Deb,
    /// Electron ASAR。
    Asar,
    /// ZPAQ 归档。
    Zpaq,
    /// UU 编码文件。
    Uu,
    /// IMG 软盘/磁盘映像。
    Img,
    /// BIN 磁盘映像。
    Bin,
    /// AES 加密容器。
    Enc,
    /// Lzip 单文件流。
    Lzip,
    /// Lumark 伪装容器：内层格式由尾部信息保存的原文件名决定。
    ///
    /// 该变体只由 `ArchiveService::detect` 在识别到尾部信息时返回，不会
    /// 出现在扩展名或魔数识别结果中，也不属于可写格式。
    Disguised,
    /// 无法识别的格式。
    Unknown,
}

impl ArchiveFormat {
    /// 全部已识别格式，不包含 `Unknown` 和仅由尾部探测得到的 `Disguised`。
    ///
    /// 该列表用于检测矩阵、可写格式覆盖和界面选项一致性测试；新增格式时
    /// 必须同步登记，避免再次出现“核心支持但界面漏项”的偏差。
    pub const ALL: &'static [ArchiveFormat] = &[
        Self::Zip,
        Self::SevenZip,
        Self::Tar,
        Self::TarGz,
        Self::TarBz2,
        Self::TarXz,
        Self::TarZst,
        Self::TarBr,
        Self::TarLz4,
        Self::TarLz,
        Self::TarLzma,
        Self::TarZ,
        Self::Gzip,
        Self::Bzip2,
        Self::Xz,
        Self::Zstd,
        Self::Brotli,
        Self::Lz4,
        Self::Rar,
        Self::Iso,
        Self::Udf,
        Self::Cab,
        Self::Arj,
        Self::Ace,
        Self::Arc,
        Self::Alz,
        Self::Zoo,
        Self::Ha,
        Self::Hyp,
        Self::Sq,
        Self::Sqz,
        Self::Uc2,
        Self::Ice,
        Self::PackIce,
        Self::UnixCompress,
        Self::Lzh,
        Self::Cpio,
        Self::Wim,
        Self::Isz,
        Self::Deb,
        Self::Asar,
        Self::Zpaq,
        Self::Uu,
        Self::Img,
        Self::Bin,
        Self::Enc,
        Self::Lzip,
    ];

    /// 返回用于界面展示的格式名称。
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP 压缩文件",
            Self::SevenZip => "7Z 压缩文件",
            Self::Tar => "TAR 归档",
            Self::TarGz => "TAR.GZ 归档",
            Self::TarBz2 => "TAR.BZ2 归档",
            Self::TarXz => "TAR.XZ 归档",
            Self::TarZst => "TAR.ZST 归档",
            Self::TarBr => "TAR.BR 归档",
            Self::TarLz4 => "TAR.LZ4 归档",
            Self::TarLz => "TAR.LZ 归档",
            Self::TarLzma => "TAR.LZMA 归档",
            Self::TarZ => "TAR.Z 归档",
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
            Self::Zoo => "ZOO 压缩文件",
            Self::Ha => "HA 压缩文件",
            Self::Hyp => "HYP 压缩文件",
            Self::Sq => "SQ 压缩文件",
            Self::Sqz => "SQZ 压缩文件",
            Self::Uc2 => "UC2 压缩文件",
            Self::Ice => "ICE 压缩文件",
            Self::PackIce => "PACKICE 压缩文件",
            Self::UnixCompress => "Z 压缩文件",
            Self::Lzh => "LZH 压缩文件",
            Self::Cpio => "CPIO 归档",
            Self::Wim => "WIM 镜像",
            Self::Isz => "ISZ 镜像",
            Self::Xar => "XAR/PKG 归档",
            Self::Rpm => "RPM 包",
            Self::Msi => "MSI 安装包",
            Self::Deb => "DEB 包",
            Self::Asar => "ASAR 归档",
            Self::Zpaq => "ZPAQ 压缩文件",
            Self::Uu => "UU 编码文件",
            Self::Img => "IMG 镜像",
            Self::Bin => "BIN 镜像",
            Self::Enc => "AES 加密容器",
            Self::Lzip => "LZIP 文件",
            Self::Disguised => "Lumark 伪装压缩文件",
            Self::Unknown => "文件",
        }
    }

    /// 判断该格式是否支持创建。
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
                | Self::TarBr
                | Self::TarLz4
                | Self::TarLz
                | Self::TarLzma
                | Self::Gzip
                | Self::Bzip2
                | Self::Xz
                | Self::Zstd
                | Self::Brotli
                | Self::Lz4
                | Self::Lzh
                | Self::Zpaq
        )
    }

    /// 是否为可写的单文件流格式。
    ///
    /// 这些格式只编码一段字节流，没有归档目录，因此一次只能压缩一个普通
    /// 文件；多文件或文件夹需要改用对应的 TAR 变体。
    pub fn is_single_stream(self) -> bool {
        matches!(
            self,
            Self::Gzip | Self::Bzip2 | Self::Xz | Self::Zstd | Self::Brotli | Self::Lz4
        )
    }

    /// 返回单文件流格式对应的 TAR 变体。
    ///
    /// 用于多文件压缩时把 GZ/BZ2/XZ/ZST/Brotli/LZ4 自动切换为
    /// TAR.GZ/TAR.BZ2/TAR.XZ/TAR.ZST/TAR.BR/TAR.LZ4，而不是给出
    /// “归档损坏”这类误导性错误。
    pub fn single_stream_tar_variant(self) -> Option<Self> {
        match self {
            Self::Gzip => Some(Self::TarGz),
            Self::Bzip2 => Some(Self::TarBz2),
            Self::Xz => Some(Self::TarXz),
            Self::Zstd => Some(Self::TarZst),
            Self::Brotli => Some(Self::TarBr),
            Self::Lz4 => Some(Self::TarLz4),
            _ => None,
        }
    }

    /// 判断该格式是否支持密码。
    pub fn supports_password(self) -> bool {
        matches!(
            self,
            Self::Zip | Self::SevenZip | Self::Rar | Self::Zpaq | Self::Alz | Self::Enc
        )
    }

    /// 判断该格式是否支持写入注释。
    pub fn supports_comment(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip)
    }

    /// 返回该格式的默认扩展名。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZip => "7z",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarBz2 => "tar.bz2",
            Self::TarXz => "tar.xz",
            Self::TarZst => "tar.zst",
            Self::TarBr => "tar.br",
            Self::TarLz4 => "tar.lz4",
            Self::TarLz => "tar.lz",
            Self::TarLzma => "tar.lzma",
            Self::TarZ => "tar.z",
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
            Self::Zoo => "zoo",
            Self::Ha => "ha",
            Self::Hyp => "hyp",
            Self::Sq => "sq",
            Self::Sqz => "sqz",
            Self::Uc2 => "uc2",
            Self::Ice => "ice",
            Self::PackIce => "pi9",
            Self::UnixCompress => "Z",
            Self::Lzh => "lzh",
            Self::Cpio => "cpio",
            Self::Wim => "wim",
            Self::Isz => "isz",
            Self::Xar => "xar",
            Self::Rpm => "rpm",
            Self::Msi => "msi",
            Self::Deb => "deb",
            Self::Asar => "asar",
            Self::Zpaq => "zpaq",
            Self::Uu => "uu",
            Self::Img => "img",
            Self::Bin => "bin",
            Self::Enc => "enc",
            Self::Lzip => "lz",
            // 伪装容器的扩展名由面具决定，检测逻辑不依赖它。
            Self::Disguised => "",
            Self::Unknown => "",
        }
    }
}

#[derive(Debug, Clone)]
/// 归档中单个条目或目录的元数据。
pub struct ArchiveEntry {
    /// 条目在归档内的稳定索引。
    pub id: u64,
    /// 归档内相对路径。
    pub path: String,
    /// 是否为目录条目。
    pub is_dir: bool,
    /// 原始大小。
    pub size: u64,
    /// 压缩后大小；未知时为空。
    pub compressed_size: Option<u64>,
    /// 修改时间。
    pub modified: Option<DateTime<Utc>>,
    /// CRC32 校验值。
    pub crc32: Option<u32>,
    /// 压缩方法名称。
    pub method: String,
    /// 条目数据是否加密。
    pub encrypted: bool,
}

#[derive(Debug, Clone, Default)]
/// 一次列表结果的整体信息。
pub struct ArchiveInfo {
    /// 识别出的归档格式。
    pub format: Option<ArchiveFormat>,
    /// 归档稳定标识。
    pub id: u64,
    /// 条目列表。
    pub entries: Vec<ArchiveEntry>,
    /// 是否存在加密条目。
    pub encrypted: bool,
    /// 归档头或文件名是否加密。
    pub header_encrypted: bool,
    /// 是否为固实归档。
    pub solid: bool,
    /// 归档注释。
    pub comment: Option<String>,
    /// 全部条目原始大小之和。
    pub total_size: u64,
    /// 归档文件或压缩数据大小。
    pub compressed_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 压缩速度与体积的预设。
pub enum CompressionPreset {
    /// 优先速度。
    Speed,
    /// 速度与体积平衡。
    Balanced,
    /// 优先体积。
    Size,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 归档内路径的根目录处理方式。
pub enum PathMode {
    #[default]
    Relative,
    /// 去掉共同根目录。
    NoRoot,
    /// 所有文件平铺到根。
    Flatten,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// ZIP 密码加密算法。
pub enum ZipEncryption {
    #[default]
    Aes256,
    /// ZIP 传统 ZipCrypto。
    ZipCrypto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 解压时同名文件的处理策略。
pub enum OverwritePolicy {
    #[default]
    Ask,
    /// 覆盖同名文件。
    Overwrite,
    /// 跳过同名文件。
    Skip,
    /// 同名文件自动改名。
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
/// ZIP 条目压缩方法。
pub enum ZipCompression {
    #[default]
    Deflate,
    /// ZIP BZip2 压缩。
    Bzip2,
    /// ZIP LZMA 压缩。
    Lzma,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 7Z 条目压缩方法。
pub enum SevenZCompression {
    #[default]
    Lzma2,
    /// 7Z LZMA 压缩。
    Lzma,
    /// 7Z PPMd 压缩。
    Ppmd,
    /// 7Z BZip2 压缩。
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

    /// 按界面预设推导默认级别；体积优先使用最高的 5 级。
    pub fn from_preset(preset: CompressionPreset) -> Self {
        match preset {
            CompressionPreset::Speed => Self::Fast,
            CompressionPreset::Balanced => Self::Balanced,
            CompressionPreset::Size => Self::Ultra,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 归档时间来源。
pub enum ArchiveTimeSource {
    #[default]
    Current,
    /// 保留源文件原始时间。
    Original,
    /// 使用源文件最新修改时间。
    Latest,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// 按文件修改时间过滤添加项。
pub enum FileTimeFilter {
    #[default]
    Any,
    /// 只添加指定时间之前的文件。
    Before,
    /// 只添加指定时间之后的文件。
    After,
    /// 只添加时间区间内的文件。
    Range,
}

#[derive(Debug, Clone)]
/// 创建归档的全部选项。
pub struct CompressionOptions {
    /// 输出归档格式。
    pub format: ArchiveFormat,
    /// 压缩级别；None 表示使用预设。
    pub level: Option<i64>,
    /// 速度与体积预设。
    pub preset: CompressionPreset,
    /// 是否创建固实归档。
    pub solid: bool,
    /// 7z：设置密码时同时加密归档文件头和条目数据流。
    /// ZIP 忽略此标志。
    pub encrypt_header: bool,
    /// LZMA 字典大小。
    pub dictionary_size: Option<u32>,
    /// 压缩线程数。
    pub threads: u32,
    /// ZIP 条目压缩方法。
    pub zip_compression: ZipCompression,
    /// 7Z 条目压缩方法。
    pub sevenz_compression: SevenZCompression,
    /// ZPAQ 数字压缩级别；其他格式忽略。
    pub zpaq_compression: ZpaqCompression,
    /// LZMA 字长。
    pub word_size: Option<u32>,
    /// 7Z 固实块大小。
    pub solid_block_size: Option<u64>,
    /// 归档时间来源。
    pub archive_time: ArchiveTimeSource,
    /// 按修改时间过滤添加项。
    pub file_time_filter: FileTimeFilter,
    /// 只添加此时间之后修改的文件。
    pub modified_after: Option<DateTime<Utc>>,
    /// 只添加此时间之前修改的文件。
    pub modified_before: Option<DateTime<Utc>>,
    /// 归档内路径根处理方式。
    pub path_mode: PathMode,
    /// 输出归档密码。
    pub password: Option<Zeroizing<String>>,
    /// ZIP 密码加密算法。
    pub zip_encryption: ZipEncryption,
    /// 归档注释。
    pub comment: Option<String>,
    /// 分卷大小；None 表示不分卷。
    pub split_size: Option<u64>,
    /// 创建后立即完整测试。
    pub test_after: bool,
    /// 测试成功后删除源文件。
    pub delete_source: bool,
    /// 生成自解压 EXE；只由 `ArchiveService::create_sfx` 处理，`create` 忽略。
    pub sfx: bool,
    /// 排除的 glob 规则。
    pub exclude_patterns: Vec<String>,
    /// 强制 Store 的 glob 规则。
    pub store_only_patterns: Vec<String>,
    /// 后台任务取消标志。
    pub cancel: Option<Arc<AtomicBool>>,
    /// 伪装输出参数；`None` 表示普通归档。
    ///
    /// 只有 `ArchiveService::create` 会读取该字段：设置后先生成内层归档，
    /// 再包装成媒体外观；`create_sfx` 与该字段互斥。
    pub disguise: Option<DisguiseOptions>,
}

impl Default for CompressionOptions {
    /// 返回默认解压选项。
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
            disguise: None,
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
    /// 返回默认解压选项。
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024 * 1024 * 1024,
            max_entries: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// 解压归档的全部选项。
pub struct ExtractionOptions {
    /// 解压输出目录。
    pub output_dir: std::path::PathBuf,
    /// 只解压这些归档内路径；为空表示全部。
    pub selected_paths: Vec<String>,
    /// 是否允许覆盖同名文件。
    pub overwrite: bool,
    /// 同名文件的跳过、覆盖或重命名策略。
    pub overwrite_policy: OverwritePolicy,
    /// 解压密码。
    pub password: Option<Zeroizing<String>>,
    /// 是否保留归档内目录层级。
    pub preserve_paths: bool,
    /// 后台任务取消标志。
    pub cancel: Option<Arc<AtomicBool>>,
    /// 压缩炸弹预算；默认值足够宽松，可按需收紧。
    pub budget: ExtractionBudget,
}

#[derive(Debug, Clone, Default)]
/// 解压完成后的文件数、字节数和跳过数。
pub struct ExtractReport {
    /// 成功写出的文件数。
    pub files: usize,
    /// 成功写出的字节数。
    pub bytes: u64,
    /// 因覆盖策略或过滤跳过的条目数。
    pub skipped: usize,
}

#[derive(Debug, Clone, Default)]
/// 创建完成后的输出、条目数和字节数。
pub struct CreateReport {
    /// 写入归档的文件数。
    pub files: usize,
    /// 写入归档的原始字节数。
    pub bytes: u64,
    /// 输出归档路径。
    pub output: std::path::PathBuf,
    /// 创建后清理源文件或临时文件时的非致命错误。
    pub cleanup_errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
/// 归档加密状态和加密算法描述。
pub struct EncryptionInfo {
    /// 是否加密。
    pub encrypted: bool,
    /// 归档头或文件名是否加密。
    pub header_encrypted: bool,
    /// 加密信息对应的归档格式。
    pub format: Option<ArchiveFormat>,
}

mod detect;
pub use detect::{detect_format, detect_magic};
