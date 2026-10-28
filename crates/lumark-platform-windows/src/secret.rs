//! 用 Windows DPAPI 保护短文本凭据。
//!
//! 只用于 SMTP 密码等少量设置；密文绑定当前 Windows 用户，复制到其他账户
//! 无法解密。非 Windows 平台直接返回错误，不提供明文回退。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// 加密一段 UTF-8 文本并返回 Base64 密文。
#[cfg(windows)]
pub fn protect_text(plain: &str) -> Result<String, crate::PlatformError> {
    let encrypted = protect(plain.as_bytes())?;
    Ok(STANDARD.encode(encrypted))
}

/// 解密 Base64 密文并返回 UTF-8 文本。
#[cfg(windows)]
pub fn unprotect_text(blob: &str) -> Result<String, crate::PlatformError> {
    let encrypted = STANDARD
        .decode(blob)
        .map_err(|error| crate::PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    let plain = unprotect(&encrypted)?;
    String::from_utf8(plain)
        .map_err(|error| crate::PlatformError::Windows(std::io::Error::other(error.to_string())))
}

/// 非 Windows 平台没有 DPAPI。
#[cfg(not(windows))]
pub fn protect_text(_plain: &str) -> Result<String, crate::PlatformError> {
    Err(crate::PlatformError::Windows(std::io::Error::other(
        "DPAPI only supported on Windows",
    )))
}

/// 非 Windows 平台没有 DPAPI。
#[cfg(not(windows))]
pub fn unprotect_text(_blob: &str) -> Result<String, crate::PlatformError> {
    Err(crate::PlatformError::Windows(std::io::Error::other(
        "DPAPI only supported on Windows",
    )))
}

#[cfg(windows)]
/// 调用 DPAPI 保护字节。
fn protect(data: &[u8]) -> Result<Vec<u8>, crate::PlatformError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

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
            return Err(crate::PlatformError::Windows(
                std::io::Error::last_os_error(),
            ));
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(bytes)
    }
}

#[cfg(windows)]
/// 调用 DPAPI 解密字节。
fn unprotect(data: &[u8]) -> Result<Vec<u8>, crate::PlatformError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

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
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(crate::PlatformError::Windows(
                std::io::Error::last_os_error(),
            ));
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 DPAPI 文本往返和错误密文拒绝。
    #[cfg(windows)]
    #[test]
    fn text_round_trips_and_rejects_corruption() {
        let blob = protect_text("smtp-password").unwrap();
        assert_ne!(blob, "smtp-password");
        assert_eq!(unprotect_text(&blob).unwrap(), "smtp-password");
        assert!(unprotect_text("not-base64").is_err());
    }
}
