//! 构建脚本：给自解压模块加上与应用一致的 Windows 图标。
//!
//! 模块和主程序同为 Windows 可执行文件，复用同一枚图标可以避免自解压包在
//! 资源管理器里显示成默认程序图标。
fn main() {
    println!("cargo:rerun-if-changed=lumark.rc");
    println!("cargo:rerun-if-changed=../lumark-app/assets/lumark.ico");

    #[cfg(windows)]
    embed_resource::compile("lumark.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("嵌入自解压模块图标失败");
}
