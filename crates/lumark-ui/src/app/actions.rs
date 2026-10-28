//! 应用动作入口，按文件、归档、密码、设置和任务拆分。

mod archive;
mod disguise;
mod files;
mod iso;
mod passwords;
mod settings;
pub(crate) mod tasks;

pub(crate) use files::TempExtractMode;

pub(crate) mod mail;
