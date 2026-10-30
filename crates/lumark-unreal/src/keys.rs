//! 从 UE Shipping.exe 提取 AES-256 密钥。
//!
//! 主路径参考 AESDumpster 的 `C7` （MOV imm32）签名模式：UE 把 32 字节
//! 密钥连续写成 8 个 dword 立即数（不同编译版本对应不同寄存器布局）。
//! 候选密钥必须再用真实加密容器校验，避免把随机常量误报成密钥。
//!
//! 本实现独立编写；签名模式与误报清单参考 aesdumpster-rs（MIT），详见
//! 仓库 `THIRD_PARTY_NOTICES.md`。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use memchr::memchr_iter;
use zeroize::Zeroizing;

use lumark_archive::{ArchiveFormat, ArchiveService};

use crate::game::DetectedGame;

/// 密钥提取阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStage {
    /// 正在扫描 exe。
    Analyzing,
    /// 正在用加密容器验证候选。
    Validating,
    /// 已完成。
    Done,
}

/// 一次密钥提取的结果。
#[derive(Debug, Clone)]
pub struct KeyExtraction {
    /// 找到并验证通过的密钥；未找到时为 `None`。
    pub key: Option<Zeroizing<String>>,
    /// 签名扫描得到的候选数量。
    pub candidate_count: usize,
    /// 验证通过的容器路径。
    pub validated_against: Option<PathBuf>,
    /// 失败或取消时的可读说明。
    pub message: Option<String>,
}

/// 一个签名模式：固定字节约束 + 8 个密钥 dword 偏移。
struct Signature {
    constraints: Vec<(usize, u8)>,
    key_offsets: [usize; 8],
}

/// 已知误报，按大写十六进制保存。
const FALSE_POSITIVES: &[&str] = &[
    "FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9FFD9",
    "67E6096A85AE67BB72F36E3C3AF54FA57F520E518C68059BABD9831F19CDE05B",
    "D89E05C107D57C3617DD703039590EF7310BC0FF11155868A78FF964A44FFABE",
    "9A99593F9A99593F0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F",
    "6F168073B9B21449D742241700068ADABC306FA9AA3831164DEE8DE34E0EFBB0",
    "0AD7633FCDCC4C3DCDCCCC3D52B8BE3F9A99593F9A99593FC9767E3FE17A543F",
    "168073C7B21449C7430C00064310BC304314AA3843184DEE431C4E0E83C4205B",
    "E6096AC7AE67BBC7430C3AF543107F5243148C684318ABD9431C19CD436C2000",
    "9E05C1C7D57C36C7430C39594310310B431411154318A78F431CA44F436C1C00",
    "9E05C1C7D57C36C7DD7030C7590EF7C70BC0FFC7155868C78FF964C7A44FFABE",
    "168073C7B21449C7422417C7068ADAC7306FA9C7383116C7EE8DE3C74E0EFBB0",
    "0AD7633FCDCC4C3D00C742143DC742183FC7421C3FC742203FC742247E3FC742",
    "0000803F0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53B54AE47A1",
    "0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F58583934",
    "0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F38583934",
    "0000803F0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53B34AE47A1",
    "0000803F0000803F0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D2C4260E5",
    "0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F5839343C4CC9767E",
    "0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F5839343C4CC9767E",
    "07D57C3617DD703039590EF7310BC0FF11155868A78FF964A44FFABE6C1C0000",
    "85AE67BB72F36E3C3AF54FA57F520E518C68059BABD9831F19CDE05B6C200000",
    "E6096AC7AE67BBC7F36E3CC7F54FA5C7520E51C768059BC7D9831FC719CDE05B",
    "0AD7A33E0AD7633F52B8BE3FE17A543FCDCC4C3D4260E53BAE47A13F3C583934",
    "E4D6E74FE4D667500044AC47926595380080DC43000A9B46000080BF000080BF",
    "D04C8F7D71ECC047D8A60970FBA31C9E9EC1250BBBF6459AC480947212E1DB8C",
];

/// 四条 AESDumpster 签名模式。
const PATTERNS: &[(&str, [usize; 8])] = &[
    (
        "C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ?",
        [3, 10, 17, 24, 35, 42, 49, 56],
    ),
    (
        "C7 ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ?",
        [2, 9, 16, 23, 30, 37, 44, 51],
    ),
    (
        "C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? 48 ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ?",
        [3, 10, 21, 28, 35, 42, 49, 56],
    ),
    (
        "C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? ? C7 ? ? ? ? ? C3",
        [51, 45, 38, 31, 24, 17, 10, 3],
    ),
];

