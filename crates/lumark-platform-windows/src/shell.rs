//! 管理当前用户的文件关联和资源管理器右键菜单注册表项。
//!
//! 所有写入都限定在 `HKEY_CURRENT_USER`，无需管理员权限；文件关联只注册
//! Lumark 的 ProgID 和打开方式，默认应用仍由 Windows 的“默认应用”页面确认。

use std::collections::HashSet;
use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

use crate::PlatformError;

/// 归档文件关联的 ProgID。
const ARCHIVE_PROG_ID: &str = "Lumark.Archive";
/// 伪装媒体文件的“打开方式”ProgID。
const MEDIA_PROG_ID: &str = "Lumark.Media";
/// 注册到默认程序和开机启动项的应用名。
const APPLICATION_NAME: &str = "Lumark";
/// Windows 默认程序能力注册表子键。
const CAPABILITIES_SUBKEY: &str = "Software\\Lumark\\Capabilities";
/// 已注册应用列表子键。
const REGISTERED_APPLICATIONS: &str = "Software\\RegisteredApplications";

/// Lumark 注册为打开方式的压缩格式。
pub const ARCHIVE_EXTENSIONS: [&str; 28] = [
    ".zip",
    ".7z",
    ".rar",
    ".zpaq",
    ".zpq",
    ".lzh",
    ".lha",
    ".tar",
    ".gz",
    ".bz2",
    ".xz",
    ".zst",
    ".lz4",
    ".cab",
    ".iso",
    ".wim",
    ".arj",
    ".ace",
    ".alz",
    ".tar.br",
    ".tar.lz4",
    ".tar.lz",
    ".tar.lzma",
    ".tar.z",
    ".xar",
    ".pkg",
    ".rpm",
    ".msi",
];

/// 添加“打开方式”候选的媒体扩展名；不注册默认关联。
pub const MEDIA_EXTENSIONS: [&str; 2] = [".mp4", ".jpg"];

/// 为当前用户注册全部文件关联和资源管理器右键菜单。
pub fn register_current_user() -> Result<(), PlatformError> {
    register_shell_menu()?;
    let extensions = ARCHIVE_EXTENSIONS
        .iter()
        .map(|extension| extension.to_string())
        .collect::<Vec<_>>();
    register_file_associations(&extensions)
}

/// 移除当前用户注册的全部 Shell 集成项。
pub fn unregister_current_user() -> Result<(), PlatformError> {
    unregister_shell_menu()?;
    unregister_file_associations()
}

/// 注册当前用户的经典资源管理器右键菜单。
pub fn register_shell_menu() -> Result<(), PlatformError> {
    let executable = std::env::current_exe().map_err(PlatformError::Windows)?;
    let quoted = quote_executable(&executable);
    let icon = icon_reference(&executable);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu
        .create_subkey("Software\\Classes")
        .map_err(registry_error)?
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
        (
            "Lumark.Disguise",
            "伪装为媒体文件…",
            format!("{quoted} --disguise \"%1\""),
        ),
        (
            "Lumark.Restore",
            "Lumark 一键还原",
            format!("{quoted} --restore \"%1\""),
        ),
    ] {
        write_shell_verb(
            &classes,
            &format!("*\\shell\\{verb_key}"),
            label,
            &command,
            &icon,
            verb_key == "Lumark.Compress",
        )?;
    }

    for base in ["Directory\\shell", "Directory\\Background\\shell"] {
        write_shell_verb(
            &classes,
            &format!("{base}\\Lumark.Compress"),
            "用 Lumark 压缩",
            &format!("{quoted} --compress \"%1\""),
            &icon,
            true,
        )?;
    }

    // ISO 专属菜单放在 SystemFileAssociations 下，不依赖 .iso 当前绑定的
    // ProgID；即使第三方压缩软件抢占了默认打开方式，也能看到 Lumark 入口。
    for (verb_key, label, argument) in [
        ("Lumark.MountIso", "加载至虚拟光驱", "--mount-iso"),
        ("Lumark.UnmountIso", "卸载虚拟光驱", "--unmount-iso"),
    ] {
        write_shell_verb(
            &classes,
            &format!("SystemFileAssociations\\.iso\\shell\\{verb_key}"),
            label,
            &format!("{quoted} {argument} \"%1\""),
            &icon,
            false,
        )?;
    }
    notify_shell();
    Ok(())
}

