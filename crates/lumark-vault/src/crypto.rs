//! 密码库文件格式、Argon2 派生和认证加密。

use std::path::Path;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chrono::{DateTime, Utc};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::model::{PasswordEntry, VaultError, VaultSettings};

/// 密码库文件的固定魔数，用于快速识别损坏或错误文件。
const MAGIC: &str = "LUMARKVAULT1";
/// 旧版 AES-256-GCM 文件版本。
const LEGACY_VERSION: u32 = 1;
/// 当前 XChaCha20-Poly1305 文件版本。
const VERSION: u32 = 2;
/// 新文件载荷使用的认证加密算法标识。
const XCHACHA_ALGORITHM: &str = "xchacha20poly1305";
/// 旧版文件载荷使用的认证加密算法标识。
const AES_GCM_ALGORITHM: &str = "aes-256-gcm";
/// 旧版主密钥封装使用的关联数据，防止密文跨版本替换。
const LEGACY_KEY_AAD: &[u8] = b"LUMARKVAULT1:key:v1";
/// 旧版载荷使用的关联数据。
const LEGACY_PAYLOAD_AAD: &[u8] = b"LUMARKVAULT1:payload:v1";
/// 新版主密钥封装使用的关联数据。
const KEY_AAD: &[u8] = b"LUMARKVAULT2:key:v1";
/// 新版载荷使用的关联数据。
const PAYLOAD_AAD: &[u8] = b"LUMARKVAULT2:payload:v1";
/// Argon2 默认内存成本（KiB）。
const ARGON_M_COST: u32 = 64 * 1024;
/// Argon2 默认迭代次数。
const ARGON_T_COST: u32 = 3;
/// Argon2 默认并行度。
const ARGON_P_COST: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 密码库磁盘文件的外层信封，包含版本、密钥封装和加密载荷。
pub struct VaultFile {
    /// 文件魔数。
    pub magic: String,
    /// 格式版本。
    pub version: u32,
    #[serde(default)]
    pub entry_count: u32,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 最后写入时间。
    pub updated_at: DateTime<Utc>,
    /// 密钥封装方式。
    pub key_wrap: KeyWrap,
    /// 加密载荷。
    pub payload: EncryptedBlob,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// 主密钥的两种封装方式：DPAPI 或主密码派生密钥。
pub enum KeyWrap {
    /// 使用 Windows DPAPI 封装会话密钥。
    Dpapi {
        blob: String,
        /// 新文件使用计算机绑定；旧文件缺少该字段时会在打开后自动迁移。
        #[serde(default)]
        machine_bound: bool,
    },
    /// 使用主密码派生密钥封装会话密钥。
    MasterPassword {
        kdf: KdfParams,
        nonce: String,
        ciphertext: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 主密码派生密钥的 Argon2 参数。
pub struct KdfParams {
    /// KDF 算法名称。
    pub algorithm: String,
    /// 随机盐（Base64）。
    pub salt: String,
    /// Argon2 内存成本。
    pub m_cost: u32,
    /// Argon2 迭代次数。
    pub t_cost: u32,
    /// Argon2 并行度。
    pub p_cost: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 带算法标识、随机 nonce 和密文的认证加密数据块。
pub struct EncryptedBlob {
    /// 旧版 1 格式文件此字段为空，当时使用 AES-256-GCM。
    #[serde(default)]
    pub algorithm: String,
    /// 随机 nonce（Base64）。
    pub nonce: String,
    /// 认证密文（Base64）。
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 解密后的密码条目和密码库设置。
struct VaultPayload {
    entries: Vec<PasswordEntry>,
    settings: VaultSettings,
}

#[derive(Debug, Clone)]
/// 一次成功解锁后的内存视图，包含条目、设置和会话密钥。
pub struct DecryptedVault {
    /// 解密后的条目。
    pub entries: Vec<PasswordEntry>,
    /// 解密后的设置。
    pub settings: VaultSettings,
    /// 会话密钥。
    pub key: [u8; 32],
}

impl DecryptedVault {
    /// 拆出条目、设置和会话密钥，并清零原结构中的密钥副本。
    pub fn into_parts(mut self) -> (Vec<PasswordEntry>, VaultSettings, [u8; 32]) {
        let key = self.key;
        self.key.zeroize();
        (self.entries, self.settings, key)
    }
}

/// 密码库文件格式、密钥封装和认证加密的纯函数入口。
pub struct VaultCrypto;

impl VaultCrypto {
    /// 创建使用当前 Windows 用户 DPAPI 绑定的新密码库。
    pub fn new_dpapi(
        entries: Vec<PasswordEntry>,
        settings: VaultSettings,
    ) -> Result<(VaultFile, [u8; 32]), VaultError> {
        let key = random_key();
        let payload = encrypt_payload(&entries, &settings, &key, XCHACHA_ALGORITHM)?;
        let now = Utc::now();
        let file = VaultFile {
            magic: MAGIC.to_string(),
            version: VERSION,
            entry_count: entries.len() as u32,
            created_at: now,
            updated_at: now,
            key_wrap: KeyWrap::Dpapi {
                blob: STANDARD.encode(dpapi_protect(&key)?),
                machine_bound: true,
            },
            payload,
        };
        Ok((file, key))
    }

    /// 创建使用主密码派生密钥保护的新密码库。
    pub fn new_master(
        entries: Vec<PasswordEntry>,
        settings: VaultSettings,
        master_password: &str,
    ) -> Result<(VaultFile, [u8; 32]), VaultError> {
        if master_password.chars().count() < 8 {
            return Err(VaultError::WeakMasterPassword);
        }
        let key = random_key();
        let payload = encrypt_payload(&entries, &settings, &key, XCHACHA_ALGORITHM)?;
        let now = Utc::now();
        let file = VaultFile {
            magic: MAGIC.to_string(),
            version: VERSION,
            entry_count: entries.len() as u32,
            created_at: now,
            updated_at: now,
            key_wrap: wrap_master_key(&key, master_password, VERSION)?,
            payload,
        };
        Ok((file, key))
    }

    /// 校验主密码并解出载荷和会话密钥。
    pub fn unlock(
        file: &VaultFile,
        master_password: Option<&str>,
    ) -> Result<DecryptedVault, VaultError> {
        if file.magic != MAGIC || !matches!(file.version, LEGACY_VERSION | VERSION) {
            return Err(VaultError::UnsupportedFormat);
        }

        let key = match &file.key_wrap {
            KeyWrap::Dpapi { blob, .. } => {
                let blob = STANDARD
                    .decode(blob)
                    .map_err(|_| VaultError::Corrupt("DPAPI blob".into()))?;
                let bytes = dpapi_unprotect(&blob)?;
                let key: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| VaultError::Corrupt("DPAPI key length".into()))?;
                key
            }
            KeyWrap::MasterPassword {
                kdf,
                nonce,
                ciphertext,
            } => {
                let password = master_password.ok_or(VaultError::Locked)?;
                let salt = STANDARD
                    .decode(&kdf.salt)
                    .map_err(|_| VaultError::Corrupt("salt".into()))?;
                let ciphertext = STANDARD
                    .decode(ciphertext)
                    .map_err(|_| VaultError::Corrupt("wrapped key".into()))?;
                let nonce = STANDARD
                    .decode(nonce)
                    .map_err(|_| VaultError::Corrupt("key nonce".into()))?;
                derive_and_open(password, &salt, kdf, &nonce, &ciphertext, file.version)?
            }
        };

        let payload = decrypt_payload(&file.payload, &key, file.version)?;
        Ok(DecryptedVault {
            entries: payload.entries,
            settings: payload.settings,
            key,
        })
    }

    /// 用同一会话密钥重新加密载荷并更新文件时间戳。
    pub fn rewrite_payload(
        file: &mut VaultFile,
        entries: &[PasswordEntry],
        settings: &VaultSettings,
        key: &[u8; 32],
    ) -> Result<(), VaultError> {
        let algorithm = if file.payload.algorithm.is_empty() {
            AES_GCM_ALGORITHM
        } else {
            file.payload.algorithm.as_str()
        };
        file.payload = encrypt_payload(entries, settings, key, algorithm)?;
        file.entry_count = entries.len() as u32;
        file.updated_at = Utc::now();
        Ok(())
    }

    /// 把密码库从 DPAPI 迁移到主密码保护。
    pub fn set_master_password(
        file: &mut VaultFile,
        key: &[u8; 32],
        master_password: &str,
    ) -> Result<(), VaultError> {
        if master_password.chars().count() < 8 {
            return Err(VaultError::WeakMasterPassword);
        }
        file.version = VERSION;
        file.key_wrap = wrap_master_key(key, master_password, VERSION)?;
        file.updated_at = Utc::now();
        Ok(())
    }

    /// 把密码库从主密码迁移到 DPAPI 保护。
    pub fn set_dpapi(file: &mut VaultFile, key: &[u8; 32]) -> Result<(), VaultError> {
        file.version = VERSION;
        file.key_wrap = KeyWrap::Dpapi {
            blob: STANDARD.encode(dpapi_protect(key)?),
            machine_bound: true,
        };
        file.updated_at = Utc::now();
        Ok(())
    }

    /// 只校验主密码，不返回解密后的条目。
    pub fn verify_master_password(file: &VaultFile, password: &str) -> Result<(), VaultError> {
        let KeyWrap::MasterPassword {
            kdf,
            nonce,
            ciphertext,
        } = &file.key_wrap
        else {
            return Err(VaultError::UnsupportedFormat);
        };
        let salt = STANDARD
            .decode(&kdf.salt)
            .map_err(|_| VaultError::Corrupt("salt".into()))?;
        let nonce = STANDARD
            .decode(nonce)
            .map_err(|_| VaultError::Corrupt("nonce".into()))?;
        let ciphertext = STANDARD
            .decode(ciphertext)
            .map_err(|_| VaultError::Corrupt("wrapped key".into()))?;
        derive_and_open(password, &salt, kdf, &nonce, &ciphertext, file.version)?;
        Ok(())
    }

    /// 读取并解析密码库信封，校验魔数和版本。
    pub fn read_file(path: &Path) -> Result<VaultFile, VaultError> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    /// 把密码库信封原子写回磁盘。
    pub fn write_file(path: &Path, file: &VaultFile) -> Result<(), VaultError> {
        let bytes = serde_json::to_vec_pretty(file)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        {
            use std::io::Write;
            let mut output = std::fs::File::create(&temp)?;
            output.write_all(&bytes)?;
            output.sync_all()?;
        }
        let backup = path.with_extension("bak");
        if let Err(error) = atomic_replace(path, &temp, &backup) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(())
    }

    /// 导出可移植的加密密码库副本。
    pub fn export(
        path: &Path,
        entries: &[PasswordEntry],
        settings: &VaultSettings,
        passphrase: &str,
    ) -> Result<(), VaultError> {
        let (file, _) = Self::new_master(entries.to_vec(), *settings, passphrase)?;
        Self::write_file(path, &file)
    }

    /// 校验并导入外部密码库副本。
    pub fn import(
        path: &Path,
        passphrase: &str,
    ) -> Result<(Vec<PasswordEntry>, VaultSettings), VaultError> {
        let file = Self::read_file(path)?;
        let decrypted = Self::unlock(&file, Some(passphrase)).map_err(|error| match error {
            VaultError::InvalidMasterPassword => VaultError::InvalidExportPassword,
            other => other,
        })?;
        let (entries, settings, _) = decrypted.into_parts();
        Ok((entries, settings))
    }
}

/// 生成 256 位会话密钥。
fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// 生成 AES-256-GCM 的 96 位 nonce。
fn random_aes_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// 生成 XChaCha20-Poly1305 的 192 位 nonce。
fn random_xnonce() -> [u8; 24] {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// 序列化并认证加密密码库载荷。
fn encrypt_payload(
    entries: &[PasswordEntry],
    settings: &VaultSettings,
    key: &[u8; 32],
    algorithm: &str,
) -> Result<EncryptedBlob, VaultError> {
    let payload = VaultPayload {
        entries: entries.to_vec(),
        settings: *settings,
    };
    let mut plaintext = serde_json::to_vec(&payload)?;
    let (nonce, ciphertext) = if algorithm == AES_GCM_ALGORITHM {
        let nonce = random_aes_nonce();
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| VaultError::Crypto)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: payload_aad(LEGACY_VERSION),
                },
            )
            .map_err(|_| VaultError::Crypto)?;
        (nonce.to_vec(), ciphertext)
    } else {
        let nonce = random_xnonce();
        let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::Crypto)?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: payload_aad(VERSION),
                },
            )
            .map_err(|_| VaultError::Crypto)?;
        (nonce.to_vec(), ciphertext)
    };
    plaintext.zeroize();
    Ok(EncryptedBlob {
        algorithm: algorithm.to_string(),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
}

/// 认证解密载荷；失败时统一映射为密码错误或损坏。
fn decrypt_payload(
    blob: &EncryptedBlob,
    key: &[u8; 32],
    file_version: u32,
) -> Result<VaultPayload, VaultError> {
    let nonce = STANDARD
        .decode(&blob.nonce)
        .map_err(|_| VaultError::Corrupt("payload nonce".into()))?;
    let ciphertext = STANDARD
        .decode(&blob.ciphertext)
        .map_err(|_| VaultError::Corrupt("payload".into()))?;
    let algorithm = if !blob.algorithm.is_empty() {
        blob.algorithm.as_str()
    } else if file_version == LEGACY_VERSION {
        AES_GCM_ALGORITHM
    } else {
        XCHACHA_ALGORITHM
    };
    let mut plaintext = if algorithm == AES_GCM_ALGORITHM {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| VaultError::Crypto)?;
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: payload_aad(LEGACY_VERSION),
                },
            )
            .map_err(|_| VaultError::Corrupt("payload authentication".into()))?
    } else {
        let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::Crypto)?;
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: payload_aad(file_version),
                },
            )
            .map_err(|_| VaultError::Corrupt("payload authentication".into()))?
    };
    let payload =
        serde_json::from_slice(&plaintext).map_err(|_| VaultError::Corrupt("payload json".into()));
    plaintext.zeroize();
    payload
}

