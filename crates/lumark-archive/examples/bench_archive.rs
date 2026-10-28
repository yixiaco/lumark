//! 归档创建、列表、解压和测试的基准。
//!
//! 运行：`cargo run -p lumark-archive --release --example bench_archive`
//! 默认生成 32 MiB 文本源，分别测量 ZIP、7Z、TAR.GZ；结果用于发现明显的
//! 性能回退，不作为严格的跨机器基准。

use std::fs;
use std::time::{Duration, Instant};

use lumark_archive::{ArchiveFormat, ArchiveService, CompressionOptions, ExtractionOptions};
use tempfile::tempdir;

/// 源文件大小。
const SOURCE_BYTES: usize = 32 * 1024 * 1024;

/// 测量一个格式的完整闭环。
fn measure(format: ArchiveFormat) {
    let directory = tempdir().unwrap();
    let source = directory.path().join("source.txt");
    let bytes = (0..SOURCE_BYTES)
        .map(|index| b'a' + ((index / 97) % 26) as u8)
        .collect::<Vec<_>>();
    fs::write(&source, &bytes).unwrap();
    let archive = directory
        .path()
        .join(format!("bench.{}", format.extension()));
    let options = CompressionOptions {
        format,
        ..Default::default()
    };

    let started = Instant::now();
    ArchiveService::create(&archive, std::slice::from_ref(&source), &options, |_| {}).unwrap();
    let create = started.elapsed();

    let started = Instant::now();
    let _ = ArchiveService::list(&archive, None).unwrap();
    let list = started.elapsed();

    let output = directory.path().join("out");
    let started = Instant::now();
    ArchiveService::extract(
        &archive,
        &ExtractionOptions {
            output_dir: output,
            overwrite: true,
            ..Default::default()
        },
        |_| {},
    )
    .unwrap();
    let extract = started.elapsed();

    let started = Instant::now();
    let _ = ArchiveService::test(&archive, None, |_| {}).unwrap();
    let test = started.elapsed();

    println!(
        "{:<6} create {:>8.1} ms  list {:>7.1} ms  extract {:>8.1} ms  test {:>8.1} ms",
        format!("{format:?}"),
        millis(create),
        millis(list),
        millis(extract),
        millis(test)
    );
}

/// 转换为毫秒。
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn main() {
    println!("源大小：{} MiB", SOURCE_BYTES / (1024 * 1024));
    for format in [
        ArchiveFormat::Zip,
        ArchiveFormat::SevenZip,
        ArchiveFormat::TarGz,
    ] {
        measure(format);
    }
}