/// 移除当前用户的经典资源管理器右键菜单。
pub fn unregister_shell_menu() -> Result<(), PlatformError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = "Software\\Classes";
    for key in [
        "*\\shell\\Lumark.Open",
        "*\\shell\\Lumark.Extract",
        "*\\shell\\Lumark.Compress",
        "*\\shell\\Lumark.Restore",
        "Directory\\shell\\Lumark.Compress",
        "Directory\\Background\\shell\\Lumark.Compress",
        "SystemFileAssociations\\.iso\\shell\\Lumark.MountIso",
        "SystemFileAssociations\\.iso\\shell\\Lumark.UnmountIso",
    ] {
        let _ = hkcu.delete_subkey_all(format!("{classes}\\{key}"));
    }
    Ok(())
}

/// 注册压缩格式的 ProgID、打开方式、应用能力和默认应用入口。
///
/// `extensions` 只保留用户选中的扩展名；未选中的格式会从 OpenWithProgids、
/// 应用能力和 SupportedTypes 中移除。函数只把 Lumark 添加到 Windows 的
/// 候选应用列表，不直接改写扩展名的默认值；Windows 10/11 的最终默认应用
/// 由用户在系统设置中确认。
pub fn register_file_associations(extensions: &[String]) -> Result<(), PlatformError> {
    let selected = extensions
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let executable = std::env::current_exe().map_err(PlatformError::Windows)?;
    let quoted = quote_executable(&executable);
    let command = format!("{quoted} --open \"%1\"");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu
        .create_subkey("Software\\Classes")
        .map_err(registry_error)?
        .0;

    let (prog_id, _) = classes
        .create_subkey(ARCHIVE_PROG_ID)
        .map_err(registry_error)?;
    prog_id
        .set_value("", &"Lumark 压缩文件")
        .map_err(registry_error)?;
    let (open_command, _) = prog_id
        .create_subkey("shell\\open\\command")
        .map_err(registry_error)?;
    open_command
        .set_value("", &command)
        .map_err(registry_error)?;
    let (icon, _) = prog_id
        .create_subkey("DefaultIcon")
        .map_err(registry_error)?;
    icon.set_value("", &icon_reference(&executable))
        .map_err(registry_error)?;

    for extension in ARCHIVE_EXTENSIONS {
        let (open_with, _) = classes
            .create_subkey(format!("{extension}\\OpenWithProgids"))
            .map_err(registry_error)?;
        if selected.contains(extension) {
            open_with
                .set_value(ARCHIVE_PROG_ID, &"")
                .map_err(registry_error)?;
        } else {
            let _ = open_with.delete_value(ARCHIVE_PROG_ID);
        }
    }

    let (media_prog_id, _) = classes
        .create_subkey(MEDIA_PROG_ID)
        .map_err(registry_error)?;
    media_prog_id
        .set_value("", &"Lumark 伪装媒体")
        .map_err(registry_error)?;
    let (media_icon, _) = media_prog_id
        .create_subkey("DefaultIcon")
        .map_err(registry_error)?;
    media_icon
        .set_value("", &icon_reference(&executable))
        .map_err(registry_error)?;
    let (media_command, _) = media_prog_id
        .create_subkey("shell\\open\\command")
        .map_err(registry_error)?;
    media_command
        .set_value("", &command)
        .map_err(registry_error)?;
    for extension in MEDIA_EXTENSIONS {
        let (open_with, _) = classes
            .create_subkey(format!("{extension}\\OpenWithProgids"))
            .map_err(registry_error)?;
        open_with
            .set_value(MEDIA_PROG_ID, &"")
            .map_err(registry_error)?;
    }

    let application_key = application_key_name(&executable);
    let (application, _) = classes
        .create_subkey(format!("Applications\\{application_key}"))
        .map_err(registry_error)?;
    application
        .set_value("FriendlyAppName", &APPLICATION_NAME)
        .map_err(registry_error)?;
    application
        .set_value("ApplicationCompany", &APPLICATION_NAME)
        .map_err(registry_error)?;
    let (application_command, _) = application
        .create_subkey("shell\\open\\command")
        .map_err(registry_error)?;
    application_command
        .set_value("", &command)
        .map_err(registry_error)?;
    let (supported, _) = application
        .create_subkey("SupportedTypes")
        .map_err(registry_error)?;
    for extension in ARCHIVE_EXTENSIONS {
        if selected.contains(extension) {
            supported
                .set_value(extension, &"")
                .map_err(registry_error)?;
        } else {
            let _ = supported.delete_value(extension);
        }
    }

    let (capabilities, _) = hkcu
        .create_subkey(CAPABILITIES_SUBKEY)
        .map_err(registry_error)?;
    capabilities
        .set_value("ApplicationName", &APPLICATION_NAME)
        .map_err(registry_error)?;
    capabilities
        .set_value("ApplicationDescription", &"本地优先的压缩管理器")
        .map_err(registry_error)?;
    capabilities
        .set_value("ApplicationIcon", &icon_reference(&executable))
        .map_err(registry_error)?;
    let (file_associations, _) = capabilities
        .create_subkey("FileAssociations")
        .map_err(registry_error)?;
    for extension in ARCHIVE_EXTENSIONS {
        if selected.contains(extension) {
            file_associations
                .set_value(extension, &ARCHIVE_PROG_ID)
                .map_err(registry_error)?;
        } else {
            let _ = file_associations.delete_value(extension);
        }
    }
    let (registered, _) = hkcu
        .create_subkey(REGISTERED_APPLICATIONS)
        .map_err(registry_error)?;
    registered
        .set_value(APPLICATION_NAME, &CAPABILITIES_SUBKEY)
        .map_err(registry_error)?;

    notify_shell();
    Ok(())
}

