//! 自解压运行时的端到端测试。
//!
//! 用当前构建的 `lumark-sfx.exe` 作为运行时模块，拼接真实 ZIP/7Z 数据段和
//! v2 页脚，再用 `/S` 静默模式运行，验证目录、密码和解压结果。

#![cfg(windows)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sevenz_rust2::encoder_options::AesEncoderOptions;
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, Password};

/// 轮询等待子进程退出，避免 GUI 程序异常时测试永久挂起。
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// 组装 v2 页脚。
fn footer(offset: u64, length: u64, format: u8, encrypted: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LMSFX002");
    bytes.push(format);
    bytes.push(u8::from(encrypted));
    bytes.extend_from_slice(&[0u8; 6]);
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes
}

/// 把当前运行时模块、载荷和页脚拼接成可执行的自解压文件。
fn make_sfx(directory: &Path, payload: &[u8], format: u8, encrypted: bool) -> PathBuf {
    let stub = PathBuf::from(env!("CARGO_BIN_EXE_lumark-sfx"));
    let stub_bytes = fs::read(&stub).expect("读取运行时模块");
    let bundle = directory.join("bundle.exe");
    let mut file = File::create(&bundle).expect("创建自解压文件");
    file.write_all(&stub_bytes).expect("写入运行时模块");
    file.write_all(payload).expect("写入载荷");
    file.write_all(&footer(
        stub_bytes.len() as u64,
        payload.len() as u64,
        format,
        encrypted,
    ))
    .expect("写入页脚");
    drop(file);
    bundle
}

/// 生成含单个文本条目的 ZIP 数据。
fn zip_payload(path: &Path, name: &str, contents: &[u8]) -> Vec<u8> {
    {
        let file = File::create(path).expect("创建 ZIP");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("开始 ZIP 条目");
        writer.write_all(contents).expect("写入 ZIP 条目");
        writer.finish().expect("完成 ZIP");
    }
    fs::read(path).expect("读取 ZIP")
}

/// 生成含单个文本条目的 7Z 数据，可选用密码。
fn seven_zip_payload(path: &Path, source: &Path, password: Option<&str>) -> Vec<u8> {
    let mut writer = ArchiveWriter::create(path).expect("创建 7Z");
    if let Some(password) = password {
        writer.set_content_methods(vec![
            EncoderConfiguration::new(EncoderMethod::LZMA2),
            AesEncoderOptions::new(Password::from(password)).into(),
        ]);
        writer.set_encrypt_header(true);
    }
    let entry = ArchiveEntry::from_path(source, "hello.txt".to_string());
    let mut input = File::open(source).expect("打开 7Z 源");
    writer
        .push_archive_entry(entry, Some(&mut input))
        .expect("写入 7Z 条目");
    writer.finish().expect("完成 7Z");
    fs::read(path).expect("读取 7Z")
}

/// 运行自解压程序并断言成功。
fn run_bundle(bundle: &Path, target: &Path, password: Option<&str>) {
    let mut child = Command::new(bundle)
        .arg("/S")
        .arg(format!("/D={}", target.display()))
        .args(password.map(|value| format!("/P={value}")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动自解压程序");
    let status =
        wait_with_timeout(&mut child, Duration::from_secs(30)).expect("自解压程序未在限期内退出");
    assert!(status.success(), "自解压程序退出码异常：{status:?}");
}

/// 静默 ZIP 自解压应解压到 /D 指定目录。
#[test]
fn silent_zip_sfx_extracts_to_selected_directory() {
    let directory = tempfile::tempdir().unwrap();
    let payload = zip_payload(
        &directory.path().join("payload.zip"),
        "hello.txt",
        b"zip sfx",
    );
    let bundle = make_sfx(directory.path(), &payload, 0, false);
    let target = directory.path().join("selected dir");
    run_bundle(&bundle, &target, None);
    assert_eq!(
        fs::read_to_string(target.join("hello.txt")).unwrap(),
        "zip sfx"
    );
}

/// 带密码的 7Z 自解压应使用 /P 并通过 /D 选择目录。
#[test]
fn silent_encrypted_seven_zip_sfx_uses_password() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("hello.txt");
    fs::write(&source, b"seven sfx").unwrap();
    let payload = seven_zip_payload(
        &directory.path().join("payload.7z"),
        &source,
        Some("sfx-pass"),
    );
    let bundle = make_sfx(directory.path(), &payload, 1, true);
    let target = directory.path().join("seven out");
    run_bundle(&bundle, &target, Some("sfx-pass"));
    assert_eq!(
        fs::read_to_string(target.join("hello.txt")).unwrap(),
        "seven sfx"
    );
}
