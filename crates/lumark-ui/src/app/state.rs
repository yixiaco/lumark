//! 主窗口跨模块共享的辅助状态类型。
//!
//! 这些结构只描述界面状态，不包含窗口生命周期或动作实现；单独成文件可以
//! 让 `app.rs` 专注于应用状态、构造和窗口入口。

use std::path::PathBuf;

use gpui_kit::Point;
use lumark_settings::ColumnKind;
use lumark_tasks::{TaskKind, TaskProgress, TaskState};

/// 任务中心展示的一条任务记录。
///
/// 记录在任务整个生命周期内保留，成功后也不会立即删除，因此用户可以从
/// 任务中心重新打开后台任务的进度窗口或查看最终结果。
#[derive(Clone)]
pub(crate) struct TaskRecord {
    /// 任务 ID。
    pub(crate) id: u64,
    /// 任务类型。
    pub(crate) kind: TaskKind,
    /// 任务展示名称。
    pub(crate) label: String,
    /// 任务状态。
    pub(crate) state: TaskState,
    /// 最近进度。
    pub(crate) progress: TaskProgress,
    /// 完成或失败消息。
    pub(crate) message: Option<String>,
}

/// 拖出或复制归档条目时的临时解压进度弹窗。
#[derive(Clone)]
pub(crate) struct TempExtractDialog {
    /// 弹窗标题。
    pub(crate) title: String,
    /// 说明文本。
    pub(crate) message: String,
    /// 临时解压进度。
    pub(crate) progress: f32,
}

/// 侧栏目录树的扁平行。
pub(crate) struct TreeRow {
    pub(crate) path: PathBuf,
    pub(crate) label: String,
    pub(crate) depth: usize,
    pub(crate) expanded: bool,
}

/// 正在拖动调整的文件表列。
#[derive(Clone, Copy)]
pub(crate) struct ColumnResize {
    /// 正在调整的列。
    pub(crate) column: ColumnKind,
    /// 按下鼠标时的窗口横坐标。
    pub(crate) start_x: f32,
    /// 按下鼠标时的列宽。
    pub(crate) start_width: f32,
}

/// 文件表框选状态。
///
/// 框选只在文件表空白区域开始，记录按下/当前位置和框选开始时的选择快照；
/// Ctrl/Shift 按下时在快照上追加，否则替换选择。
#[derive(Clone, Debug)]
pub(crate) struct MarqueeState {
    /// 按下鼠标时的窗口坐标。
    pub(crate) start: Point<gpui_kit::Pixels>,
    /// 当前鼠标的窗口坐标。
    pub(crate) current: Point<gpui_kit::Pixels>,
    /// 开始框选前已选中的条目 ID。
    pub(crate) base_ids: Vec<String>,
}

/// 等待详情解析完成后要自动打开的资产查看器。
///
/// 双击和右键“使用内置查看器打开”都走同一个入口：资产已经解析就立即打开
/// 模型/纹理查看器，尚未解析则先挂起请求，等后台预览完成后再打开，避免两次
/// 入口给出不同提示。
#[derive(Clone, Debug)]
pub(crate) struct PendingAssetViewer {
    /// 和 `DetailsPreview` 对齐的缓存 key。
    pub(crate) key: String,
}
