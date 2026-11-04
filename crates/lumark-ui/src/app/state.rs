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

/// 文件表显示顺序到源条目下标的映射。
///
/// 游戏容器会隐藏 `.uexp/.ubulk/.uptnl` 伴随文件；没有过滤时 `Direct`
/// 直接按源下标访问，不复制索引表。渲染层每帧只构建一次，行回调按
/// `source_index` O(1) 取源条目，避免每个可见行都重新扫描整张表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DisplayedOrder {
    /// 未过滤：显示下标即源下标。
    Direct(usize),
    /// 已过滤：`Vec` 保存每个显示下标对应的源下标。
    Filtered(Vec<usize>),
}

impl DisplayedOrder {
    /// 按源条目数量与保留谓词构造映射；全部保留时返回 `Direct`。
    pub(crate) fn from_source(len: usize, keep: impl Fn(usize) -> bool) -> Self {
        let indices: Vec<usize> = (0..len).filter(|index| keep(*index)).collect();
        if indices.len() == len {
            Self::Direct(len)
        } else {
            Self::Filtered(indices)
        }
    }

    /// 显示条目数量。
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Direct(len) => *len,
            Self::Filtered(indices) => indices.len(),
        }
    }

    /// 显示下标 → 源下标；越界返回 `None`。
    pub(crate) fn source_index(&self, display_index: usize) -> Option<usize> {
        match self {
            Self::Direct(len) => (display_index < *len).then_some(display_index),
            Self::Filtered(indices) => indices.get(display_index).copied(),
        }
    }
}

/// 当前目录摘要：条目数、总大小与归档压缩后总大小。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DisplayedSummary {
    /// 显示条目数量。
    pub(crate) count: usize,
    /// 全部显示条目的总大小。
    pub(crate) total_size: u64,
    /// 归档条目的压缩后总大小；没有任何压缩信息时为 `None`。
    pub(crate) compressed_size: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部保留时使用零分配的显示顺序。
    #[test]
    fn displayed_order_keeps_direct_when_nothing_filtered() {
        let order = DisplayedOrder::from_source(3, |_| true);
        assert_eq!(order, DisplayedOrder::Direct(3));
        assert_eq!(order.len(), 3);
        assert_eq!(order.source_index(0), Some(0));
        assert_eq!(order.source_index(2), Some(2));
        assert_eq!(order.source_index(3), None);
    }

    /// 过滤后显示下标映射回原表下标，越界返回 None。
    #[test]
    fn displayed_order_maps_filtered_indices() {
        let order = DisplayedOrder::from_source(6, |index| index % 2 == 0);
        assert_eq!(order, DisplayedOrder::Filtered(vec![0, 2, 4]));
        assert_eq!(order.len(), 3);
        assert_eq!(order.source_index(1), Some(2));
        assert_eq!(order.source_index(2), Some(4));
        assert_eq!(order.source_index(3), None);
    }

    /// 空表不分配索引。
    #[test]
    fn displayed_order_handles_empty_source() {
        assert_eq!(
            DisplayedOrder::from_source(0, |_| true),
            DisplayedOrder::Direct(0)
        );
        assert_eq!(
            DisplayedOrder::from_source(0, |_| false),
            DisplayedOrder::Direct(0)
        );
    }

    /// 摘要默认值表示空目录。
    #[test]
    fn displayed_summary_defaults_to_empty() {
        let summary = DisplayedSummary::default();
        assert_eq!(summary.count, 0);
        assert_eq!(summary.total_size, 0);
        assert_eq!(summary.compressed_size, None);
    }
}
