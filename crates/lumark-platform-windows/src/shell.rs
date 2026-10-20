use std::path::Path;

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::PlatformError;

const ARCHIVE_PROG_ID: &str = "Lumark.Archive";

pub fn register_current_user() -> Result<(), PlatformError> {
    let executable = std::env::current_exe()
        .map_err(PlatformError::Windows)?
        .display()
        .to_string();
    let quoted = format!("\"{executable}\"");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu
        .create_subkey("Software\\Classes")
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?
        .0;

    for (verb_key, label, command) in [
        (
            "Lumark.Open",
            "用 Lumark 打开",
            format!("{quoted} --open \"%1\""),
        ),
        (
            "Lumark.Extract",
            "解压到…",
            format!("{quoted} --extract \"%1\""),
        ),
        (
            "Lumark.Compress",
            "压缩为 Lumark 归档",
            format!("{quoted} --compress \"%1\""),
        ),
    ] {
        let path = format!("*\\shell\\{verb_key}");
        let (key, _) = classes
            .create_subkey(&path)
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        key.set_value("", &label)
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        let (command_key, _) = key
            .create_subkey("command")
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        command_key
            .set_value("", &command)
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    }

    for base in ["Directory\\shell", "Directory\\Background\\shell"] {
        let path = format!("{base}\\Lumark.Compress");
        let (key, _) = classes
            .create_subkey(&path)
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        key.set_value("", &"用 Lumark 压缩")
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        let (command_key, _) = key
            .create_subkey("command")
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        command_key
            .set_value("", &format!("{quoted} --compress \"%1\""))
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    }

    let (prog_id, _) = classes
        .create_subkey(ARCHIVE_PROG_ID)
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    prog_id
        .set_value("", &"Lumark 压缩文件")
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    let (open_key, _) = prog_id
        .create_subkey("shell\\open\\command")
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    open_key
        .set_value("", &format!("{quoted} --open \"%1\""))
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    let (icon_key, _) = prog_id
        .create_subkey("DefaultIcon")
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    icon_key
        .set_value("", &format!("{executable},0"))
        .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;

    for extension in [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".zst", ".lz4", ".cab", ".iso",
        ".wim", ".arj", ".ace",
    ] {
        let (key, _) = classes
            .create_subkey(format!("{extension}\\OpenWithProgids"))
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
        key.set_value(ARCHIVE_PROG_ID, &"")
            .map_err(|error| PlatformError::Windows(std::io::Error::other(error.to_string())))?;
    }
    Ok(())
}

pub fn unregister_current_user() -> Result<(), PlatformError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = "Software\\Classes".to_string();
    for key in [
        "*\\shell\\Lumark.Open",
        "*\\shell\\Lumark.Extract",
        "*\\shell\\Lumark.Compress",
        "Directory\\shell\\Lumark.Compress",
        "Directory\\Background\\shell\\Lumark.Compress",
        ARCHIVE_PROG_ID,
    ] {
        let _ = hkcu.delete_subkey_all(format!("{classes}\\{key}"));
    }
    Ok(())
}

pub fn copy_to_send_to(_executable: &Path) -> Result<(), PlatformError> {
    // 创建快捷方式需要 Shell Link COM 接口。调用方仍可使用上述经典命令；
    // 在安装器接管快捷方式生命周期前，这里刻意保持无操作。
    Ok(())
}
