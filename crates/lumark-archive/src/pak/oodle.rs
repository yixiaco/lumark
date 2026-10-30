//! Oodle 解压桥接：只加载用户提供的 Oodle 动态库。
//!
//! 出于许可和分发考虑，Lumark 不捆绑 Oodle。加载顺序固定为显式配置的
//! 路径、`LUMARK_OODLE_DLL`，然后是可执行文件同目录下的 Oodle DLL；
//! 不会扫描归档所在目录，避免被恶意 PAK 旁路加载。非 Windows 平台
//! 不提供 Oodle 支持，压缩块会返回可操作的错误。

#[cfg(windows)]
mod platform {
    use std::ffi::CString;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    /// `OodleLZ_Decompress` 的函数指针。
    type OodleDecompress = unsafe extern "system" fn(
        comp_buf: *const u8,
        comp_buf_size: isize,
        raw_buf: *mut u8,
        raw_len: isize,
        fuzz_safe: i32,
        check_crc: i32,
        verbosity: i32,
        dec_buf_base: isize,
        dec_buf_size: isize,
        fp_callback: usize,
        callback_user_data: usize,
        decoder_memory: *mut u8,
        decoder_memory_size: isize,
        thread_phase: i32,
    ) -> isize;

    /// 已加载的 Oodle 动态库。
    struct OodleLibrary {
        module: *mut core::ffi::c_void,
        decompress: OodleDecompress,
    }

    // 函数指针和 HMODULE 在 Windows 上可跨线程使用；加载后只读。
    unsafe impl Send for OodleLibrary {}
    unsafe impl Sync for OodleLibrary {}

    impl Drop for OodleLibrary {
        fn drop(&mut self) {
            if !self.module.is_null() {
                unsafe {
                    FreeLibrary(self.module);
                }
            }
        }
    }

    /// 进程内加载状态；显式配置路径后允许重新加载。
    struct OodleState {
        configured: Option<PathBuf>,
        library: Option<OodleLibrary>,
        failed: bool,
    }

    static OODLE: OnceLock<Mutex<OodleState>> = OnceLock::new();

    /// 解压一个 Oodle 压缩块。
    pub(crate) fn decompress(input: &[u8], output: &mut [u8]) -> Result<(), String> {
        let decompress = decompress_pointer()?;
        let input_len = isize::try_from(input.len()).map_err(|_| "Oodle 输入过大".to_string())?;
        let output_len = isize::try_from(output.len()).map_err(|_| "Oodle 输出过大".to_string())?;
        let written = unsafe {
            decompress(
                input.as_ptr(),
                input_len,
                output.as_mut_ptr(),
                output_len,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                0,
                3,
            )
        };
        if written <= 0 || written as usize != output.len() {
            return Err(format!(
                "Oodle 解压失败：期望 {} 字节，返回 {written}",
                output.len()
            ));
        }
        Ok(())
    }

    /// 设置或清除 Oodle DLL 路径；设置时立即尝试加载并验证导出符号。
    pub(crate) fn set_library_path(path: Option<PathBuf>) -> Result<(), String> {
        let state = OODLE.get_or_init(|| {
            Mutex::new(OodleState {
                configured: None,
                library: None,
                failed: false,
            })
        });
        let mut state = state
            .lock()
            .map_err(|_| "Oodle 加载状态已损坏".to_string())?;
        state.library = None;
        state.failed = false;
        state.configured = path.clone();
        if let Some(path) = path {
            let library = load_one(&path).ok_or_else(|| {
                format!(
                    "无法加载 Oodle 库或缺少 OodleLZ_Decompress：{}",
                    path.display()
                )
            })?;
            state.library = Some(library);
        }
        Ok(())
    }

    /// 返回已加载的函数指针；首次调用时按候选顺序加载。
    fn decompress_pointer() -> Result<OodleDecompress, String> {
        let state = OODLE.get_or_init(|| {
            Mutex::new(OodleState {
                configured: None,
                library: None,
                failed: false,
            })
        });
        let mut state = state
            .lock()
            .map_err(|_| "Oodle 加载状态已损坏".to_string())?;
        if state.library.is_none() && !state.failed {
            if let Some(library) = load_library(state.configured.as_deref()) {
                state.library = Some(library);
            } else {
                state.failed = true;
            }
        }
        state
            .library
            .as_ref()
            .map(|library| library.decompress)
            .ok_or_else(|| {
                "Oodle 压缩数据需要用户提供 Oodle 库（在游戏资产设置中选择，或设置 LUMARK_OODLE_DLL）"
                    .to_string()
            })
    }

    /// 按固定顺序查找并加载 DLL。
    fn load_library(configured: Option<&Path>) -> Option<OodleLibrary> {
        let mut candidates = Vec::new();
        if let Some(path) = configured {
            candidates.push(path.to_path_buf());
        }
        if let Ok(path) = std::env::var("LUMARK_OODLE_DLL")
            && !path.trim().is_empty()
        {
            candidates.push(PathBuf::from(path));
        }
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
            && let Ok(entries) = std::fs::read_dir(directory)
        {
            let mut local: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| is_oodle_file_name(path))
                .collect();
            local.sort();
            candidates.extend(local);
        }
        for path in candidates {
            if let Some(library) = load_one(&path) {
                return Some(library);
            }
        }
        None
    }

    /// 判断文件名是否是 Oodle 动态库。
    fn is_oodle_file_name(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_ascii_lowercase())
                .is_some_and(|name| name.starts_with("oo2core") || name.starts_with("oodle"))
    }

    /// 加载单个 DLL 并取函数地址。
    fn load_one(path: &Path) -> Option<OodleLibrary> {
        if !path.is_file() {
            return None;
        }
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return None;
        }
        let name = CString::new("OodleLZ_Decompress").ok()?;
        let address = unsafe { GetProcAddress(module, name.as_ptr() as *const u8) };
        let Some(address) = address else {
            unsafe {
                FreeLibrary(module);
            }
            return None;
        };
        // SAFETY: 地址来自用户明确提供的 Oodle DLL，签名与公开的
        // `OodleLZ_Decompress` 一致；调用方保证缓冲区长度正确。
        let decompress: OodleDecompress = unsafe { std::mem::transmute(address) };
        Some(OodleLibrary { module, decompress })
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    /// 非 Windows 平台没有可用的 Oodle 动态库加载路径。
    pub(crate) fn decompress(_input: &[u8], _output: &mut [u8]) -> Result<(), String> {
        Err("Oodle 解压仅支持 Windows".to_string())
    }

    /// 非 Windows 平台拒绝配置 Oodle 库。
    pub(crate) fn set_library_path(_path: Option<PathBuf>) -> Result<(), String> {
        Err("Oodle 解压仅支持 Windows".to_string())
    }
}

pub(crate) use platform::decompress;

/// 配置 Oodle 动态库路径；`None` 表示恢复自动查找。
///
/// 游戏目录自带的 `oo2core*.dll` 或 FModel/UE 安装中的
/// `oodle-data-shared.dll` 均可由上层传入，Lumark 只负责加载和校验符号。
pub fn configure_oodle_library(path: Option<std::path::PathBuf>) -> Result<(), String> {
    platform::set_library_path(path)
}
