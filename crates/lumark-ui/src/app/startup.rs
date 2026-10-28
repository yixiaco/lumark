//! 启动模式和单实例请求解析。

use super::*;

/// Shell 请求的打开方式，对应 `--open`、`--extract`、`--compress`、
/// `--disguise`、`--restore` 以及 ISO 的 `--mount-iso` / `--unmount-iso`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupMode {
    /// 打开归档。
    Open,
    /// 解压归档。
    Extract,
    /// 压缩路径。
    Compress,
    /// 打开伪装压缩窗口，把选中的文件或文件夹包装成媒体外观。
    Disguise,
    /// 把伪装文件还原成原归档或原文件。
    Restore,
    /// 挂载 ISO。
    MountIso,
    /// 卸载 ISO。
    UnmountIso,
}

/// 一个由独立窗口处理的启动请求。
///
/// 首次启动来自命令行参数，之后由单实例转发的请求队列产生；两种来源
/// 都只描述要处理哪些路径，窗口和界面状态由接收它的窗口自己创建。
#[derive(Debug, Clone)]
pub(crate) struct StartupRequest {
    /// 请求的处理方式。
    pub(crate) mode: StartupMode,
    /// 请求携带的路径。
    pub(crate) paths: Vec<PathBuf>,
}

impl StartupRequest {
    /// 解析命令行参数并保留非 UTF-8 路径。
    pub(crate) fn from_env() -> Option<Self> {
        Self::parse_os(std::env::args_os().skip(1))
    }

    /// 解析单实例转发请求，参数已经由发送方转换成 UTF-8 文本。
    pub(crate) fn parse(arguments: Vec<String>) -> Option<Self> {
        Self::parse_os(arguments.into_iter().map(std::ffi::OsString::from))
    }

    /// 识别启动模式；未知开关忽略，其余参数按路径处理。
    fn parse_os(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Option<Self> {
        let mut mode = StartupMode::Open;
        let mut paths = Vec::new();
        for argument in arguments {
            let text = argument.to_string_lossy();
            match text.as_ref() {
                "--open" => mode = StartupMode::Open,
                "--extract" => mode = StartupMode::Extract,
                "--compress" => mode = StartupMode::Compress,
                "--disguise" => mode = StartupMode::Disguise,
                "--restore" => mode = StartupMode::Restore,
                "--mount-iso" => mode = StartupMode::MountIso,
                "--unmount-iso" => mode = StartupMode::UnmountIso,
                value if value.starts_with('-') => {}
                _ => paths.push(PathBuf::from(argument)),
            }
        }
        if paths.is_empty() {
            return None;
        }
        Some(Self { mode, paths })
    }

    /// 返回请求真正要处理的全部路径。
    ///
    /// 打开和解压只接受压缩包文件，压缩还接受文件夹；不匹配的路径会被
    /// 过滤，避免为无效请求打开空窗口。Shell 一次传入多个路径时由界面
    /// 决定开多个窗口还是合并处理。
    pub(crate) fn targets(&self) -> Vec<&Path> {
        self.paths
            .iter()
            .map(PathBuf::as_path)
            .filter(|path| match self.mode {
                StartupMode::Compress | StartupMode::Disguise => path.exists(),
                StartupMode::Restore => path.is_file(),
                StartupMode::MountIso | StartupMode::UnmountIso => {
                    path.is_file() && is_iso_path(path)
                }
                StartupMode::Open | StartupMode::Extract => path.is_file() && is_archive_path(path),
            })
            .collect()
    }

    /// 返回请求真正要处理的第一个路径。
    pub(crate) fn target(&self) -> Option<&Path> {
        self.targets().into_iter().next()
    }
}
