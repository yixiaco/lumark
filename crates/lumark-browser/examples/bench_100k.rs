//! 100k 行目录的真实加载和搜索基准。
//!
//! 运行方式：
//! `cargo run -p lumark-browser --release --example bench_100k`
//!
//! 该示例会在临时目录创建 10 万个空文件，测量 `BrowserModel` 的目录加载、
//! 排序和一次全量搜索耗时；文件数量固定，避免默认测试套件承担磁盘压力。

use std::fs::File;
use std::time::Instant;

use lumark_browser::{BrowserModel, SearchMode, SearchQuery};

/// 基准文件数量。
const ENTRIES: usize = 100_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    for index in 0..ENTRIES {
        File::create(directory.path().join(format!("entry-{index:06}.txt")))?;
    }

    let started = Instant::now();
    let model = BrowserModel::new(directory.path())?;
    let load = started.elapsed();
    assert_eq!(model.entries.len(), ENTRIES);

    let started = Instant::now();
    let matches = model.search(&SearchQuery {
        pattern: "entry-050".to_string(),
        recursive: false,
        case_sensitive: false,
        mode: SearchMode::Contains,
        extensions: Vec::new(),
        min_size: None,
        max_size: None,
        modified_after: None,
        modified_before: None,
    });
    let search = started.elapsed();
    assert!(!matches.is_empty());

    println!("目录加载 {ENTRIES} 项：{load:?}");
    println!("搜索 {ENTRIES} 项：{search:?}");
    Ok(())
}
