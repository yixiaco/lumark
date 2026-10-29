//! 只读格式兼容性测试：ALZ、ZIP 旧方法、unarc 传统格式和 TAR.Z 检测。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use lumark_archive::{
    ArchiveError, ArchiveFormat, ArchiveService, ExtractionBudget, ExtractionOptions,
    OverwritePolicy,
};
use tempfile::tempdir;

/// ALZ 测试条目描述。
struct AlzSpec<'a> {
    name: &'a str,
    data: &'a [u8],
    /// 0 = Store，2 = raw Deflate。
    method: u8,
    encrypted: bool,
    is_dir: bool,
    crc_override: Option<u32>,
}

impl<'a> AlzSpec<'a> {
    /// 构造一个普通文件条目。
    fn file(name: &'a str, data: &'a [u8], method: u8) -> Self {
        Self {
            name,
            data,
            method,
            encrypted: false,
            is_dir: false,
            crc_override: None,
        }
    }
}

/// 测试侧 ZIP 传统加密状态，只用于生成 ALZ 加密样例。
struct TestZipCrypto {
    key: [u32; 3],
}

impl TestZipCrypto {
    /// 用密码初始化密钥。
    fn new(password: &[u8]) -> Self {
        let mut crypto = Self {
            key: [305419896, 591751049, 878082192],
        };
        for byte in password {
            crypto.update_keys(*byte);
        }
        crypto
    }

    /// 标准 PKWARE CRC32 单字节更新。
    fn crc32_byte(mut crc: u32, byte: u8) -> u32 {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        crc
    }

    /// 用明文字节推进密钥。
    fn update_keys(&mut self, byte: u8) {
        self.key[0] = Self::crc32_byte(self.key[0], byte);
        self.key[1] = self.key[1].wrapping_add(self.key[0] & 0xff);
        self.key[1] = self.key[1].wrapping_mul(134_775_813).wrapping_add(1);
        self.key[2] = Self::crc32_byte(self.key[2], (self.key[1] >> 24) as u8);
    }

    /// 计算当前密钥流字节。
    fn decrypt_byte(&self) -> u8 {
        let temp = (self.key[2] | 2) as u16;
        ((temp.wrapping_mul(temp ^ 1)) >> 8) as u8
    }

    /// 原地加密一段明文（头部和数据必须使用同一个状态连续加密）。
    fn encrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let plain = *byte;
            *byte = plain ^ self.decrypt_byte();
            self.update_keys(plain);
        }
    }
}

/// 用 raw Deflate 压缩一段数据。
fn raw_deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// 构建最小 ALZ 归档，覆盖 Store、Deflate 和 ZipCrypto 加密条目。
fn build_alz(specs: &[AlzSpec<'_>]) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"ALZ\x01");
    output.extend_from_slice(&[0u8; 4]);
    for spec in specs {
        let payload = if spec.method == 2 {
            raw_deflate(spec.data)
        } else {
            spec.data.to_vec()
        };
        let crc = spec
            .crc_override
            .unwrap_or_else(|| crc32fast::hash(spec.data));
        output.extend_from_slice(b"BLZ\x01");
        let name = spec.name.as_bytes();
        output.extend_from_slice(&(name.len() as u16).to_le_bytes());
        output.push(if spec.is_dir { 0x10 } else { 0x20 });
        output.extend_from_slice(&0u32.to_le_bytes());
        output.push(0x40 | u8::from(spec.encrypted));
        output.push(0);
        output.push(spec.method);
        output.push(0);
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        output.extend_from_slice(&(spec.data.len() as u32).to_le_bytes());
        output.extend_from_slice(name);
        if spec.encrypted {
            let mut header = [0u8; 12];
            header[11] = (crc >> 24) as u8;
            let mut crypto = TestZipCrypto::new(b"alz-pass");
            crypto.encrypt(&mut header);
            output.extend_from_slice(&header);
            let mut encrypted = payload;
            crypto.encrypt(&mut encrypted);
            output.extend_from_slice(&encrypted);
        } else {
            output.extend_from_slice(&payload);
        }
    }
    output.extend_from_slice(b"CLZ\x01");
    output.extend_from_slice(&[0u8; 12]);
    output.extend_from_slice(b"CLZ\x02");
    output
}

/// 把 ALZ 字节写入临时目录并返回路径。
fn write_alz(dir: &Path, name: &str, specs: &[AlzSpec<'_>]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, build_alz(specs)).unwrap();
    path
}

/// 构造默认解压选项。
fn extract_options(output_dir: &Path) -> ExtractionOptions {
    ExtractionOptions {
        output_dir: output_dir.to_path_buf(),
        overwrite: true,
        ..Default::default()
    }
}

/// 返回测试样例的绝对路径。
fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

/// 验证 ALZ 的 Store/Deflate、目录、列表、解压、测试和魔数识别。
#[test]
fn alz_store_deflate_and_directory_roundtrip() {
    let dir = tempdir().unwrap();
    let archive = write_alz(
        dir.path(),
        "sample.alz",
        &[
            AlzSpec::file("hello.txt", b"hello alz", 0),
            AlzSpec {
                name: "sub",
                data: b"",
                method: 0,
                encrypted: false,
                is_dir: true,
                crc_override: None,
            },
            AlzSpec::file("sub/nested.txt", b"nested deflate payload", 2),
        ],
    );

    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Alz));
    assert!(!info.encrypted);
    assert!(info.entries.iter().any(|entry| entry.path == "hello.txt"));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path == "sub" && entry.is_dir)
    );

    let output = dir.path().join("out");
    let report = ArchiveService::extract(&archive, &extract_options(&output), |_| {}).unwrap();
    assert_eq!(report.files, 2);
    assert_eq!(fs::read(output.join("hello.txt")).unwrap(), b"hello alz");
    assert_eq!(
        fs::read(output.join("sub/nested.txt")).unwrap(),
        b"nested deflate payload"
    );

    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 2);

    let renamed = dir.path().join("renamed.bin");
    fs::copy(&archive, &renamed).unwrap();
    assert_eq!(ArchiveService::detect(&renamed), ArchiveFormat::Alz);
}

