//! Lumark 桌面应用入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 启动 Lumark 应用并把错误写入日志。
fn main() -> anyhow::Result<()> {
    lumark_ui::run()
}