/// 解析一次签名模式。
fn signatures() -> &'static [Signature] {
    static SIGNATURES: OnceLock<Vec<Signature>> = OnceLock::new();
    SIGNATURES.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|(pattern, offsets)| {
                let mut constraints = Vec::new();
                for (index, token) in pattern.split_whitespace().enumerate() {
                    if token == "?" || token == "??" {
                        continue;
                    }
                    if let Ok(value) = u8::from_str_radix(token, 16) {
                        constraints.push((index, value));
                    }
                }
                Signature {
                    constraints,
                    key_offsets: *offsets,
                }
            })
            .collect()
    })
}

/// 从 Shipping.exe 的字节缓冲区提取候选密钥。
fn collect_candidates(data: &[u8]) -> Vec<String> {
    let false_positives = FALSE_POSITIVES
        .iter()
        .map(|value| value.to_string())
        .collect::<BTreeSet<_>>();
    let mut candidates = BTreeSet::new();
    for signature in signatures() {
        let Some((anchor_offset, anchor_value)) = signature.constraints.first().copied() else {
            continue;
        };
        if anchor_value != 0xC7 {
            continue;
        }
        for base in memchr_iter(anchor_value, data) {
            if base < anchor_offset {
                continue;
            }
            let start = base - anchor_offset;
            let valid = signature.constraints.iter().all(|(offset, value)| {
                start
                    .checked_add(*offset)
                    .and_then(|index| data.get(index))
                    .is_some_and(|byte| *byte == *value)
            });
            if !valid {
                continue;
            }
            let mut hex = String::with_capacity(64);
            let mut in_bounds = true;
            for offset in signature.key_offsets {
                let Some(index) = start.checked_add(offset) else {
                    in_bounds = false;
                    break;
                };
                let Some(bytes) = data.get(index..index + 4) else {
                    in_bounds = false;
                    break;
                };
                for byte in bytes {
                    hex.push_str(&format!("{byte:02X}"));
                }
            }
            if !in_bounds
                || hex.len() != 64
                || false_positives.contains(&hex)
                || entropy(&hex) < 3.3
            {
                continue;
            }
            candidates.insert(hex);
        }
    }
    // 高熵候选优先，减少无谓的容器解析。
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        entropy(right)
            .partial_cmp(&entropy(left))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// 计算十六进制字符串的 Shannon 熵（以 2 为底）。
fn entropy(value: &str) -> f64 {
    let mut counts = [0usize; 256];
    for byte in value.bytes() {
        counts[byte as usize] += 1;
    }
    let length = value.len() as f64;
    if length == 0.0 {
        return 0.0;
    }
    counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let probability = *count as f64 / length;
            -probability * probability.log2()
        })
        .sum()
}

/// 从 `.pak` / `.utoc` 中筛选真正加密、可用于校验的容器。
fn encrypted_containers(containers: &[PathBuf]) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    for path in containers {
        let format = ArchiveService::detect(path);
        if !matches!(format, ArchiveFormat::UnrealPak | ArchiveFormat::IoStore) {
            continue;
        }
        if ArchiveService::probe_encryption(path)
            .map(|info| info.encrypted)
            .unwrap_or(false)
        {
            targets.push(path.clone());
        }
    }
    targets
}

/// 从 Shipping.exe 提取并验证游戏 AES 密钥。
///
/// `containers` 可以是游戏目录中全部候选容器；函数只使用实际加密的
/// Unreal 容器校验。`cancel` 为真时尽快停止。
pub fn extract_key(
    shipping_exe: &Path,
    containers: &[PathBuf],
    cancel: Option<&AtomicBool>,
    mut progress: impl FnMut(KeyStage, usize, usize),
) -> KeyExtraction {
    let targets = encrypted_containers(containers);
    if targets.is_empty() {
        return KeyExtraction {
            key: None,
            candidate_count: 0,
            validated_against: None,
            message: Some("没有检测到加密容器，无需提取 AES 密钥".to_string()),
        };
    }
    progress(KeyStage::Analyzing, 0, 1);
    let data = match std::fs::read(shipping_exe) {
        Ok(data) => data,
        Err(error) => {
            return KeyExtraction {
                key: None,
                candidate_count: 0,
                validated_against: None,
                message: Some(format!("无法读取 Shipping.exe：{error}")),
            };
        }
    };
    if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        return cancelled(0);
    }
    let candidates = collect_candidates(&data);
    let candidate_count = candidates.len();
    drop(data);
    progress(KeyStage::Validating, 0, candidate_count);
    for (index, candidate) in candidates.iter().enumerate() {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            return cancelled(candidate_count);
        }
        for target in &targets {
            if matches!(ArchiveService::verify_password(target, candidate), Ok(true)) {
                progress(KeyStage::Done, candidate_count, candidate_count);
                return KeyExtraction {
                    key: Some(Zeroizing::new(candidate.clone())),
                    candidate_count,
                    validated_against: Some(target.clone()),
                    message: None,
                };
            }
        }
        progress(KeyStage::Validating, index + 1, candidate_count);
    }
    KeyExtraction {
        key: None,
        candidate_count,
        validated_against: None,
        message: Some(format!(
            "找到 {candidate_count} 个候选密钥，但都无法通过容器校验；请手动输入或确认游戏版本"
        )),
    }
}

