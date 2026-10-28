use super::super::*;

mod compress;
mod extract;
mod open;
mod password;
mod test;
mod util;
use util::*;

#[cfg(test)]
mod tests {
    use super::default_archive_name;
    use std::path::PathBuf;

    /// 多选时优先使用父目录名。
    #[test]
    fn archive_name_uses_parent_for_multiple_sources() {
        let sources = [
            PathBuf::from(r"C:\工作\a.txt"),
            PathBuf::from(r"C:\工作\b.txt"),
        ];
        assert_eq!(default_archive_name(&sources), "工作");
    }

    /// 根目录没有父目录名时退回第一个文件名。
    #[test]
    fn archive_name_falls_back_to_first_file_at_root() {
        let sources = [PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b.txt")];
        assert_eq!(default_archive_name(&sources), "a");
    }

    /// 单选文件使用文件 stem，而不是父目录名。
    #[test]
    fn archive_name_uses_file_name_for_single_source() {
        let sources = [PathBuf::from(r"C:\工作\a.txt")];
        assert_eq!(default_archive_name(&sources), "a");
    }
}
