//! Oodle 运行时下载与缓存（O-3）。
//!
//! 下载 zip → 可选 SHA-256 校验 → 缓存 `oodle.zip` → 解压出
//! `oo2core*.dll`/`oodle*.dll`。只取 zip 内文件名，拒绝目录穿越；
//! 下载/解压均使用临时文件，失败不会留下半成品库。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

/// 下载与缓存配置。
#[derive(Debug, Clone)]
pub struct OodleDownloadOptions {
    /// zip 下载地址。
    pub url: String,
    /// 缓存目录（保存 `oodle.zip` 与解压出的库）。
    pub cache_dir: PathBuf,
    /// 期望的 zip SHA-256（十六进制，大小写不敏感）。
    pub expected_sha256: Option<String>,
    /// 优先选择的动态库文件名（如 `oo2core_9_win64.dll`）。
    pub preferred_library: Option<String>,
}

/// 安装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OodleDownloadResult {
    /// 解压出的动态库路径。
    pub library: PathBuf,
    /// 缓存 zip 路径。
    pub archive: PathBuf,
    /// 是否命中缓存（未重新下载）。
    pub cache_hit: bool,
}

/// 下载并缓存 Oodle 库（带进度：已下载字节、总字节）。
pub fn download_and_cache_oodle(
    options: &OodleDownloadOptions,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<OodleDownloadResult, String> {
    fs::create_dir_all(&options.cache_dir)
        .map_err(|error| format!("创建 Oodle 缓存目录失败：{error}"))?;
    let archive_path = options.cache_dir.join("oodle.zip");
    if archive_path.is_file()
        && cached_archive_matches(&archive_path, options.expected_sha256.as_deref())
    {
        let library = extract_oodle_library(
            &archive_path,
            &options.cache_dir,
            options.preferred_library.as_deref(),
        )?;
        return Ok(OodleDownloadResult {
            library,
            archive: archive_path,
            cache_hit: true,
        });
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("Lumark/OodleDownloader")
        .build()
        .map_err(|error| format!("创建下载客户端失败：{error}"))?;
    let mut response = client
        .get(&options.url)
        .send()
        .map_err(|error| format!("下载 Oodle 失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 Oodle 失败：HTTP {}（{}）",
            response.status(),
            options.url
        ));
    }
    let total = response.content_length();
    let temporary = options.cache_dir.join("oodle.zip.part");
    let mut file =
        File::create(&temporary).map_err(|error| format!("创建 Oodle 临时文件失败：{error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("读取 Oodle 下载流失败：{error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| format!("写入 Oodle 缓存失败：{error}"))?;
        downloaded += read as u64;
        progress(downloaded, total);
    }
    file.flush()
        .map_err(|error| format!("刷新 Oodle 缓存失败：{error}"))?;
    drop(file);
    let actual = format!("{:x}", hasher.finalize());
    if let Some(expected) = options.expected_sha256.as_deref()
        && !actual.eq_ignore_ascii_case(expected.trim())
    {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "Oodle zip SHA-256 不匹配：期望 {expected}，实际 {actual}"
        ));
    }
    fs::rename(&temporary, &archive_path)
        .map_err(|error| format!("保存 Oodle 缓存失败：{error}"))?;
    let library = extract_oodle_library(
        &archive_path,
        &options.cache_dir,
        options.preferred_library.as_deref(),
    )?;
    Ok(OodleDownloadResult {
        library,
        archive: archive_path,
        cache_hit: false,
    })
}

/// 直接安装一个已取得的 zip（离线/测试用）。
pub fn install_oodle_zip(
    zip_bytes: &[u8],
    cache_dir: &Path,
    expected_sha256: Option<&str>,
    preferred_library: Option<&str>,
) -> Result<OodleDownloadResult, String> {
    let actual = format!("{:x}", Sha256::digest(zip_bytes));
    if let Some(expected) = expected_sha256
        && !actual.eq_ignore_ascii_case(expected.trim())
    {
        return Err(format!(
            "Oodle zip SHA-256 不匹配：期望 {expected}，实际 {actual}"
        ));
    }
    fs::create_dir_all(cache_dir).map_err(|error| format!("创建 Oodle 缓存目录失败：{error}"))?;
    let archive_path = cache_dir.join("oodle.zip");
    fs::write(&archive_path, zip_bytes).map_err(|error| format!("保存 Oodle zip 失败：{error}"))?;
    let library = extract_oodle_library(&archive_path, cache_dir, preferred_library)?;
    Ok(OodleDownloadResult {
        library,
        archive: archive_path,
        cache_hit: false,
    })
}

