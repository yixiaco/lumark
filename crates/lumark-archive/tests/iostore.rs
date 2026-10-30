//! IoStore（`.utoc` + `.ucas`）识别、列表、解密和块读取测试。
//!
//! 测试用写入器只覆盖读取器需要的字段：v8 头部、目录索引、压缩块和
//! AES-256-ECB 加密；它不是通用打包器，也不参与生产代码。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use aes::cipher::{BlockEncrypt, KeyInit};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use lumark_archive::{ArchiveError, ArchiveFormat, ArchiveService, IoStoreContainer};
use std::io::Write;
use tempfile::tempdir;

/// 测试容器使用的固定 AES-256 密钥。
const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
/// 测试用 IoStore 逻辑块大小。
const BLOCK_SIZE: u32 = 64 * 1024;

/// 一个待写入的包文件。
struct TestChunk {
    path: String,
    data: Vec<u8>,
    compress: bool,
}

impl TestChunk {
    /// 创建不压缩的测试块。
    fn store(path: &str, data: &[u8]) -> Self {
        Self {
            path: path.to_string(),
            data: data.to_vec(),
            compress: false,
        }
    }

    /// 创建 zlib 压缩的测试块。
    fn zlib(path: &str, data: &[u8]) -> Self {
        Self {
            path: path.to_string(),
            data: data.to_vec(),
            compress: true,
        }
    }
}

/// 目录节点。
#[derive(Default)]
struct DirNode {
    parent: Option<String>,
    children: Vec<String>,
    files: Vec<(String, u32)>,
}

/// 写入测试容器并返回 `.utoc` 路径。
fn write_container(directory: &Path, chunks: &[TestChunk], encrypted: bool) -> PathBuf {
    let toc_path = directory.join("test.utoc");
    let cas_path = directory.join("test.ucas");
    let any_compressed = chunks.iter().any(|chunk| chunk.compress);
    let methods = if any_compressed {
        vec!["Zlib".to_string()]
    } else {
        Vec::new()
    };

    let key = decode_test_key();
    let mut cas = Vec::new();
    let mut blocks = Vec::new();
    let mut offsets = Vec::new();
    let mut ids = Vec::new();
    let mut block_index = 0u32;
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        let logical_offset = u64::from(block_index) * u64::from(BLOCK_SIZE);
        for part in chunk.data.chunks(BLOCK_SIZE as usize) {
            let stored = if chunk.compress {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(part).unwrap();
                encoder.finish().unwrap()
            } else {
                part.to_vec()
            };
            let physical_offset = cas.len() as u64;
            let mut padded = stored.clone();
            if encrypted {
                padded.resize(padded.len().div_ceil(16) * 16, 0);
                encrypt(&key, &mut padded);
            }
            cas.extend_from_slice(&padded);
            blocks.push(Block {
                offset: physical_offset,
                compressed_size: stored.len() as u32,
                uncompressed_size: part.len() as u32,
                method_index: if chunk.compress { 1 } else { 0 },
            });
            block_index += 1;
        }
        offsets.push((logical_offset, chunk.data.len() as u64));
        let mut id = [0u8; 12];
        id[0..8].copy_from_slice(&(0x1000u64 + chunk_index as u64).to_le_bytes());
        id[8..10].copy_from_slice(&0u16.to_le_bytes());
        id[11] = 1; // ExportBundleData
        ids.push(id);
    }

    let directory_index = build_directory_index(chunks);
    let directory_index = if encrypted {
        let mut padded = directory_index;
        padded.resize(padded.len().div_ceil(16) * 16, 0);
        encrypt(&key, &mut padded);
        padded
    } else {
        directory_index
    };

    let mut toc = Vec::new();
    write_header(
        &mut toc,
        HeaderValues {
            version: 8,
            entry_count: chunks.len() as u32,
            block_count: blocks.len() as u32,
            method_count: methods.len() as u32,
            directory_index_size: directory_index.len() as u32,
            flags: 8 | u8::from(encrypted) << 1 | u8::from(any_compressed),
        },
    );
    for id in &ids {
        toc.extend_from_slice(id);
    }
    for (offset, length) in &offsets {
        let mut bytes = [0u8; 10];
        bytes[0..5].copy_from_slice(&offset.to_be_bytes()[3..]);
        bytes[5..10].copy_from_slice(&length.to_be_bytes()[3..]);
        toc.extend_from_slice(&bytes);
    }
    for block in &blocks {
        toc.extend_from_slice(&encode_block(block));
    }
    for method in &methods {
        let mut name = [0u8; 32];
        name[..method.len()].copy_from_slice(method.as_bytes());
        toc.extend_from_slice(&name);
    }
    toc.extend_from_slice(&directory_index);
    // v8 每个条目 24 字节元数据；读取器不解码，但必须保持文件结构完整。
    toc.resize(toc.len() + chunks.len() * 24, 0);
    fs::write(&toc_path, toc).unwrap();
    fs::write(&cas_path, cas).unwrap();
    toc_path
}

