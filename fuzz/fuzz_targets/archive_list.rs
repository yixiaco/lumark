//! 归档列表和测试入口的持续模糊测试目标。
//!
//! 运行：`cargo +nightly fuzz run archive_list`
//! 输入先写入临时 `.zip`，再交给 `ArchiveService::list` / `test`；目标只关心
//! 是否 panic 或越界，不要求解析成功。

#![no_main]

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use libfuzzer_sys::fuzz_target;
use lumark_archive::ArchiveService;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fuzz_target!(|data: &[u8]| {
    if data.len() > 4 * 1024 * 1024 {
        return;
    }
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "lumark-fuzz-{}-{id}",
        std::process::id()
    ));
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("input.zip");
    if fs::write(&path, data).is_ok() {
        let _ = ArchiveService::list(&path, None);
        let _ = ArchiveService::test(&path, None, |_| {});
    }
    let _ = fs::remove_dir_all(&directory);
});
