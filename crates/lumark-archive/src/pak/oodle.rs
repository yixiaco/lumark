//! Oodle 解压桥接：优先使用纯 Rust 解码器，动态库作为兼容回退。
//!
//! Lumark 支持两种后端：
//! - `rust`：`oozextract` 纯 Rust 解码器，无外部 DLL，Windows/Linux/macOS 均可用；
//! - `dll`：加载用户或随包提供的 Oodle 动态库（仅 Windows）。
//!
//! 后端可通过 `LUMARK_OODLE_BACKEND=auto|rust|dll` 选择，默认 `auto`：先尝试
//! Rust 后端，失败后再回退 DLL。DLL 的查找顺序保持原行为：显式配置路径、
//! `LUMARK_OODLE_DLL`、可执行文件同目录；不会扫描归档所在目录，避免被恶意
//! PAK 旁路加载。

/// Oodle 解压后端选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// 先 Rust 后 DLL。
    Auto,
    /// 仅使用纯 Rust 解码器。
    Rust,
    /// 仅使用动态库。
    Dll,
}

/// 解析后端环境变量；非法或缺失时回落到 `auto`。
fn parse_backend(value: Option<&str>) -> Backend {
    match value.map(str::trim).map(str::to_ascii_lowercase) {
        Some(value) if value == "rust" => Backend::Rust,
        Some(value) if value == "dll" => Backend::Dll,
        _ => Backend::Auto,
    }
}

/// 当前配置的后端名称；用于诊断和界面展示。
pub fn oodle_backend_setting() -> &'static str {
    match parse_backend(std::env::var("LUMARK_OODLE_BACKEND").ok().as_deref()) {
        Backend::Auto => "auto",
        Backend::Rust => "rust",
        Backend::Dll => "dll",
    }
}

/// Oodle 后端状态快照（O-1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OodleStatus {
    /// 环境变量配置：`auto`/`rust`/`dll`。
    pub setting: &'static str,
    /// 纯 Rust 解码器是否可用（当前始终可用）。
    pub rust_available: bool,
    /// 当前平台是否支持动态库后端。
    pub dll_supported: bool,
    /// 显式配置的 DLL 路径。
    pub dll_configured: Option<std::path::PathBuf>,
    /// DLL 是否已成功加载。
    pub dll_loaded: bool,
}

impl OodleStatus {
    /// 当前配置下是否存在可用解码后端。
    pub fn usable(&self) -> bool {
        match self.setting {
            "dll" => self.dll_supported && self.dll_loaded,
            _ => self.rust_available || (self.dll_supported && self.dll_loaded),
        }
    }

    /// 界面展示用简短描述。
    pub fn summary(&self) -> String {
        let backend = if self.usable() {
            if self.setting == "dll" && self.dll_loaded {
                "DLL"
            } else if self.rust_available && self.setting != "dll" {
                "Rust"
            } else {
                "DLL"
            }
        } else {
            "不可用"
        };
        format!(
            "{}（配置 {}；Rust {}；DLL {}）",
            backend,
            self.setting,
            if self.rust_available {
                "可用"
            } else {
                "不可用"
            },
            if self.dll_loaded {
                "已加载"
            } else if self.dll_supported {
                "未加载"
            } else {
                "不支持"
            }
        )
    }
}

/// 返回当前 Oodle 后端状态。
pub fn oodle_status() -> OodleStatus {
    let (dll_configured, dll_loaded, dll_supported) = dll::status();
    OodleStatus {
        setting: oodle_backend_setting(),
        rust_available: true,
        dll_supported,
        dll_configured,
        dll_loaded,
    }
}

/// 解压一个 Oodle 压缩块。
pub(crate) fn decompress(input: &[u8], output: &mut [u8]) -> Result<(), String> {
    match parse_backend(std::env::var("LUMARK_OODLE_BACKEND").ok().as_deref()) {
        Backend::Rust => decompress_rust(input, output),
        Backend::Dll => dll::decompress(input, output),
        Backend::Auto => match decompress_rust(input, output) {
            Ok(()) => Ok(()),
            Err(rust_error) => dll::decompress(input, output).map_err(|dll_error| {
                format!("Oodle 解压失败：Rust 后端（{rust_error}）；DLL 后端（{dll_error}）")
            }),
        },
    }
}

/// 纯 Rust Oodle 解码；输出缓冲必须恰好等于解压后大小。
fn decompress_rust(input: &[u8], output: &mut [u8]) -> Result<(), String> {
    if output.is_empty() {
        return Ok(());
    }
    let written = oozextract::Extractor::new()
        .read_from_slice(input, output)
        .map_err(|error| format!("{error}"))?;
    if written != output.len() {
        return Err(format!(
            "Rust 解码输出长度不匹配：期望 {} 字节，实际 {written} 字节",
            output.len()
        ));
    }
    Ok(())
}

