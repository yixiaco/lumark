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
    /// 用户输入或当前后端无法满足的请求，例如空压缩计划、重复归档路径。
    #[error("无法继续：{0}")]
    InvalidRequest(String),
    /// 解压超出资源预算，通常是压缩炸弹或异常巨大的归档。
    #[error("解压超出资源预算：{0}")]
    BudgetExceeded(String),
    /// 归档操作内部 panic 被上层捕获，用于把“线程直接消失”变成可见的失败结果。
    #[error("任务异常中止：{0}")]
    Panicked(String),
}