/// 用主密码派生密钥封装会话密钥。
fn wrap_master_key(key: &[u8; 32], password: &str, version: u32) -> Result<KeyWrap, VaultError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let kdf = KdfParams {
        algorithm: "argon2id".to_string(),
        salt: STANDARD.encode(salt),
        m_cost: ARGON_M_COST,
        t_cost: ARGON_T_COST,
        p_cost: ARGON_P_COST,
    };
    let derived = derive_key(password, &salt, &kdf)?;
    let (nonce, ciphertext) = if version == LEGACY_VERSION {
        let nonce = random_aes_nonce();
        let cipher = Aes256Gcm::new_from_slice(&derived).map_err(|_| VaultError::Crypto)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: key,
                    aad: key_aad(LEGACY_VERSION),
                },
            )
            .map_err(|_| VaultError::Crypto)?;
        (nonce.to_vec(), ciphertext)
    } else {
        let nonce = random_xnonce();
        let cipher = XChaCha20Poly1305::new_from_slice(&derived).map_err(|_| VaultError::Crypto)?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: key,
                    aad: key_aad(VERSION),
                },
            )
            .map_err(|_| VaultError::Crypto)?;
        (nonce.to_vec(), ciphertext)
    };
    let mut derived_cleanup = derived;
    derived_cleanup.zeroize();
    Ok(KeyWrap::MasterPassword {
        kdf,
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
}

/// 按 KDF 参数派生密钥并解开会话密钥。
fn derive_and_open(
    password: &str,
    salt: &[u8],
    kdf: &KdfParams,
    nonce: &[u8],
    ciphertext: &[u8],
    version: u32,
) -> Result<[u8; 32], VaultError> {
    let mut derived = derive_key(password, salt, kdf)?;
    let opened = if version == LEGACY_VERSION {
        let cipher = Aes256Gcm::new_from_slice(&derived).map_err(|_| VaultError::Crypto)?;
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: key_aad(LEGACY_VERSION),
                },
            )
            .map_err(|_| VaultError::InvalidMasterPassword)?
    } else {
        let cipher = XChaCha20Poly1305::new_from_slice(&derived).map_err(|_| VaultError::Crypto)?;
        cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: key_aad(VERSION),
                },
            )
            .map_err(|_| VaultError::InvalidMasterPassword)?
    };
    derived.zeroize();
    opened
        .try_into()
        .map_err(|_| VaultError::Corrupt("master key length".into()))
}

