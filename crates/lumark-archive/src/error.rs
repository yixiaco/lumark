#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("不支持的归档格式：{0}")]
    UnsupportedFormat(String),
    #[error("归档文件损坏：{0}")]
    Corrupt(String),
    #[error("需要密码")]
    PasswordRequired,
    #[error("密码错误")]
    WrongPassword,
    #[error("路径不安全：{0}")]
    UnsafePath(String),
    #[error("目标已存在：{0}")]
    AlreadyExists(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("归档库错误：{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("7z 错误：{0}")]
    SevenZ(String),
    #[error("任务已取消")]
    Cancelled,
}
