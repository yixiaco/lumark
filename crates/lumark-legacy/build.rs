//! 为本地传统适配层补充 Windows 系统库链接。
//!
//! `unrar_sys` 的实现使用 `OpenProcessToken` 和 `Crypt*`，但其构建脚本
//! 没有在 Windows 上声明 `advapi32`；这里显式补上，避免测试或应用链接失败。

fn main() {
    #[cfg(windows)]
    println!("cargo:rustc-link-lib=advapi32");
}