/// 移除当前用户的文件关联和应用能力注册。
pub fn unregister_file_associations() -> Result<(), PlatformError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(classes) = hkcu.open_subkey("Software\\Classes") {
        for extension in ARCHIVE_EXTENSIONS {
            if let Ok(open_with) = hkcu.open_subkey_with_flags(
                format!("Software\\Classes\\{extension}\\OpenWithProgids"),
                KEY_READ | KEY_SET_VALUE,
            ) {
                let _ = open_with.delete_value(ARCHIVE_PROG_ID);
            }
        }
        for extension in MEDIA_EXTENSIONS {
            if let Ok(open_with) = hkcu.open_subkey_with_flags(
                format!("Software\\Classes\\{extension}\\OpenWithProgids"),
                KEY_READ | KEY_SET_VALUE,
            ) {
                let _ = open_with.delete_value(MEDIA_PROG_ID);
            }
        }
        let _ = classes.delete_subkey_all(MEDIA_PROG_ID);
        let _ = classes.delete_subkey_all(ARCHIVE_PROG_ID);
        if let Ok(executable) = std::env::current_exe() {
            let _ = classes.delete_subkey_all(format!(
                "Applications\\{}",
                application_key_name(&executable)
            ));
        }
    }

    let _ = hkcu.delete_subkey_all(CAPABILITIES_SUBKEY);
    let _ = hkcu.delete_subkey("Software\\Lumark");
    if let Ok(registered) =
        hkcu.open_subkey_with_flags(REGISTERED_APPLICATIONS, KEY_READ | KEY_SET_VALUE)
        && let Ok(value) = registered.get_value::<String, _>(APPLICATION_NAME)
        && value == CAPABILITIES_SUBKEY
    {
        let _ = registered.delete_value(APPLICATION_NAME);
    }

    notify_shell();
    Ok(())
}

