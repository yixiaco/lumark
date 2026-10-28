//! 归档服务内部进度和分卷测试。

use super::*;

#[cfg(test)]
mod solid_block_tests {
    use super::{DEFAULT_SOLID_BLOCK_BYTES, solid_blocks};
    use std::path::PathBuf;

    /// 生成只带大小信息的占位文件列表，测试不落盘。
    fn files(sizes: &[u64]) -> Vec<(String, PathBuf, u64)> {
        sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                (
                    format!("f{index}"),
                    PathBuf::from(format!("f{index}")),
                    *size,
                )
            })
            .collect()
    }

    /// 文件数超过上限时按上限切块。
    #[test]
    fn splits_by_file_count() {
        let plan = files(&[1; 5]);
        let blocks = solid_blocks(&plan, DEFAULT_SOLID_BLOCK_BYTES, 2);
        assert_eq!(
            blocks.iter().map(|block| block.len()).collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
    }

    /// 累计字节超过上限时切块，超过上限的单文件独占一块。
    #[test]
    fn splits_by_bytes_and_keeps_oversized_file_alone() {
        let plan = files(&[600, 600, 5000, 100]);
        let blocks = solid_blocks(&plan, 1000, 100);
        assert_eq!(
            blocks.iter().map(|block| block.len()).collect::<Vec<_>>(),
            vec![1, 1, 1, 1]
        );
    }

    /// 小目录仍然只有一个固实块，保持既有压缩结果。
    #[test]
    fn keeps_small_plan_in_one_block() {
        let plan = files(&[10, 20, 30]);
        let blocks = solid_blocks(&plan, DEFAULT_SOLID_BLOCK_BYTES, 4096);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].len(), 3);
    }
}

#[cfg(test)]
mod progress_tests {
    use std::io::{Read, sink};
    use std::time::Duration;

    /// 慢速读取器：每次只返回一小块并停顿，模拟大文件解压的分块读取。
    struct SlowReader {
        remaining: usize,
    }

    impl Read for SlowReader {
        /// 每次返回一小块并休眠，模拟慢速解压读取。
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(30));
            let size = buffer.len().min(self.remaining).min(1024);
            if size == 0 {
                return Ok(0);
            }
            self.remaining -= size;
            Ok(size)
        }
    }

    /// 已完成字节落在一段文件中间时，应换算成该文件的内部进度。
    #[test]
    fn chunk_state_maps_done_bytes_to_current_file() {
        let mut state = super::ChunkProgressState {
            active: false,
            base: 100,
            total: 0,
            chunk_bytes: 16,
            chunks: 0,
            entries_before: 2,
            entries_total: 5,
            bytes_total: 130,
            output_bytes: 0,
            spans: Vec::new(),
        };
        state.begin(
            100,
            30,
            2,
            vec![
                super::ProgressSpan {
                    name: "a.bin".to_string(),
                    start: 0,
                    end: 10,
                },
                super::ProgressSpan {
                    name: "b.bin".to_string(),
                    start: 10,
                    end: 30,
                },
            ],
        );
        let (name, done, total, entries) = state.locate(4);
        assert_eq!((name.as_str(), done, total, entries), ("a.bin", 4, 10, 2));
        let (name, done, total, entries) = state.locate(16);
        assert_eq!((name.as_str(), done, total, entries), ("b.bin", 6, 20, 3));
        let (name, done, total, entries) = state.locate(30);
        assert_eq!((name.as_str(), done, total, entries), ("b.bin", 20, 20, 4));
    }

    /// 单个大条目在复制过程中应多次回调进度，而不是只在结束时回调一次。
    #[test]
    fn copy_with_progress_reports_inside_single_file() {
        let mut input = SlowReader {
            remaining: 16 * 1024,
        };
        let mut output = sink();
        let mut reports = Vec::new();
        let total =
            super::copy_entry_with_progress(&mut input, &mut output, &None, u64::MAX, |done| {
                reports.push(done);
            })
            .unwrap();
        assert_eq!(total, 16 * 1024);
        assert!(reports.len() >= 2, "单文件内部应多次上报进度：{reports:?}");
        assert!(reports.iter().all(|done| *done > 0 && *done <= total));
    }
}

#[cfg(all(test, windows))]
mod source_plan_tests {
    use super::*;
    use std::process::Command;

    /// 目录联接不应被当作普通文件加入源计划。
    #[test]
    fn skips_directory_junctions() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("real.txt"), b"real").unwrap();
        let link = directory.path().join("link");
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status();
        if !status.is_ok_and(|status| status.success()) {
            // 非 NTFS 或缺少联接权限时跳过；普通 CI 环境通常可以创建。
            return;
        }
        let source = directory.path().to_path_buf();
        let plan = build_source_plan(
            std::slice::from_ref(&source),
            &CompressionOptions::default(),
        )
        .expect("源计划应成功");
        let paths = plan
            .files
            .iter()
            .map(|(path, _, _)| path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.iter().any(|path| path.ends_with("target/real.txt")));
        assert!(
            !paths.iter().any(|path| path.ends_with("link")),
            "目录联接入口不应进入计划：{paths:?}"
        );
    }
}
