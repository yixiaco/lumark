//! 按扩展名和文件头魔数识别归档格式。

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
    if name.ends_with(".xar") || name.ends_with(".pkg") {
        return ArchiveFormat::Xar;
    }
    if name.ends_with(".rpm") {
        return ArchiveFormat::Rpm;
    }
    if name.ends_with(".msi") {
        return ArchiveFormat::Msi;
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

/// 按文件头魔数识别归档格式。
pub fn detect_magic(bytes: &[u8]) -> ArchiveFormat {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return ArchiveFormat::Zip;
    }
    if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return ArchiveFormat::SevenZip;
    }
    if bytes.starts_with(b"xar!") {
        return ArchiveFormat::Xar;
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

#[cfg(test)]
mod tests {
    use super::{ArchiveFormat, detect_format, detect_magic};
    use std::path::Path;

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
}