/// 返回当前用户已选择的压缩格式。
///
/// 同时要求 OpenWithProgids 和默认应用能力项仍指向 Lumark；可执行文件移动后
/// 旧注册项会失效，因此这里也会校验当前 ProgID 的打开命令。
pub fn registered_formats() -> Vec<String> {
    if !registration_matches_current_executable() {
        return Vec::new();
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    ARCHIVE_EXTENSIONS
        .iter()
        .filter(|extension| {
            let open_with = hkcu
                .open_subkey(format!("Software\\Classes\\{extension}\\OpenWithProgids"))
                .ok()
                .and_then(|key| key.get_value::<String, _>(ARCHIVE_PROG_ID).ok())
                .is_some();
            let capability = hkcu
                .open_subkey(format!("{CAPABILITIES_SUBKEY}\\FileAssociations"))
                .ok()
                .and_then(|key| key.get_value::<String, _>(*extension).ok())
                .map(|value| value == ARCHIVE_PROG_ID)
                .unwrap_or(false);
            open_with && capability
        })
        .map(|extension| (*extension).to_string())
        .collect()
}

/// 返回当前用户已完整注册的压缩格式数量。
pub fn registered_extension_count() -> usize {
    registered_formats().len()
}

/// 写入一个资源管理器命令，包括显示名称和 `command` 子键。
fn write_shell_verb(
    classes: &RegKey,
    path: &str,
    label: &str,
    command: &str,
    icon: &str,
    multi_select: bool,
) -> Result<(), PlatformError> {
    let (key, _) = classes.create_subkey(path).map_err(registry_error)?;
    key.set_value("", &label).map_err(registry_error)?;
    key.set_value("Icon", &icon).map_err(registry_error)?;
    if multi_select {
        // Player 允许一次选择多个文件；应用侧会把逐文件进程请求合并。
        key.set_value("MultiSelectModel", &"Player")
            .map_err(registry_error)?;
    }
    let (command_key, _) = key.create_subkey("command").map_err(registry_error)?;
    command_key
        .set_value("", &command)
        .map_err(registry_error)?;
    Ok(())
}

/// 用双引号包裹可执行文件路径，避免空格被 Shell 拆分。
fn quote_executable(executable: &Path) -> String {
    format!("\"{}\"", executable.display())
}

/// 返回注册表图标引用；引号保证带空格的安装路径也能被 Shell 解析。
fn icon_reference(executable: &Path) -> String {
    format!("\"{}\",0", executable.display())
}

/// 返回注册表 `Applications` 子键使用的可执行文件名。
fn application_key_name(executable: &Path) -> String {
    executable
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("lumark-app.exe")
        .to_string()
}

/// 校验当前注册项仍指向正在运行的可执行文件。
fn registration_matches_current_executable() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let executable_text = executable.display().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(command_key) = hkcu.open_subkey(format!(
        "Software\\Classes\\{ARCHIVE_PROG_ID}\\shell\\open\\command"
    )) else {
        return false;
    };
    let Ok(command) = command_key.get_value::<String, _>("") else {
        return false;
    };
    if !command.contains(&executable_text) {
        return false;
    }
    let Ok(registered) = hkcu.open_subkey(REGISTERED_APPLICATIONS) else {
        return false;
    };
    registered
        .get_value::<String, _>(APPLICATION_NAME)
        .map(|value| value == CAPABILITIES_SUBKEY)
        .unwrap_or(false)
}

/// 把注册表错误转换为平台错误，保留错误文本。
fn registry_error(error: std::io::Error) -> PlatformError {
    PlatformError::Windows(std::io::Error::other(error.to_string()))
}

/// 通知资源管理器文件关联已变化，刷新图标和“打开方式”列表。
fn notify_shell() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ARCHIVE_EXTENSIONS, icon_reference, quote_executable};
    use std::path::Path;

    /// 验证可执行文件路径带空格时仍作为一个完整的 Shell 参数。
    #[test]
    fn executable_path_is_quoted_for_shell() {
        let command = quote_executable(Path::new(r"C:\Program Files\Lumark\Lumark.exe"));
        assert_eq!(command, r#""C:\Program Files\Lumark\Lumark.exe""#);
    }

    /// 验证右键菜单图标引用使用带引号的路径和资源索引 0。
    #[test]
    fn executable_icon_reference_quotes_path() {
        let icon = icon_reference(Path::new(r"C:\Program Files\Lumark\Lumark.exe"));
        assert_eq!(icon, r#""C:\Program Files\Lumark\Lumark.exe",0"#);
    }

    /// 验证注册的扩展名都带点且没有重复项。
    #[test]
    fn archive_extensions_are_unique_and_dotted() {
        let mut unique = ARCHIVE_EXTENSIONS.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ARCHIVE_EXTENSIONS.len());
        assert!(
            ARCHIVE_EXTENSIONS
                .iter()
                .all(|extension| { extension.starts_with('.') && extension.len() > 1 })
        );
    }
}