/// 头部字段。
struct HeaderValues {
    version: u8,
    entry_count: u32,
    block_count: u32,
    method_count: u32,
    directory_index_size: u32,
    flags: u8,
}

/// 压缩块。
struct Block {
    offset: u64,
    compressed_size: u32,
    uncompressed_size: u32,
    method_index: u8,
}

/// 写入 0x90 字节头部。
fn write_header(toc: &mut Vec<u8>, values: HeaderValues) {
    toc.extend_from_slice(b"-==--==--==--==-");
    toc.push(values.version);
    toc.push(0);
    toc.extend_from_slice(&0u16.to_le_bytes());
    toc.extend_from_slice(&0x90u32.to_le_bytes());
    toc.extend_from_slice(&values.entry_count.to_le_bytes());
    toc.extend_from_slice(&values.block_count.to_le_bytes());
    toc.extend_from_slice(&12u32.to_le_bytes());
    toc.extend_from_slice(&values.method_count.to_le_bytes());
    toc.extend_from_slice(&32u32.to_le_bytes());
    toc.extend_from_slice(&BLOCK_SIZE.to_le_bytes());
    toc.extend_from_slice(&values.directory_index_size.to_le_bytes());
    toc.extend_from_slice(&1u32.to_le_bytes());
    toc.extend_from_slice(&0x1234u64.to_le_bytes());
    toc.extend_from_slice(&[0u8; 16]);
    toc.push(values.flags);
    toc.push(0);
    toc.extend_from_slice(&0u16.to_le_bytes());
    toc.extend_from_slice(&0u32.to_le_bytes());
    toc.extend_from_slice(&u64::MAX.to_le_bytes());
    toc.extend_from_slice(&0u32.to_le_bytes());
    toc.extend_from_slice(&0u32.to_le_bytes());
    toc.extend_from_slice(&[0u8; 40]);
    assert_eq!(toc.len(), 0x90);
}

/// 编码一个压缩块。
fn encode_block(block: &Block) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    bytes[0..5].copy_from_slice(&block.offset.to_le_bytes()[0..5]);
    bytes[5..8].copy_from_slice(&block.compressed_size.to_le_bytes()[0..3]);
    bytes[8..11].copy_from_slice(&block.uncompressed_size.to_le_bytes()[0..3]);
    bytes[11] = block.method_index;
    bytes
}

