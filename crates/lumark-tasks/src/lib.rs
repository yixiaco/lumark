//! 压缩和解压的后台任务编排。

pub mod manager;
pub mod progress;

pub use manager::{ArchiveLoadResult, ResolvedPassword, TaskControl, TaskManager, TaskRequest};
pub use progress::{TaskEvent, TaskKind, TaskProgress, TaskState};