/// 验证 ALZ 加密探测、密码校验、错误密码和正确密码解压。
#[test]
fn alz_encrypted_password_roundtrip() {
    let dir = tempdir().unwrap();
    let archive = write_alz(
        dir.path(),
        "secret.alz",
        &[AlzSpec {
            name: "secret.txt",
            data: b"secret alz payload",
            method: 0,
            encrypted: true,
            is_dir: false,
            crc_override: None,
        }],
    );

    let probe = ArchiveService::probe_encryption(&archive).unwrap();
    assert!(probe.encrypted);
    assert!(!probe.header_encrypted);
    assert!(ArchiveService::verify_password(&archive, "alz-pass").unwrap());
    assert!(!ArchiveService::verify_password(&archive, "wrong").unwrap());

    let output = dir.path().join("out");
    assert!(matches!(
        ArchiveService::extract(&archive, &extract_options(&output), |_| {}),
        Err(ArchiveError::PasswordRequired)
    ));
    let mut wrong = extract_options(&output);
    wrong.password = Some(zeroize::Zeroizing::new("wrong".to_string()));
    assert!(matches!(
        ArchiveService::extract(&archive, &wrong, |_| {}),
        Err(ArchiveError::WrongPassword)
    ));

    let mut right = extract_options(&output);
    right.password = Some(zeroize::Zeroizing::new("alz-pass".to_string()));
    let report = ArchiveService::extract(&archive, &right, |_| {}).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(
        fs::read(output.join("secret.txt")).unwrap(),
        b"secret alz payload"
    );
    let tested = ArchiveService::test(&archive, Some("alz-pass"), |_| {}).unwrap();
    assert_eq!(tested.files, 1);
}

/// 验证路径逃逸、预算超限和 CRC 不匹配都会留下干净的输出目录。
#[test]
fn alz_rejects_unsafe_path_budget_and_crc() {
    let dir = tempdir().unwrap();
    let unsafe_archive = write_alz(
        dir.path(),
        "unsafe.alz",
        &[AlzSpec::file("../evil.txt", b"escape", 0)],
    );
    let output = dir.path().join("unsafe-out");
    assert!(matches!(
        ArchiveService::extract(&unsafe_archive, &extract_options(&output), |_| {}),
        Err(ArchiveError::UnsafePath(_))
    ));
    assert!(!dir.path().join("evil.txt").exists());

    let budget_archive = write_alz(
        dir.path(),
        "budget.alz",
        &[AlzSpec::file("big.txt", &[0u8; 64], 0)],
    );
    let output = dir.path().join("budget-out");
    let options = ExtractionOptions {
        output_dir: output.clone(),
        overwrite: true,
        budget: ExtractionBudget {
            max_bytes: 8,
            max_entries: 10,
        },
        ..Default::default()
    };
    assert!(matches!(
        ArchiveService::extract(&budget_archive, &options, |_| {}),
        Err(ArchiveError::BudgetExceeded(_))
    ));
    assert!(!output.join("big.txt").exists());

    let crc_archive = write_alz(
        dir.path(),
        "crc.alz",
        &[AlzSpec {
            name: "bad.txt",
            data: b"crc mismatch",
            method: 0,
            encrypted: false,
            is_dir: false,
            crc_override: Some(0),
        }],
    );
    let output = dir.path().join("crc-out");
    assert!(matches!(
        ArchiveService::extract(&crc_archive, &extract_options(&output), |_| {}),
        Err(ArchiveError::Corrupt(_))
    ));
    assert!(!output.join("bad.txt").exists());
}