#[cfg(windows)]
mod dll {
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
    pub(super) fn decompress(input: &[u8], output: &mut [u8]) -> Result<(), String> {
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
    pub(super) fn set_library_path(path: Option<PathBuf>) -> Result<(), String> {
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

    /// 返回 DLL 配置/加载状态。
    pub(super) fn status() -> (Option<PathBuf>, bool, bool) {
        let Some(state) = OODLE.get() else {
            return (None, false, true);
        };
        match state.lock() {
            Ok(state) => (state.configured.clone(), state.library.is_some(), true),
            Err(_) => (None, false, true),
        }
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
                "未找到可用的 Oodle DLL（可在游戏资产设置中选择，或设置 LUMARK_OODLE_DLL）"
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
mod dll {
    use std::path::PathBuf;

    /// 非 Windows 平台没有动态库后端；由纯 Rust 后端负责解压。
    pub(super) fn decompress(_input: &[u8], _output: &mut [u8]) -> Result<(), String> {
        Err("当前平台没有可用的 Oodle DLL，请使用 Rust 后端".to_string())
    }

    /// 非 Windows 平台不接受显式 DLL 路径。
    pub(super) fn set_library_path(path: Option<PathBuf>) -> Result<(), String> {
        if path.is_some() {
            Err("Oodle DLL 仅支持 Windows；当前平台使用纯 Rust 解码器".to_string())
        } else {
            Ok(())
        }
    }

    /// 非 Windows 平台没有 DLL 状态。
    pub(super) fn status() -> (Option<PathBuf>, bool, bool) {
        (None, false, false)
    }
}

/// 配置 Oodle 动态库路径；`None` 表示恢复自动查找。
///
/// 游戏目录自带的 `oo2core*.dll` 或 FModel/UE 安装中的
/// `oodle-data-shared.dll` 均可由上层传入，Lumark 只负责加载和校验符号。
/// 配置失败不会影响纯 Rust 后端。
pub fn configure_oodle_library(path: Option<std::path::PathBuf>) -> Result<(), String> {
    dll::set_library_path(path)
}

#[cfg(test)]
mod tests {
    use super::{Backend, decompress_rust, parse_backend};

    /// 验证后端配置解析：大小写、空白与非法值。
    #[test]
    fn parses_backend_preference() {
        assert_eq!(parse_backend(None), Backend::Auto);
        assert_eq!(parse_backend(Some("")), Backend::Auto);
        assert_eq!(parse_backend(Some("auto")), Backend::Auto);
        assert_eq!(parse_backend(Some(" Rust ")), Backend::Rust);
        assert_eq!(parse_backend(Some("DLL")), Backend::Dll);
        assert_eq!(parse_backend(Some("unknown")), Backend::Auto);
    }

    /// Rust 后端应能解码未压缩块：块头 0x4C（低半字节 0xC、未压缩位）
    /// 加解码器类型 0x06（Kraken），随后是原样字节。
    #[test]
    fn rust_backend_decodes_uncompressed_block() {
        let input = [0x4C, 0x06, b'l', b'u', b'm', b'a', b'r', b'k'];
        let mut output = [0u8; 6];
        decompress_rust(&input, &mut output).expect("未压缩块应解码成功");
        assert_eq!(&output, b"lumark");
    }

    /// 非法块头应返回错误而不是 panic。
    #[test]
    fn rust_backend_rejects_invalid_header() {
        let input = [0x00, 0x00, 0x01];
        let mut output = [0u8; 4];
        assert!(decompress_rust(&input, &mut output).is_err());
    }

    /// 输出长度为 0 时直接成功，不读取输入。
    #[test]
    fn rust_backend_accepts_empty_output() {
        let mut output = [];
        decompress_rust(&[0xFF], &mut output).expect("空输出应直接成功");
    }

    /// 后端状态快照报告 Rust 可用且可解码。
    #[test]
    fn status_reports_available_backend() {
        let status = super::oodle_status();
        assert!(status.rust_available);
        assert!(status.usable(), "默认配置应至少有一个可用后端");
        assert!(status.summary().contains("配置"));
        assert!(matches!(status.setting, "auto" | "rust" | "dll"));
    }

    /// 仅 DLL 模式且 DLL 未加载时不可用。
    #[test]
    fn dll_only_status_requires_loaded_dll() {
        let status = super::OodleStatus {
            setting: "dll",
            rust_available: false,
            dll_supported: true,
            dll_configured: None,
            dll_loaded: false,
        };
        assert!(!status.usable());
        assert!(status.summary().contains("不可用"));
    }
}
