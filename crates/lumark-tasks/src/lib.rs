//! Background task orchestration for compression and extraction.

pub mod manager;
pub mod progress;

pub use manager::{ArchiveLoadResult, ResolvedPassword, TaskControl, TaskManager, TaskRequest};
pub use progress::{TaskEvent, TaskKind, TaskProgress, TaskState};