/// 从缓存 zip 解压动态库；已存在同名库时直接复用。
pub fn extract_oodle_library(
    archive_path: &Path,
    cache_dir: &Path,
    preferred_library: Option<&str>,
) -> Result<PathBuf, String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 Oodle zip 失败：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("解析 Oodle zip 失败：{error}"))?;
    let mut candidates: Vec<(String, usize)> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("读取 Oodle zip 条目失败：{error}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = enclosed.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_oodle_library(name) {
            candidates.push((name.to_string(), index));
        }
    }
    if candidates.is_empty() {
        return Err("Oodle zip 中没有找到 oo2core*.dll/oodle*.dll".to_string());
    }
    if let Some(preferred) = preferred_library
        && let Some(position) = candidates
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(preferred))
    {
        let selected = candidates.remove(position);
        candidates.insert(0, selected);
    }
    let (name, index) = &candidates[0];
    let target = cache_dir.join(name);
    if target.is_file() {
        return Ok(target);
    }
    let mut entry = archive
        .by_index(*index)
        .map_err(|error| format!("读取 Oodle 库条目失败：{error}"))?;
    let temporary = cache_dir.join(format!("{name}.part"));
    let mut output =
        File::create(&temporary).map_err(|error| format!("创建 Oodle 库临时文件失败：{error}"))?;
    std::io::copy(&mut entry, &mut output)
        .map_err(|error| format!("解压 Oodle 库失败：{error}"))?;
    output
        .flush()
        .map_err(|error| format!("刷新 Oodle 库失败：{error}"))?;
    drop(output);
    fs::rename(&temporary, &target).map_err(|error| format!("保存 Oodle 库失败：{error}"))?;
    Ok(target)
}

/// 判断缓存 zip 的 SHA-256；没有期望值时只检查文件存在。
fn cached_archive_matches(path: &Path, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return false,
        }
    }
    format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected.trim())
}

/// 动态库文件名规则。
fn is_oodle_library(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".dll") && (lower.starts_with("oo2core") || lower.starts_with("oodle"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    /// 生成只含一个假 DLL 的 zip。
    fn oodle_zip(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .start_file(name, SimpleFileOptions::default())
                .expect("zip 条目");
            writer.write_all(contents).expect("写入 zip");
            writer.finish().expect("完成 zip");
        }
        buffer.into_inner()
    }

    /// 安装 zip、SHA 校验、缓存复用。
    #[test]
    fn installs_and_reuses_cached_library() {
        let directory = tempfile::tempdir().expect("临时目录");
        let bytes = oodle_zip("oo2core_9_win64.dll", b"fake dll");
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let result = install_oodle_zip(
            &bytes,
            directory.path(),
            Some(&sha),
            Some("oo2core_9_win64.dll"),
        )
        .expect("安装");
        assert_eq!(result.library.file_name().unwrap(), "oo2core_9_win64.dll");
        assert_eq!(fs::read(&result.library).expect("读取库"), b"fake dll");
        assert!(result.archive.is_file());
        assert!(!result.cache_hit);
        assert!(cached_archive_matches(&result.archive, Some(&sha)));
        let second = extract_oodle_library(
            &result.archive,
            directory.path(),
            Some("oo2core_9_win64.dll"),
        )
        .expect("复用");
        assert_eq!(second, result.library);
    }

    /// SHA 不匹配时不写缓存、不产生库。
    #[test]
    fn rejects_checksum_mismatch() {
        let directory = tempfile::tempdir().expect("临时目录");
        let bytes = oodle_zip("oo2core_9_win64.dll", b"fake dll");
        assert!(install_oodle_zip(&bytes, directory.path(), Some("00"), None).is_err());
        assert!(!directory.path().join("oo2core_9_win64.dll").exists());
    }

    /// 路径穿越条目不会被当成动态库解压。
    #[test]
    fn rejects_path_traversal_entries() {
        let directory = tempfile::tempdir().expect("临时目录");
        let bytes = oodle_zip("../evil.dll", b"evil");
        assert!(install_oodle_zip(&bytes, directory.path(), None, None).is_err());
        assert!(!directory.path().join("evil.dll").exists());
    }

    /// 只接受 oo2core/oodle 命名的 DLL。
    #[test]
    fn only_oodle_library_names_are_candidates() {
        let directory = tempfile::tempdir().expect("临时目录");
        let bytes = oodle_zip("readme.dll", b"not oodle");
        assert!(install_oodle_zip(&bytes, directory.path(), None, None).is_err());
    }
}
