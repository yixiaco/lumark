//! 手工诊断 IoStore 容器：列出包路径，或读取指定条目。
//!
//! 用法：`iostore_list <toc> [aes-key] [entry-path]`。
//! 密钥与容器数据仅来自命令行，不写入任何文件或日志。

use std::error::Error;

use lumark_archive::IoStoreContainer;

/// 列出容器信息；给出条目路径时读取并输出长度。
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let toc = args.next().ok_or("缺少 .utoc 路径")?;
    let key = args.next();
    let entry = args.next();
    let container = IoStoreContainer::open(&toc, key.as_deref())?;
    println!(
        "files={} encrypted={} methods={:?}",
        container.files().len(),
        container.encrypted(),
        container.compression_methods()
    );
    let mut types = std::collections::BTreeMap::new();
    for index in 0..container.entry_count() {
        if let Some(kind) = container.chunk_type(index as u32) {
            *types.entry(kind).or_insert(0usize) += 1;
        }
    }
    println!("chunk types: {types:?}");
    for file in container.files().iter() {
        println!("{}\t{}", file.size, file.path);
    }
    if let Some(entry) = entry {
        let data = container.read_file(&entry)?;
        println!("read {entry}: {} bytes", data.len());
        println!("first bytes: {:02X?}", &data[..data.len().min(64)]);
        println!(
            "last bytes: {:02X?}",
            &data[data.len().saturating_sub(64)..]
        );
        let magic = [0xC1u8, 0x83, 0x2A, 0x9E];
        let position = data.windows(magic.len()).position(|window| window == magic);
        println!("uasset magic offset: {position:?}");
        if let Some(index) = container.files().iter().position(|file| file.path == entry) {
            let toc_index = container.files()[index].toc_index;
            if let Some(package_id) = container.package_id(toc_index) {
                println!("package id: {package_id:016X}");
                for chunk_type in 0..=13u8 {
                    for candidate in container.find_package_chunks(package_id, chunk_type) {
                        let size = container
                            .read_chunk(candidate)
                            .map(|data| data.len())
                            .unwrap_or(0);
                        println!("chunk {candidate}: type={chunk_type} size={size}");
                    }
                }
            }
        }
    }
    Ok(())
}