/// 验证 ALZ 解压遵循跳过和重命名覆盖策略。
#[test]
fn alz_overwrite_policy_skip_and_rename() {
    let dir = tempdir().unwrap();
    let archive = write_alz(
        dir.path(),
        "overwrite.alz",
        &[AlzSpec::file("hello.txt", b"new", 0)],
    );
    let output = dir.path().join("out");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("hello.txt"), b"old").unwrap();

    let mut skip = extract_options(&output);
    skip.overwrite_policy = OverwritePolicy::Skip;
    let report = ArchiveService::extract(&archive, &skip, |_| {}).unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(fs::read(output.join("hello.txt")).unwrap(), b"old");

    let mut rename = extract_options(&output);
    rename.overwrite_policy = OverwritePolicy::Rename;
    let report = ArchiveService::extract(&archive, &rename, |_| {}).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(fs::read(output.join("hello (1).txt")).unwrap(), b"new");
}

/// 构建最小单条目 ZIP，用于验证指定压缩方法的解码分支。
///
/// Deflate64 没有可用的 Rust 编码器；这里把一段普通 raw Deflate 流标记为
/// 方法 9。Deflate64 是 Deflate 的超集，普通 Deflate 流对 Deflate64 解码器
/// 仍然合法，因此足以验证方法分派和特性开关；扩展长度/距离码由上游
/// `deflate64` 解码器自身测试覆盖。
fn build_single_entry_zip(name: &str, data: &[u8], method: u16) -> Vec<u8> {
    let compressed = if method == 9 {
        raw_deflate(data)
    } else {
        data.to_vec()
    };
    let crc = crc32fast::hash(data);
    let mut output = Vec::new();
    output.extend_from_slice(b"PK\x03\x04");
    output.extend_from_slice(&20u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&method.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&crc.to_le_bytes());
    output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&(name.len() as u16).to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(name.as_bytes());
    output.extend_from_slice(&compressed);
    let central_offset = output.len() as u32;
    output.extend_from_slice(b"PK\x01\x02");
    output.extend_from_slice(&20u16.to_le_bytes());
    output.extend_from_slice(&20u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&method.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&crc.to_le_bytes());
    output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&(name.len() as u16).to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(name.as_bytes());
    let central_size = output.len() as u32 - central_offset;
    output.extend_from_slice(b"PK\x05\x06");
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&central_size.to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output
}

/// 验证 ZIP 方法 9（Deflate64）走通列表和解压。
#[test]
fn zip_deflate64_method_decodes() {
    let dir = tempdir().unwrap();
    let payload = b"deflate64 payload generated from a deflate subset";
    let archive = dir.path().join("deflate64.zip");
    fs::write(&archive, build_single_entry_zip("payload.txt", payload, 9)).unwrap();

    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Zip));
    assert_eq!(info.entries[0].method, "Deflate64");

    let output = dir.path().join("out");
    let report = ArchiveService::extract(&archive, &extract_options(&output), |_| {}).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(fs::read(output.join("payload.txt")).unwrap(), payload);
}

/// 验证 ZIPX/旧 ZIP 的 Shrink、Reduce、Implode 能解压。
#[test]
fn zip_legacy_methods_extract() {
    for (relative, method) in [
        ("zip/shrink.zip", "Shrink"),
        ("zip/reduce.zip", "Reduce"),
        ("zip/implode.zip", "Implode"),
    ] {
        let archive = fixture(relative);
        let info = ArchiveService::list(&archive, None).unwrap_or_else(|error| {
            panic!("{method} 样例列表失败：{relative}：{error}");
        });
        assert_eq!(info.format, Some(ArchiveFormat::Zip), "{relative}");
        assert!(!info.entries.is_empty(), "{relative} 没有条目");
        let output = tempdir().unwrap();
        let report = ArchiveService::extract(&archive, &extract_options(output.path()), |_| {})
            .unwrap_or_else(|error| panic!("{method} 样例解压失败：{relative}：{error}"));
        let files: Vec<_> = info.entries.iter().filter(|entry| !entry.is_dir).collect();
        assert_eq!(report.files, files.len(), "{relative} 解压文件数不一致");
        for entry in files {
            let path = output.path().join(entry.path.replace('\\', "/"));
            let length = fs::metadata(&path)
                .unwrap_or_else(|error| panic!("{relative} 缺少 {}：{error}", entry.path))
                .len();
            assert_eq!(length, entry.size, "{relative} {} 大小不一致", entry.path);
        }
    }
}