/// 按文件版本选择载荷关联数据。
fn payload_aad(version: u32) -> &'static [u8] {
    if version == LEGACY_VERSION {
        LEGACY_PAYLOAD_AAD
    } else {
        PAYLOAD_AAD
    }
}

/// 按文件版本选择密钥封装关联数据。
fn key_aad(version: u32) -> &'static [u8] {
    if version == LEGACY_VERSION {
        LEGACY_KEY_AAD
    } else {
        KEY_AAD
    }
}

#[cfg(windows)]
/// 原子替换密码库文件，并按需保留备份。
fn atomic_replace(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    if !path.exists() {
        return std::fs::rename(temp, path);
    }
    let path_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let temp_wide: Vec<u16> = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let backup_wide: Vec<u16> = backup
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let ok = unsafe {
        ReplaceFileW(
            path_wide.as_ptr(),
            temp_wide.as_ptr(),
            backup_wide.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
/// 原子替换密码库文件，并按需保留备份。
fn atomic_replace(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::copy(path, backup)?;
    }
    std::fs::rename(temp, path)
}

/// 按 Argon2 参数派生 256 位密钥。
fn derive_key(password: &str, salt: &[u8], kdf: &KdfParams) -> Result<[u8; 32], VaultError> {
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(32))
        .map_err(|_| VaultError::Corrupt("argon2 params".into()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| VaultError::Crypto)?;
    Ok(key)
}

#[cfg(windows)]
/// 用当前 Windows 用户 DPAPI 保护会话密钥。
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let _guard = dpapi_lock()
        .lock()
        .map_err(|_| VaultError::Dpapi("DPAPI lock poisoned".into()))?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE,
            &mut output,
        );
        if ok == 0 {
            return Err(VaultError::Dpapi(format!(
                "CryptProtectData failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(bytes)
    }
}

#[cfg(not(windows))]
/// 用当前 Windows 用户 DPAPI 保护会话密钥。
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, VaultError> {
    Err(VaultError::Dpapi("only supported on Windows".into()))
}

#[cfg(windows)]
/// 用当前 Windows 用户 DPAPI 解开会话密钥。
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

    let _guard = dpapi_lock()
        .lock()
        .map_err(|_| VaultError::Dpapi("DPAPI lock poisoned".into()))?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        );
        if ok == 0 {
            return Err(VaultError::Dpapi(format!(
                "CryptUnprotectData failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(bytes)
    }
}

#[cfg(not(windows))]
/// 用当前 Windows 用户 DPAPI 解开会话密钥。
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>, VaultError> {
    Err(VaultError::Dpapi("only supported on Windows".into()))
}

#[cfg(windows)]
/// 串行化 DPAPI 调用，避免并发 CryptProtect/CryptUnprotect 竞争。
fn dpapi_lock() -> &'static std::sync::Mutex<()> {
    /// DPAPI 调用互斥锁。
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
