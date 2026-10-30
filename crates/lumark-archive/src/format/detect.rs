//! 按扩展名和文件头魔数识别归档格式。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::ArchiveFormat;

/// 按扩展名识别归档格式。
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
    // 带压缩后缀的 TAR 必须先于同名单流格式判断：`.tar.br`、`.tar.lz4`、
    // `.tar.lz`、`.tar.z` 分别以 `.br`、`.lz4`、`.lz`、`.z` 结尾，顺序
    // 反过来会把整个归档当成单文件流。`.tar.Z` 会先统一转成小写。
    if name.ends_with(".tar.br") {
        return ArchiveFormat::TarBr;
    }
    if name.ends_with(".tar.lz4") {
        return ArchiveFormat::TarLz4;
    }
    if name.ends_with(".tar.lz") {
        return ArchiveFormat::TarLz;
    }
    if name.ends_with(".tar.lzma") {
        return ArchiveFormat::TarLzma;
    }
    if name.ends_with(".tar.z") || name.ends_with(".taz") {
        return ArchiveFormat::TarZ;
    }
    if name.ends_with(".zipx") {
        return ArchiveFormat::Zipx;
    }
    if name.ends_with(".apk") {
        return ArchiveFormat::Apk;
    }
    if name.ends_with(".jar") {
        return ArchiveFormat::Jar;
    }
    if name.ends_with(".zip")
        || name.ends_with(".war")
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
    if name.ends_with(".pax") {
        return ArchiveFormat::Pax;
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
    if name.ends_with(".sz") || name.ends_with(".snappy") {
        return ArchiveFormat::Snappy;
    }
    if name.ends_with(".bz3") {
        return ArchiveFormat::Bz3;
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
    if name.ends_with(".utoc") {
        return ArchiveFormat::IoStore;
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
    if name.ends_with(".xar") || name.ends_with(".pkg") {
        return ArchiveFormat::Xar;
    }
    if name.ends_with(".rpm") {
        return ArchiveFormat::Rpm;
    }
    if name.ends_with(".msi") {
        return ArchiveFormat::Msi;
    }
    if name.ends_with(".chm") {
        return ArchiveFormat::Chm;
    }
    if name.ends_with(".squashfs") || name.ends_with(".sqfs") {
        return ArchiveFormat::SquashFs;
    }
    if name.ends_with(".dmg") {
        return ArchiveFormat::Dmg;
    }
    if name.ends_with(".egg") {
        return ArchiveFormat::Egg;
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
    // 传统格式通常没有稳定的魔数，只能按扩展名识别；解码由 `unarc-rs`
    // 统一适配层完成。
    if name.ends_with(".zoo") {
        return ArchiveFormat::Zoo;
    }
    if name.ends_with(".ha") {
        return ArchiveFormat::Ha;
    }
    if name.ends_with(".hyp") {
        return ArchiveFormat::Hyp;
    }
    if name.ends_with(".sq") || name.ends_with(".sq2") || name.ends_with(".qqq") {
        return ArchiveFormat::Sq;
    }
    if name.ends_with(".sqz") {
        return ArchiveFormat::Sqz;
    }
    if name.ends_with(".uc2") {
        return ArchiveFormat::Uc2;
    }
    if name.ends_with(".ice") {
        return ArchiveFormat::Ice;
    }
    if name.ends_with(".pi9") {
        return ArchiveFormat::PackIce;
    }
    ArchiveFormat::Unknown
}

/// 识别 `.pak` 家族：Quake PACK、Chromium 数据包、Unreal PAK、经典 ARC。
///
/// 只在扩展名 `.pak`（或无法按扩展名识别）时调用；`head` 是文件前 512 字节，
/// Unreal 的 footer 需要额外读取文件末尾。
pub(crate) fn detect_pak_family(path: &Path, head: &[u8]) -> Option<ArchiveFormat> {
    if head.starts_with(b"PACK") {
        return Some(ArchiveFormat::QuakePak);
    }
    let length = std::fs::metadata(path).ok()?.len();
    if chromium_pak_valid(head, length) {
        return Some(ArchiveFormat::ChromiumPak);
    }
    if unreal_pak_valid(path, length) {
        return Some(ArchiveFormat::UnrealPak);
    }
    if head.first() == Some(&0x1A) && head.get(1).is_some_and(|method| (1..=9).contains(method)) {
        return Some(ArchiveFormat::Arc);
    }
    None
}

/// 校验 Chromium data pack v4/v5 的头部、资源表和别名表边界。
fn chromium_pak_valid(head: &[u8], length: u64) -> bool {
    if head.len() < 9 {
        return false;
    }
    let version = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let (header_len, resource_count, alias_count, encoding) = match version {
        4 => {
            let count = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as u64;
            (9u64, count, 0u64, head[8])
        }
        5 => {
            if head.len() < 12 {
                return false;
            }
            let count = u16::from_le_bytes([head[8], head[9]]) as u64;
            let aliases = u16::from_le_bytes([head[10], head[11]]) as u64;
            (12u64, count, aliases, head[4])
        }
        _ => return false,
    };
    if encoding > 2 {
        return false;
    }
    let table_end = header_len
        .saturating_add((resource_count + 1).saturating_mul(6))
        .saturating_add(alias_count.saturating_mul(4));
    if table_end > length {
        return false;
    }
    // 只校验头部实际覆盖到的表项；完整表由读取器再次逐项检查。
    let available = (head.len() as u64).saturating_sub(header_len) / 6;
    let checked = (resource_count + 1).min(available);
    let mut previous = table_end;
    for index in 0..checked {
        let offset_position = header_len + index * 6 + 2;
        let Some(bytes) = head.get(offset_position as usize..offset_position as usize + 4) else {
            return false;
        };
        let offset = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64;
        if offset < previous || offset > length {
            return false;
        }
        previous = offset;
    }
    let alias_start = header_len + (resource_count + 1) * 6;
    let available_aliases = (head.len() as u64).saturating_sub(alias_start) / 4;
    for index in 0..alias_count.min(available_aliases) {
        let position = alias_start + index * 4;
        let Some(bytes) = head.get(position as usize..position as usize + 4) else {
            return false;
        };
        let target = u16::from_le_bytes([bytes[2], bytes[3]]) as u64;
        if target >= resource_count {
            return false;
        }
    }
    true
}

/// 从文件末尾扫描 Unreal PAK 的 footer 魔数并校验版本/索引边界。
fn unreal_pak_valid(path: &Path, length: u64) -> bool {
    if length < 64 {
        return false;
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let window = length.min(256);
    if file.seek(SeekFrom::End(-(window as i64))).is_err() {
        return false;
    }
    let mut tail = vec![0u8; window as usize];
    if file.read_exact(&mut tail).is_err() {
        return false;
    }
    let tail_start = length - window;
    const MAGIC: [u8; 4] = [0xE1, 0x12, 0x6F, 0x5A];
    for index in 0..tail.len().saturating_sub(24) {
        if tail[index..index + 4] != MAGIC {
            continue;
        }
        let version = u32::from_le_bytes([
            tail[index + 4],
            tail[index + 5],
            tail[index + 6],
            tail[index + 7],
        ]);
        if !(1..=11).contains(&version) {
            continue;
        }
        let magic = tail_start + index as u64;
        let prefix: u64 = if version >= 4 { 1 } else { 0 } + if version >= 7 { 16 } else { 0 };
        if magic < prefix {
            continue;
        }
        let index_offset =
            u64::from_le_bytes(tail[index + 8..index + 16].try_into().unwrap_or_default());
        let index_size =
            u64::from_le_bytes(tail[index + 16..index + 24].try_into().unwrap_or_default());
        if index_offset
            .checked_add(index_size)
            .is_none_or(|end| end > length)
        {
            continue;
        }
        let mut footer_end = magic + 44;
        if version == 9 {
            footer_end += 1;
        }
        if footer_end > length {
            continue;
        }
        let names = length - footer_end;
        let valid_names = if version >= 9 {
            names == 160
        } else if version == 8 {
            names == 128 || names == 160
        } else {
            names == 0
        };
        if valid_names {
            return true;
        }
    }
    false
}

/// 检查 Windows PE 签名：`MZ`、`e_lfanew` 和 `PE\0\0`。///
/// `bytes` 只需要覆盖 DOS 头和 `e_lfanew` 指向的 PE 签名字节。
pub fn is_pe_image(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let offset = u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    offset
        .checked_add(4)
        .is_some_and(|end| end <= bytes.len() && &bytes[offset..end] == b"PE\0\0")
}

/// 按文件头魔数识别归档格式。
pub fn detect_magic(bytes: &[u8]) -> ArchiveFormat {
    // Quake/idTech PAK 的 PACK 头足够强，任意扩展名都直接识别。
    if bytes.starts_with(b"PACK") {
        return ArchiveFormat::QuakePak;
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return ArchiveFormat::Zip;
    }
    if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return ArchiveFormat::SevenZip;
    }
    if bytes.starts_with(b"xar!") {
        return ArchiveFormat::Xar;
    }
    if bytes.starts_with(b"ITSF") {
        return ArchiveFormat::Chm;
    }
    if bytes.starts_with(b"hsqs") {
        return ArchiveFormat::SquashFs;
    }
    if bytes.starts_with(&0x4147_4745u32.to_le_bytes()) {
        return ArchiveFormat::Egg;
    }
    // ASAR 没有魔数，只能依靠头部 4 个长度字段和 JSON 根键识别；这段检查
    // 同时覆盖 Electron 官方 asar 与 Lumark 写出的归档。
    if bytes.len() >= 25
        && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 4
        && &bytes[16..25] == b"{\"files\":"
    {
        return ArchiveFormat::Asar;
    }
    if bytes.starts_with(b"\xed\xab\xee\xdb") {
        return ArchiveFormat::Rpm;
    }
    // ALZ 头固定为 "ALZ\x01"；多卷拆分由 `unalz` 在打开时自行拼接。
    if bytes.starts_with(b"ALZ\x01") {
        return ArchiveFormat::Alz;
    }
    // 加密 ZPAQ 以随机 salt 开头，只能靠扩展名识别；明文流式归档统一
    // 以 JIDAC 标签 `7kSt` 或旧版 `zPQ` 版本头开始。
    if bytes.starts_with(b"7kSt")
        || (bytes.starts_with(b"zPQ") && bytes.get(3).is_some_and(|version| *version >= 1))
    {
        return ArchiveFormat::Zpaq;
    }
    if bytes.starts_with(b"-==--==--==--==-") {
        return ArchiveFormat::IoStore;
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
    // Snappy Framed 的流标识块固定为 ff 06 00 00 后接 sNaPpY。
    if bytes.starts_with(b"\xff\x06\x00\x00sNaPpY") {
        return ArchiveFormat::Snappy;
    }
    if bytes.starts_with(b"BZ3v1") {
        return ArchiveFormat::Bz3;
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

#[cfg(test)]
mod tests {
    use super::{ArchiveFormat, detect_format, detect_magic};
    use std::path::Path;

    /// RAR4 和 RAR5 头部都归入只读 RAR 后端；列表层再按签名显示 RAR5。
    #[test]
    fn rar5_magic_is_recognized() {
        assert_eq!(
            detect_magic(b"Rar!\x1a\x07\x01\x00rest"),
            ArchiveFormat::Rar
        );
        assert_eq!(detect_magic(b"Rar!\x1a\x07\x00rest"), ArchiveFormat::Rar);
    }

    /// 验证新增 TAR 变体不会被同名单流格式抢先识别。
    #[test]
    fn compressed_tar_extensions_win_over_single_stream() {
        for (name, expected) in [
            ("backup.tar.br", ArchiveFormat::TarBr),
            ("backup.tar.lz4", ArchiveFormat::TarLz4),
            ("backup.tar.lz", ArchiveFormat::TarLz),
            ("backup.tar.lzma", ArchiveFormat::TarLzma),
            ("backup.tar.z", ArchiveFormat::TarZ),
            ("backup.tar.Z", ArchiveFormat::TarZ),
            ("backup.taz", ArchiveFormat::TarZ),
            ("backup.TAR.BR", ArchiveFormat::TarBr),
        ] {
            assert_eq!(detect_format(Path::new(name)), expected, "{name}");
        }
        assert_eq!(detect_format(Path::new("backup.br")), ArchiveFormat::Brotli);
        assert_eq!(detect_format(Path::new("backup.lz4")), ArchiveFormat::Lz4);
        assert_eq!(detect_format(Path::new("backup.lz")), ArchiveFormat::Lzip);
        assert_eq!(
            detect_format(Path::new("backup.Z")),
            ArchiveFormat::UnixCompress
        );
    }

    /// 验证传统格式按扩展名识别，并覆盖常见大小写别名。
    #[test]
    fn legacy_format_extensions_are_recognized() {
        for (name, expected) in [
            ("old.zoo", ArchiveFormat::Zoo),
            ("old.ha", ArchiveFormat::Ha),
            ("old.hyp", ArchiveFormat::Hyp),
            ("old.sq", ArchiveFormat::Sq),
            ("old.sq2", ArchiveFormat::Sq),
            ("old.qqq", ArchiveFormat::Sq),
            ("old.sqz", ArchiveFormat::Sqz),
            ("old.uc2", ArchiveFormat::Uc2),
            ("old.ice", ArchiveFormat::Ice),
            ("old.pi9", ArchiveFormat::PackIce),
            ("old.PI9", ArchiveFormat::PackIce),
        ] {
            assert_eq!(detect_format(Path::new(name)), expected, "{name}");
        }
    }

    /// 验证 ALZ 的固定头即使改名也能被魔数识别。
    #[test]
    fn alz_magic_is_detected() {
        assert_eq!(detect_magic(b"ALZ\x01rest"), ArchiveFormat::Alz);
    }

    /// 验证 CHM 的 ITSF 头可被识别。
    #[test]
    fn chm_magic_is_detected() {
        assert_eq!(detect_magic(b"ITSF\x03\x00\x00\x00"), ArchiveFormat::Chm);
    }

    /// 验证 SquashFS 的 hsqs 头可被识别。
    #[test]
    fn squashfs_magic_is_detected() {
        assert_eq!(
            detect_magic(b"hsqs\x00\x00\x00\x00"),
            ArchiveFormat::SquashFs
        );
    }

    /// 验证 PE 签名单独识别，普通 DOS 头和损坏偏移不误判。
    #[test]
    fn pe_signature_is_detected() {
        let mut bytes = vec![0u8; 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert!(super::is_pe_image(&bytes));
        bytes[0x80..0x84].copy_from_slice(b"NE\0\0");
        assert!(!super::is_pe_image(&bytes));
        assert!(!super::is_pe_image(b"MZ"));
    }

    /// 验证所有已登记格式都有非空标签和扩展名，且 Unknown 不在列表中。
    #[test]
    fn all_formats_are_fully_described() {
        assert!(!ArchiveFormat::ALL.contains(&ArchiveFormat::Unknown));
        assert!(!ArchiveFormat::ALL.contains(&ArchiveFormat::Disguised));
        for format in ArchiveFormat::ALL {
            assert!(!format.label().is_empty(), "{format:?} 缺少标签");
            assert!(!format.extension().is_empty(), "{format:?} 缺少扩展名");
        }
    }

    /// 验证单文件流格式与 TAR 变体的对应关系。
    #[test]
    fn single_stream_formats_have_tar_variants() {
        for (stream, tar) in [
            (ArchiveFormat::Gzip, ArchiveFormat::TarGz),
            (ArchiveFormat::Bzip2, ArchiveFormat::TarBz2),
            (ArchiveFormat::Xz, ArchiveFormat::TarXz),
            (ArchiveFormat::Zstd, ArchiveFormat::TarZst),
            (ArchiveFormat::Brotli, ArchiveFormat::TarBr),
            (ArchiveFormat::Lz4, ArchiveFormat::TarLz4),
        ] {
            assert!(stream.is_single_stream(), "{stream:?}");
            assert_eq!(stream.single_stream_tar_variant(), Some(tar), "{stream:?}");
            assert!(tar.is_writable(), "{tar:?}");
            assert!(!tar.is_single_stream(), "{tar:?}");
        }
        assert!(!ArchiveFormat::Zip.is_single_stream());
        assert_eq!(ArchiveFormat::Zip.single_stream_tar_variant(), None);
    }

    /// 验证可写格式包含本次新增的 TAR 变体与已有的 Brotli/LZ4。
    #[test]
    fn writable_formats_include_new_tar_variants() {
        for format in [
            ArchiveFormat::TarBr,
            ArchiveFormat::TarLz4,
            ArchiveFormat::TarLz,
            ArchiveFormat::TarLzma,
            ArchiveFormat::Brotli,
            ArchiveFormat::Lz4,
        ] {
            assert!(format.is_writable(), "{format:?} 应可写");
        }
        assert!(!ArchiveFormat::TarZ.is_writable());
        assert!(ArchiveFormat::Alz.supports_password());
    }

    /// 验证 EGG 的 `EGGA` 小端头可被识别。
    #[test]
    fn egg_magic_is_detected() {
        assert_eq!(detect_magic(b"EGGA\x01\x00"), ArchiveFormat::Egg);
    }
}
