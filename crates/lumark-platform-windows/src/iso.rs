//! ISO 虚拟光驱挂载与卸载。
//!
//! 使用 Windows 8+ 自带的 `Mount-DiskImage` / `Dismount-DiskImage`。客户端
//! 系统普通用户通常可以直接挂载，Server 或收紧的安全策略可能要求管理员
//! 权限；失败时把 PowerShell 的原始错误交给界面显示。
//!
//! 通过隐藏窗口的 PowerShell 子进程执行固定脚本，ISO 路径通过环境变量
//! 传入，避免命令行引号拼接和路径注入。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::PlatformError;

/// 挂载成功后返回的虚拟光驱信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedIso {
    /// Windows 分配的盘符；卷尚未就绪或没有盘符时为 `None`。
    pub drive_letter: Option<char>,
}

/// 挂载并等待卷出现盘符。
const MOUNT_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
if (-not (Get-Command Mount-DiskImage -ErrorAction SilentlyContinue)) {
    throw '当前 Windows 不支持 ISO 挂载（需要 Windows 8 或更高版本）'
}
$path = $env:LUMARK_ISO_PATH
$image = Get-DiskImage -ImagePath $path -ErrorAction SilentlyContinue
if (-not $image -or -not $image.Attached) {
    $image = Mount-DiskImage -ImagePath $path -PassThru -ErrorAction Stop
}
$deadline = (Get-Date).AddSeconds(15)
$volume = $null
do {
    $volume = Get-Volume -DiskImage $image -ErrorAction SilentlyContinue |
        Where-Object { $_.DriveLetter } |
        Select-Object -First 1
    if ($volume -and $volume.DriveLetter) { break }
    Start-Sleep -Milliseconds 200
} while ((Get-Date) -lt $deadline)
if ($volume -and $volume.DriveLetter) {
    [Console]::Out.Write("DRIVE=" + $volume.DriveLetter)
} else {
    [Console]::Out.Write("DRIVE=")
}
"#;

/// 卸载已挂载的 ISO；未挂载时视为成功。
const UNMOUNT_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
if (-not (Get-Command Dismount-DiskImage -ErrorAction SilentlyContinue)) {
    throw '当前 Windows 不支持 ISO 卸载（需要 Windows 8 或更高版本）'
}
$path = $env:LUMARK_ISO_PATH
$image = Get-DiskImage -ImagePath $path -ErrorAction SilentlyContinue
if ($image -and $image.Attached) {
    Dismount-DiskImage -ImagePath $path -ErrorAction Stop
}
[Console]::Out.Write("OK")
"#;

/// 使用系统内置的磁盘映像命令。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 返回随系统安装的 Windows PowerShell 路径。
///
/// 不依赖 PATH，避免被同名程序劫持；32 位进程的 System32 重定向仍能命中
/// 可用的 PowerShell。
fn powershell_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// 验证目标是 `.iso` 路径；挂载时还要求文件真实存在。
fn validate_iso(path: &Path, require_file: bool) -> Result<(), PlatformError> {
    if require_file && !path.is_file() {
        return Err(PlatformError::Iso(format!(
            "ISO 文件不存在：{}",
            path.display()
        )));
    }
    let is_iso = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"));
    if !is_iso {
        return Err(PlatformError::Iso(format!(
            "不是 ISO 文件：{}",
            path.display()
        )));
    }
    Ok(())
}

/// 运行固定 PowerShell 脚本，路径通过环境变量传入。
///
/// 使用 `try_wait` 轮询并设置超时，避免异常环境下界面后台任务无限等待。
fn run_script(script: &str, path: &Path) -> Result<String, PlatformError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let mut command = Command::new(powershell_path());
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .env("LUMARK_ISO_PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = command.spawn().map_err(PlatformError::Windows)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PlatformError::Windows(error));
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::Iso("虚拟光驱操作超时".into()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let output = child.wait_with_output().map_err(PlatformError::Windows)?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            let message = if stderr.is_empty() { stdout } else { stderr };
            return Err(PlatformError::Iso(if message.is_empty() {
                format!(
                    "PowerShell 退出码 {}",
                    output.status.code().unwrap_or_default()
                )
            } else {
                message
            }));
        }
        Ok(stdout)
    }
    #[cfg(not(windows))]
    {
        let _ = (script, path);
        Err(PlatformError::Iso(
            "虚拟光驱挂载仅在 Windows 上可用".to_string(),
        ))
    }
}

/// 解析挂载脚本输出。
fn parse_drive_output(stdout: &str) -> Option<char> {
    let value = stdout.trim().strip_prefix("DRIVE=")?.trim();
    let letter = value.chars().next()?;
    letter
        .is_ascii_alphabetic()
        .then(|| letter.to_ascii_uppercase())
}

/// 挂载 ISO 并返回系统分配的盘符。
///
/// 已经挂载的 ISO 会直接返回现有盘符，不重复挂载。
pub fn mount(path: &Path) -> Result<MountedIso, PlatformError> {
    validate_iso(path, true)?;
    let stdout = run_script(MOUNT_SCRIPT, path)?;
    Ok(MountedIso {
        drive_letter: parse_drive_output(&stdout),
    })
}

/// 卸载 ISO；未挂载时是幂等操作。
pub fn unmount(path: &Path) -> Result<(), PlatformError> {
    validate_iso(path, false)?;
    run_script(UNMOUNT_SCRIPT, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_drive_output;

    /// 验证挂载/卸载脚本输出和盘符解析。
    #[test]
    fn drive_output_is_parsed() {
        assert_eq!(parse_drive_output("DRIVE=E"), Some('E'));
        assert_eq!(parse_drive_output("  DRIVE=f \r\n"), Some('F'));
        assert_eq!(parse_drive_output("DRIVE="), None);
        assert_eq!(parse_drive_output("NONE"), None);
        assert_eq!(parse_drive_output("DRIVE=1"), None);
    }

    /// 提供 `LUMARK_ISO_SAMPLE` 时做一次真实挂载和卸载。
    ///
    /// 默认跳过，避免普通测试改动机器上的虚拟光驱状态。
    #[test]
    fn iso_mount_roundtrip_when_sample_provided() {
        let Ok(sample) = std::env::var("LUMARK_ISO_SAMPLE") else {
            return;
        };
        let path = std::path::PathBuf::from(sample);
        if !path.is_file() {
            return;
        }
        let mounted = super::mount(&path).expect("ISO 挂载应成功");
        assert!(mounted.drive_letter.is_some(), "真实 ISO 挂载后应分配盘符");
        super::unmount(&path).expect("ISO 卸载应成功");
    }
}