/// 对扫描结果执行密钥提取。
pub fn extract_key_for_game(
    game: &DetectedGame,
    cancel: Option<&AtomicBool>,
    progress: impl FnMut(KeyStage, usize, usize),
) -> KeyExtraction {
    let Some(shipping_exe) = game.shipping_exe.as_deref() else {
        return KeyExtraction {
            key: None,
            candidate_count: 0,
            validated_against: None,
            message: Some(
                "没有找到 Binaries/Win64/*-Win64-Shipping.exe，无法提取 AES 密钥".to_string(),
            ),
        };
    };
    let containers = game
        .containers
        .iter()
        .map(|container| container.path.clone())
        .collect::<Vec<_>>();
    extract_key(shipping_exe, &containers, cancel, progress)
}

/// 用单个容器验证手动输入的密钥。
pub fn verify_container_key(path: &Path, key: &str) -> bool {
    ArchiveService::verify_password(path, key).unwrap_or(false)
}

/// 构造取消结果。
fn cancelled(candidate_count: usize) -> KeyExtraction {
    KeyExtraction {
        key: None,
        candidate_count,
        validated_against: None,
        message: Some("已取消 AES 密钥提取".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 已知的加密 PAK 测试样本与密钥。
    const TEST_KEY: &str = "94d25bc3aeb420e0be914edc9d5435a1eaab5f2864e09e94019ac205b727a7de";

    /// 返回接口测试样本路径。
    fn encrypted_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../lumark-archive/tests/fixtures/unreal/pack_v11_encrypt_encryptindex.pak")
    }

    /// 用第一条签名模式把密钥编码进 exe 字节。
    fn exe_with_key() -> Vec<u8> {
        let pattern = PATTERNS[0].0.split_whitespace().collect::<Vec<_>>();
        let offsets = PATTERNS[0].1;
        let mut bytes = vec![0u8; 64];
        for (index, token) in pattern.iter().enumerate() {
            if *token == "C7" {
                bytes[index] = 0xC7;
            } else if let Ok(value) = u8::from_str_radix(token, 16) {
                bytes[index] = value;
            }
        }
        for (index, offset) in offsets.iter().enumerate() {
            let value = &TEST_KEY[index * 8..index * 8 + 8];
            for byte in 0..4 {
                bytes[offset + byte] =
                    u8::from_str_radix(&value[byte * 2..byte * 2 + 2], 16).unwrap();
            }
        }
        bytes
    }

    /// 验证签名扫描找到候选，并用真实加密 PAK 校验成功。
    #[test]
    fn extracts_and_validates_key_from_exe_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let exe = directory.path().join("Game-Win64-Shipping.exe");
        fs::write(&exe, exe_with_key()).unwrap();
        let fixture = encrypted_fixture();
        let result = extract_key(&exe, std::slice::from_ref(&fixture), None, |_, _, _| {});
        let key = result.key.expect("应找到并验证密钥");
        assert_eq!(key.to_string(), TEST_KEY.to_uppercase());
        assert_eq!(result.validated_against.as_deref(), Some(fixture.as_path()));
    }

    /// 无签名模式时不返回密钥。
    #[test]
    fn missing_signature_returns_no_key() {
        let directory = tempfile::tempdir().unwrap();
        let exe = directory.path().join("Game-Win64-Shipping.exe");
        fs::write(&exe, vec![0u8; 128]).unwrap();
        let result = extract_key(&exe, &[encrypted_fixture()], None, |_, _, _| {});
        assert!(result.key.is_none());
        assert_eq!(result.candidate_count, 0);
        assert!(result.message.is_some());
    }

    /// 熵阈值与手动密钥校验行为。
    #[test]
    fn entropy_and_manual_verification() {
        assert!(entropy("0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF") > 3.3);
        assert!(entropy("0000000000000000000000000000000000000000000000000000000000000000") < 1.0);
        assert!(verify_container_key(&encrypted_fixture(), TEST_KEY));
        assert!(!verify_container_key(&encrypted_fixture(), &"0".repeat(64)));
    }
}