/// 验证 unarc-rs 传统格式的列表、解压和完整测试。
#[test]
fn legacy_formats_list_extract_and_test() {
    for (relative, format) in [
        ("legacy/zoo/default.zoo", ArchiveFormat::Zoo),
        ("legacy/ha/asc.ha", ArchiveFormat::Ha),
        ("legacy/hyp/license.hyp", ArchiveFormat::Hyp),
        ("legacy/sq/license.sq", ArchiveFormat::Sq),
        ("legacy/sqz/license_m1.sqz", ArchiveFormat::Sqz),
        ("legacy/uc2/normal.uc2", ArchiveFormat::Uc2),
        ("legacy/ice/license_lha.ice", ArchiveFormat::Ice),
        ("legacy/pi9/KOERPER.PI9", ArchiveFormat::PackIce),
        ("legacy/tarz/license.tar.Z", ArchiveFormat::TarZ),
    ] {
        let archive = fixture(relative);
        let info = ArchiveService::list(&archive, None)
            .unwrap_or_else(|error| panic!("{format:?} 列表失败：{relative}：{error}"));
        assert_eq!(info.format, Some(format), "{relative}");
        assert!(!info.entries.is_empty(), "{relative} 没有条目");
        let output = tempdir().unwrap();
        let report = ArchiveService::extract(&archive, &extract_options(output.path()), |_| {})
            .unwrap_or_else(|error| panic!("{format:?} 解压失败：{relative}：{error}"));
        assert!(report.files > 0, "{relative} 没有解压出文件");
        let tested = ArchiveService::test(&archive, None, |_| {})
            .unwrap_or_else(|error| panic!("{format:?} 测试失败：{relative}：{error}"));
        assert!(tested.files > 0, "{relative} 测试没有处理文件");
    }
}

/// 验证传统格式里的 `../` 条目被拒绝，不会逃出目标目录。
#[test]
fn legacy_traversal_entry_is_rejected() {
    let dir = tempdir().unwrap();
    let archive = fixture("legacy/sqz/store.sqz");
    let output = dir.path().join("out");
    assert!(matches!(
        ArchiveService::extract(&archive, &extract_options(&output), |_| {}),
        Err(ArchiveError::UnsafePath(_))
    ));
    assert!(!dir.path().join("LICENSE").exists());
}

/// 验证 `.tar.Z` 由扩展名识别为 TAR.Z，而独立 `.Z` 仍是 Unix compress。
#[test]
fn tar_z_detection_wins_over_unix_compress_magic() {
    let dir = tempdir().unwrap();
    let tar_z = dir.path().join("sample.tar.Z");
    fs::write(&tar_z, b"\x1f\x9d\x90broken").unwrap();
    assert_eq!(ArchiveService::detect(&tar_z), ArchiveFormat::TarZ);

    let unix_z = dir.path().join("sample.Z");
    fs::write(&unix_z, b"\x1f\x9d\x90broken").unwrap();
    assert_eq!(ArchiveService::detect(&unix_z), ArchiveFormat::UnixCompress);
}

/// 验证 WIM 读取路径：用标准 LZX 写入器生成小镜像后列表、解压和测试。
#[test]
fn wim_read_roundtrip_from_writer() {
    use wim::{Compression, Wim};

    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("hello.txt"), b"hello wim").unwrap();
    let archive = dir.path().join("sample.wim");
    let mut image = Wim::new(Compression::Lzx).unwrap();
    image.capture_image(&source).unwrap();
    image.write(&archive).unwrap();

    let info = ArchiveService::list(&archive, None).unwrap();
    assert_eq!(info.format, Some(ArchiveFormat::Wim));
    assert!(
        info.entries
            .iter()
            .any(|entry| entry.path.ends_with("hello.txt"))
    );

    let output = dir.path().join("out");
    let report = ArchiveService::extract(&archive, &extract_options(&output), |_| {}).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(fs::read(output.join("hello.txt")).unwrap(), b"hello wim");

    let tested = ArchiveService::test(&archive, None, |_| {}).unwrap();
    assert_eq!(tested.files, 1);
}

/// 验证损坏的传统归档返回错误而不是 panic。
#[test]
fn corrupt_legacy_archive_is_reported() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("broken.zoo");
    fs::write(&archive, b"not a zoo archive").unwrap();
    assert!(ArchiveService::list(&archive, None).is_err());
}
