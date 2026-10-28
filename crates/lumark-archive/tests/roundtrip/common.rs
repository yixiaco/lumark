//! 往返测试共享的源目录、NEWC 条目和时间辅助。

use super::*;

/// 创建包含文本和二进制条目的测试源目录。
pub(crate) fn make_source(root: &std::path::Path) -> PathBuf {
    let source = root.join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("hello.txt"), b"hello from lumark").unwrap();
    fs::write(source.join("nested").join("data.bin"), [0u8, 1, 2, 3, 4]).unwrap();
    source
}
/// 向 NEWC/CPIO 测试数据追加一个条目。
pub(crate) fn append_newc_entry(output: &mut Vec<u8>, name: &str, data: &[u8]) {
    let name_len = name.len() + 1;
    let header = format!(
        "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
        1,
        0o100644,
        0,
        0,
        1,
        0,
        data.len(),
        0,
        0,
        0,
        0,
        name_len,
        0
    );
    output.extend_from_slice(header.as_bytes());
    output.extend_from_slice(name.as_bytes());
    output.push(0);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
    output.extend_from_slice(data);
    while !output.len().is_multiple_of(4) {
        output.push(0);
    }
}
/// 设置文件修改时间，供更新模式的时间比较使用。
pub(crate) fn set_modified(path: &std::path::Path, when: std::time::SystemTime) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(when).unwrap();
}
/// 解压归档并返回指定条目的文本内容。
pub(crate) fn extract_text(
    archive: &std::path::Path,
    output: &std::path::Path,
    name: &str,
) -> String {
    ArchiveService::extract(
        archive,
        &ExtractionOptions {
            output_dir: output.to_path_buf(),
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    String::from_utf8(fs::read(output.join(name)).unwrap()).unwrap()
}
/// 组装一个超出预算的归档和对应解压选项，验证预算拦截。
pub(crate) fn extract_with_small_budget(
    archive: &std::path::Path,
    output_dir: &std::path::Path,
) -> Result<lumark_archive::ExtractReport, lumark_archive::ArchiveError> {
    ArchiveService::extract(
        archive,
        &ExtractionOptions {
            output_dir: output_dir.to_path_buf(),
            overwrite: true,
            budget: ExtractionBudget {
                max_bytes: 1024 * 1024,
                max_entries: 10,
            },
            ..Default::default()
        },
        |_| {},
    )
}
