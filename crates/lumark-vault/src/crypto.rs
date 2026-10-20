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

const MAGIC: &str = "LUMARKVAULT1";
const LEGACY_VERSION: u32 = 1;
const VERSION: u32 = 2;
const XCHACHA_ALGORITHM: &str = "xchacha20poly1305";
const AES_GCM_ALGORITHM: &str = "aes-256-gcm";
const LEGACY_KEY_AAD: &[u8] = b"LUMARKVAULT1:key:v1";
const LEGACY_PAYLOAD_AAD: &[u8] = b"LUMARKVAULT1:payload:v1";
const KEY_AAD: &[u8] = b"LUMARKVAULT2:key:v1";
const PAYLOAD_AAD: &[u8] = b"LUMARKVAULT2:payload:v1";
const ARGON_M_COST: u32 = 64 * 1024;
const ARGON_T_COST: u32 = 3;
const ARGON_P_COST: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultFile {
    pub magic: String,
    pub version: u32,
    #[serde(default)]
    pub entry_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub key_wrap: KeyWrap,
    pub payload: EncryptedBlob,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyWrap {
    Dpapi {
        blob: String,
    },
    MasterPassword {
        kdf: KdfParams,
        nonce: String,
        ciphertext: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    pub salt: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// Empty for legacy version-1 files, which used AES-256-GCM.
    #[serde(default)]
    pub algorithm: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultPayload {
    entries: Vec<PasswordEntry>,
    settings: VaultSettings,
}

#[derive(Debug, Clone)]
pub struct DecryptedVault {
    pub entries: Vec<PasswordEntry>,
    pub settings: VaultSettings,
    pub key: [u8; 32],
}

impl DecryptedVault {
    pub fn into_parts(mut self) -> (Vec<PasswordEntry>, VaultSettings, [u8; 32]) {
        let key = self.key;
        self.key.zeroize();
        (self.entries, self.settings, key)
    }
}

pub struct VaultCrypto;

impl VaultCrypto {
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
            },
            payload,
        };
        Ok((file, key))
    }

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

    pub fn unlock(
        file: &VaultFile,
        master_password: Option<&str>,
    ) -> Result<DecryptedVault, VaultError> {
        if file.magic != MAGIC || !matches!(file.version, LEGACY_VERSION | VERSION) {
            return Err(VaultError::UnsupportedFormat);
        }

        let key = match &file.key_wrap {
            KeyWrap::Dpapi { blob } => {
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

    pub fn set_dpapi(file: &mut VaultFile, key: &[u8; 32]) -> Result<(), VaultError> {
        file.version = VERSION;
        file.key_wrap = KeyWrap::Dpapi {
            blob: STANDARD.encode(dpapi_protect(key)?),
        };
        file.updated_at = Utc::now();
        Ok(())
    }

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

    pub fn read_file(path: &Path) -> Result<VaultFile, VaultError> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

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

    pub fn export(
        path: &Path,
        entries: &[PasswordEntry],
        settings: &VaultSettings,
        passphrase: &str,
    ) -> Result<(), VaultError> {
        let (file, _) = Self::new_master(entries.to_vec(), *settings, passphrase)?;
        Self::write_file(path, &file)
    }

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

fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

fn random_aes_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

fn random_xnonce() -> [u8; 24] {
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

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

fn payload_aad(version: u32) -> &'static [u8] {
    if version == LEGACY_VERSION {
        LEGACY_PAYLOAD_AAD
    } else {
        PAYLOAD_AAD
    }
}

fn key_aad(version: u32) -> &'static [u8] {
    if version == LEGACY_VERSION {
        LEGACY_KEY_AAD
    } else {
        KEY_AAD
    }
}

#[cfg(windows)]
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
fn atomic_replace(path: &Path, temp: &Path, backup: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::copy(path, backup)?;
    }
    std::fs::rename(temp, path)
}

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
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
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
            CRYPTPROTECT_UI_FORBIDDEN,
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
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, VaultError> {
    Err(VaultError::Dpapi("only supported on Windows".into()))
}

#[cfg(windows)]
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
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>, VaultError> {
    Err(VaultError::Dpapi("only supported on Windows".into()))
}

#[cfg(windows)]
fn dpapi_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}
