//! 自解压运行时命令行参数解析。
//!
//! 交互模式由 Win32 窗口处理；这里只负责把 `/S`、`/D=`、`/P=`、`/Y`
//! 解析成可测试的计划，避免窗口代码承担参数语义。

use std::path::PathBuf;

use zeroize::Zeroizing;

/// 一次自解压运行的参数计划。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SfxPlan {
    /// 静默模式：不显示目录/密码窗口，也不在成功后打开资源管理器。
    pub silent: bool,
    /// 显式指定的解压目录。
    pub target: Option<PathBuf>,
    /// 显式指定的密码；优先于交互输入。
    pub password: Option<Zeroizing<String>>,
    /// 覆盖已存在文件，不再询问。
    pub overwrite: bool,
}

/// 解析自解压命令行参数；未知参数忽略，保持旧版本兼容。
pub fn parse_args<I>(arguments: I) -> SfxPlan
where
    I: IntoIterator<Item = String>,
{
    let mut plan = SfxPlan::default();
    for argument in arguments {
        if argument == "/S" || argument == "--silent" {
            plan.silent = true;
        } else if argument == "/Y" || argument == "--yes" {
            plan.overwrite = true;
        } else if let Some(value) = argument.strip_prefix("/D=") {
            if !value.is_empty() {
                plan.target = Some(PathBuf::from(value));
            }
        } else if let Some(value) = argument.strip_prefix("--target=") {
            if !value.is_empty() {
                plan.target = Some(PathBuf::from(value));
            }
        } else if let Some(value) = argument.strip_prefix("/P=") {
            plan.password = Some(Zeroizing::new(value.to_string()));
        } else if let Some(value) = argument.strip_prefix("--password=") {
            plan.password = Some(Zeroizing::new(value.to_string()));
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证静默、目录、密码和覆盖参数都能独立解析。
    #[test]
    fn parses_supported_arguments() {
        let plan = parse_args([
            "/S".to_string(),
            "/D=C:\\out".to_string(),
            "/P=secret".to_string(),
            "/Y".to_string(),
        ]);
        assert!(plan.silent);
        assert!(plan.overwrite);
        assert_eq!(plan.target, Some(PathBuf::from(r"C:\out")));
        assert_eq!(plan.password.as_deref().map(String::as_str), Some("secret"));
    }

    /// 无参数时保持交互模式，未知参数不产生副作用。
    #[test]
    fn defaults_to_interactive_and_ignores_unknown() {
        let plan = parse_args(["--unknown".to_string(), "/D=".to_string()]);
        assert!(!plan.silent);
        assert!(!plan.overwrite);
        assert!(plan.target.is_none());
        assert!(plan.password.is_none());
    }
}
