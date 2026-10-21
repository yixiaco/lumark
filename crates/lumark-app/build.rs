//! 构建脚本：把 Lumark 应用图标写入 Windows 可执行文件资源。
//!
//! 图标同时供任务栏、Alt+Tab 和资源管理器右键菜单的 `DefaultIcon` 使用；
//! 资源 ID 固定为 1，Windows 默认取第一枚图标作为程序图标。
fn main() {
    println!("cargo:rerun-if-changed=assets/lumark.ico");
    println!("cargo:rerun-if-changed=lumark.rc");

    #[cfg(windows)]
    embed_resource::compile("lumark.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("嵌入 Windows 应用图标失败");
}
