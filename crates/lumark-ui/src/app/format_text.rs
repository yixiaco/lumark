//! 归档格式在界面中的展示名称。

use lumark_archive::ArchiveFormat;

/// 返回当前语言的格式名称。
pub(crate) fn format_name(format: ArchiveFormat) -> String {
    format_name_language(format, crate::i18n::is_english())
}

/// 按指定语言返回格式名称。
pub(crate) fn format_name_language(format: ArchiveFormat, english: bool) -> String {
    if english {
        format!("{} archive", format.extension().to_ascii_uppercase())
    } else {
        format.label().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::format_name_language;
    use lumark_archive::ArchiveFormat;

    /// 验证中英文格式名称。
    #[test]
    fn format_names_follow_language() {
        assert_eq!(
            format_name_language(ArchiveFormat::Zip, true),
            "ZIP archive"
        );
        assert_eq!(
            format_name_language(ArchiveFormat::Zip, false),
            "ZIP 压缩文件"
        );
    }
}