/// 构建目录索引。
fn build_directory_index(chunks: &[TestChunk]) -> Vec<u8> {
    let mut directories: BTreeMap<String, DirNode> = BTreeMap::new();
    directories.insert(String::new(), DirNode::default());
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        let parts = chunk.path.split('/').collect::<Vec<_>>();
        let mut prefix = String::new();
        for part in &parts[..parts.len() - 1] {
            let child = if prefix.is_empty() {
                (*part).to_string()
            } else {
                format!("{prefix}/{part}")
            };
            directories
                .entry(prefix.clone())
                .or_default()
                .children
                .push(child.clone());
            directories.entry(child.clone()).or_default().parent = Some(prefix.clone());
            prefix = child;
        }
        directories
            .entry(prefix)
            .or_default()
            .files
            .push((parts[parts.len() - 1].to_string(), chunk_index as u32));
    }
    for directory in directories.values_mut() {
        directory.children.sort();
        directory.children.dedup();
        directory.files.sort();
    }
    let paths = directories.keys().cloned().collect::<Vec<_>>();
    let indices = paths
        .iter()
        .enumerate()
        .map(|(index, path)| (path.clone(), index as u32))
        .collect::<HashMap<_, _>>();
    let mut strings = Vec::<String>::new();
    let mut directory_entries = Vec::new();
    let mut file_entries = Vec::<FileEntry>::new();
    for path in &paths {
        let node = &directories[path];
        let name = if path.is_empty() {
            u32::MAX
        } else {
            intern(&mut strings, path.rsplit('/').next().unwrap_or(path))
        };
        let first_child = node
            .children
            .first()
            .map(|child| indices[child])
            .unwrap_or(u32::MAX);
        let next_sibling = node
            .parent
            .as_ref()
            .and_then(|parent| {
                let siblings = &directories[parent].children;
                let position = siblings.iter().position(|child| child == path)?;
                siblings.get(position + 1).map(|next| indices[next])
            })
            .unwrap_or(u32::MAX);
        let first_file = if node.files.is_empty() {
            u32::MAX
        } else {
            file_entries.len() as u32
        };
        for (file_index, (name, chunk_index)) in node.files.iter().enumerate() {
            let next_file = if file_index + 1 < node.files.len() {
                file_entries.len() as u32 + 1
            } else {
                u32::MAX
            };
            file_entries.push(FileEntry {
                name: intern(&mut strings, name),
                next_file,
                user_data: *chunk_index,
            });
        }
        directory_entries.push(DirectoryEntryBytes {
            name,
            first_child,
            next_sibling,
            first_file,
        });
    }
    let mut bytes = Vec::new();
    write_fstring(&mut bytes, "../../../");
    bytes.extend_from_slice(&(directory_entries.len() as u32).to_le_bytes());
    for entry in directory_entries {
        for value in [
            entry.name,
            entry.first_child,
            entry.next_sibling,
            entry.first_file,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&(file_entries.len() as u32).to_le_bytes());
    for entry in file_entries {
        for value in [entry.name, entry.next_file, entry.user_data] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for string in strings {
        write_fstring(&mut bytes, &string);
    }
    bytes
}

/// 目录索引条目字节。
struct DirectoryEntryBytes {
    name: u32,
    first_child: u32,
    next_sibling: u32,
    first_file: u32,
}

/// 文件索引条目。
struct FileEntry {
    name: u32,
    next_file: u32,
    user_data: u32,
}

/// 写入 Unreal FString。
fn write_fstring(output: &mut Vec<u8>, value: &str) {
    let bytes = if value.is_empty() {
        Vec::new()
    } else {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        bytes
    };
    output.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
    output.extend_from_slice(&bytes);
}

/// 返回字符串表索引，必要时追加。
fn intern(strings: &mut Vec<String>, value: &str) -> u32 {
    if let Some(index) = strings.iter().position(|item| item == value) {
        return index as u32;
    }
    strings.push(value.to_string());
    (strings.len() - 1) as u32
}

/// 测试密钥字节。
fn decode_test_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&TEST_KEY[index * 2..index * 2 + 2], 16).unwrap();
    }
    key
}

/// 就地 AES-256-ECB 加密。
fn encrypt(key: &[u8; 32], bytes: &mut [u8]) {
    let cipher = aes::Aes256::new_from_slice(key).unwrap();
    for chunk in bytes.chunks_mut(16) {
        let block = aes::Block::from_mut_slice(chunk);
        cipher.encrypt_block(block);
    }
}

/// 验证未加密容器的识别、列表、读取和只读限制。
#[test]
fn iostore_lists_and_reads_files() {
    let directory = tempdir().unwrap();
    let chunks = [
        TestChunk::store("TogetherBNB/Content/A/SM_Cube.uasset", b"mesh-data"),
        TestChunk::zlib("TogetherBNB/Content/A/T_Test.uasset", b"texture-data"),
        TestChunk::store("TogetherBNB/Content/B/readme.txt", b"hello"),
    ];
    let path = write_container(directory.path(), &chunks, false);
    assert_eq!(ArchiveService::detect(&path), ArchiveFormat::IoStore);
    let info = ArchiveService::list(&path, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::IoStore));
    assert!(!info.encrypted);
    assert_eq!(info.entries.len(), 3);
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "TogetherBNB/Content/A/SM_Cube.uasset")
    );
    let cube = info
        .entries
        .iter()
        .find(|entry| entry.path.ends_with("SM_Cube.uasset"))
        .unwrap();
    assert_eq!(cube.size, 9);
    assert_eq!(cube.method, "Store");
    let texture = info
        .entries
        .iter()
        .find(|entry| entry.path.ends_with("T_Test.uasset"))
        .unwrap();
    assert_eq!(texture.method, "Zlib");

    let container = IoStoreContainer::open(&path, None).unwrap();
    assert_eq!(
        container
            .read_file("TogetherBNB/Content/A/SM_Cube.uasset")
            .unwrap(),
        b"mesh-data"
    );
    assert_eq!(
        container
            .read_file("TogetherBNB/Content/A/T_Test.uasset")
            .unwrap(),
        b"texture-data"
    );
    assert_eq!(container.files().len(), 3);

    // IoStore 只读：解压和完整测试必须给出可读错误。
    let output = directory.path().join("out");
    let result = ArchiveService::extract(
        &path,
        &lumark_archive::ExtractionOptions {
            output_dir: output,
            ..Default::default()
        },
        |_| {},
    );
    assert!(matches!(result, Err(ArchiveError::InvalidRequest(_))));
    assert!(matches!(
        ArchiveService::test(&path, None, |_| {}),
        Err(ArchiveError::InvalidRequest(_))
    ));
}

