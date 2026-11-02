use std::env;

fn rustflags_request_native() -> bool {
    fn iter_flags(var: &str) -> Vec<String> {
        env::var(var)
            .ok()
            .map(|v| {
                if var == "CARGO_ENCODED_RUSTFLAGS" {
                    v.split('\x1f')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    v.split_whitespace().map(|s| s.to_string()).collect()
                }
            })
            .unwrap_or_default()
    }

    let mut flags = iter_flags("CARGO_ENCODED_RUSTFLAGS");
    flags.extend(iter_flags("RUSTFLAGS"));

    for (i, flag) in flags.iter().enumerate() {
        if flag == "-Ctarget-cpu=native" {
            return true;
        }

        if flag == "-C"
            && let Some(next) = flags.get(i + 1)
            && next == "target-cpu=native"
        {
            return true;
        }
    }

    false
}

fn main() {
    println!("cargo:rerun-if-changed=zpaq/libzpaq.cpp");
    println!("cargo:rerun-if-changed=zpaq/libzpaq.h");
    println!("cargo:rerun-if-changed=zpaq_rs_ffi.cpp");

    let mut build = cc::Build::new();

    // Toolchain selection:
    // - 只有显式设置 `ZPAQ_RS_CXX` 时才切换编译器，其余情况交给 cc 默认选择（Windows 上为 MSVC cl.exe）。
    // - 不再自动探测/优先 clang++：clang 在 Windows 目标上默认使用静态 CRT（/MT），
    //   与 rustc 默认的 /MD 混链会触发 LNK2038 `RuntimeLibrary` 不匹配。
    // - 若显式指定 clang 驱动，则强制动态 CRT，与 rustc 保持一致。
    println!("cargo:rerun-if-env-changed=ZPAQ_RS_CXX");
    let cxx_hint = env::var("ZPAQ_RS_CXX")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("CXX")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    let mut clang_driver = false;
    if let Ok(cxx) = env::var("ZPAQ_RS_CXX") {
        let cxx = cxx.trim();
        if !cxx.is_empty() {
            let lower = cxx.to_ascii_lowercase();
            clang_driver = lower.contains("clang") && !lower.contains("clang-cl");
            build.compiler(cxx);
        }
    }
    if !clang_driver && let Some(cxx) = cxx_hint {
        let lower = cxx.to_ascii_lowercase();
        clang_driver = lower.contains("clang") && !lower.contains("clang-cl");
    }

    build
        .cpp(true)
        .include("zpaq")
        .file("zpaq/libzpaq.cpp")
        .file("zpaq/zpaq.cpp")
        .file("zpaq_rs_ffi.cpp")
        // zpaq.cpp contains a `main()` (or `wmain()` on Windows). Rename it so it can be linked into this library.
        .define("main", "zpaq_cli_main")
        .define("wmain", "zpaq_cli_main")
        .flag_if_supported("-std=c++17")
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-fPIC")
        .flag_if_supported("-pthread")
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-null-pointer-subtraction")
        .flag_if_supported("-Wno-unused-const-variable")
        .define("NDEBUG", None);

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    // Windows MSVC 目标必须与 rustc 使用同一 CRT。cc 对 MSVC 家族按 static_crt
    // 传 -MT/-MD；显式关闭静态 CRT 以对齐 rustc 的 /MD。若显式使用 clang 驱动，
    // 再追加 clang 的动态运行库开关（不支持的驱动会由 flag_if_supported 丢弃）。
    if target_os == "windows" && target_env == "msvc" {
        build.static_crt(false);
        if clang_driver {
            build.flag_if_supported("-fms-runtime-lib=dynamic");
        }
    }

    // Only define unix on UNIX systems (not on Windows)
    #[cfg(unix)]
    build.define("unix", None);

    // libzpaq JIT is x86_64-specific. Force NOJIT on non-x86_64 targets,
    // while still allowing explicit `--features nojit` on x86_64.
    if env::var_os("CARGO_FEATURE_NOJIT").is_some() || target_arch != "x86_64" {
        build.define("NOJIT", None);
    }

    // Always optimize zpaq
    build.flag_if_supported("-O3");

    // Keep C++ codegen aligned with Rust when native tuning is explicitly requested.
    if rustflags_request_native() {
        match target_arch.as_str() {
            "x86" | "x86_64" => {
                build.flag_if_supported("-march=native");
            }
            "arm" | "aarch64" => {
                build.flag_if_supported("-mcpu=native");
            }
            _ => {}
        }
    }

    // Try to enable LTO for the C++ objects in release-like profiles.
    // Cross-language LTO (Rust <-> C++) is toolchain-dependent; this at least
    // enables LTO within the C++ compilation unit(s) when supported.
    // Notes:
    // - On Windows with clang++ + MSVC linker, -flto produces LLVM IR
    //   which lib.exe can't handle, so we skip LTO on Windows.
    // - On NetBSD, archive/link toolchains commonly miss the LTO plugin for
    //   C++ objects, which can drop symbols from libzpaq_rs_ffi.a. Disable
    //   C++-side LTO there to preserve reliable linking.
    let profile = env::var("PROFILE").unwrap_or_default();
    if (profile == "release" || profile == "bench")
        && target_os != "windows"
        && target_os != "netbsd"
    {
        build.flag_if_supported("-flto");
    }

    build.compile("zpaq_rs_ffi");

    // On Windows, zpaq needs advapi32 for CryptoAPI (CryptAcquireContext, etc.)
    #[cfg(windows)]
    println!("cargo:rustc-link-lib=advapi32");
}