/// 验证加密容器的密钥要求、错误分类和正确解密。
#[test]
fn iostore_encrypted_requires_matching_key() {
    let directory = tempdir().unwrap();
    let chunks = [TestChunk::store("TogetherBNB/Secret.uasset", b"secret")];
    let path = write_container(directory.path(), &chunks, true);
    let probe = ArchiveService::probe_encryption(&path).unwrap();
    assert!(probe.encrypted);
    assert!(probe.header_encrypted);
    assert!(matches!(
        ArchiveService::list(&path, None),
        Err(ArchiveError::PasswordRequired)
    ));
    assert!(matches!(
        ArchiveService::list(&path, Some(&"1".repeat(64))),
        Err(ArchiveError::WrongPassword)
    ));
    assert!(ArchiveService::verify_password(&path, TEST_KEY).unwrap());
    assert!(!ArchiveService::verify_password(&path, &"0".repeat(64)).unwrap());
    let info = ArchiveService::list(&path, Some(TEST_KEY)).unwrap();
    assert_eq!(info.entries.len(), 1);
    assert!(info.encrypted);
    let container = IoStoreContainer::open(&path, Some(TEST_KEY)).unwrap();
    assert_eq!(
        container.read_file("TogetherBNB/Secret.uasset").unwrap(),
        b"secret"
    );
    assert_eq!(
        container
            .read_file("TogetherBNB/Missing.uasset")
            .unwrap_err()
            .to_string(),
        "归档文件损坏：IoStore 中不存在条目：TogetherBNB/Missing.uasset"
    );
}

/// 验证损坏头部、缺失 `.ucas` 和截断文件不会 panic。
#[test]
fn iostore_corrupt_inputs_fail_cleanly() {
    let directory = tempdir().unwrap();
    let path = write_container(
        directory.path(),
        &[TestChunk::store("A.uasset", b"data")],
        false,
    );
    fs::write(&path, b"broken").unwrap();
    assert!(ArchiveService::list(&path, None).is_err());

    let directory = tempdir().unwrap();
    let path = write_container(
        directory.path(),
        &[TestChunk::store("A.uasset", b"data")],
        false,
    );
    fs::remove_file(path.with_extension("ucas")).unwrap();
    assert!(matches!(
        ArchiveService::list(&path, None),
        Err(ArchiveError::Corrupt(_))
    ));

    let directory = tempdir().unwrap();
    let path = write_container(
        directory.path(),
        &[TestChunk::store("A.uasset", b"data")],
        false,
    );
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, &bytes[..0x40]).unwrap();
    assert!(ArchiveService::list(&path, None).is_err());
}

/// 验证版本位不影响块类型和包 ID 读取。
#[test]
fn iostore_exposes_chunk_metadata() {
    let directory = tempdir().unwrap();
    let path = write_container(
        directory.path(),
        &[TestChunk::store("Pkg/A.uasset", b"data")],
        false,
    );
    let container = IoStoreContainer::open(&path, None).unwrap();
    assert_eq!(container.chunk_type(0), Some(1));
    assert_eq!(container.package_id(0), Some(0x1000));
    assert_eq!(container.find_package_chunks(0x1000, 1), vec![0]);
    assert!(container.compression_methods().is_empty());
}
