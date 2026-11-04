//! 文件表框选的几何计算。
//!
//! 文件表使用统一行高（详情列表 31px、平铺 132px），因此可以只根据滚动视口、
//! 滚动偏移和行高把鼠标坐标换算成行列，不依赖逐行测量。这里集中处理空白
//! 判定与矩形命中，界面层只负责鼠标事件和绘制。

/// 详情列表行高。
pub(super) const LIST_ROW_HEIGHT: f32 = 31.0;
/// 平铺视图行高。
pub(super) const TILE_ROW_HEIGHT: f32 = 132.0;
/// 平铺卡片的参考宽度；实际宽度按可用空间均分。
pub(super) const TILE_NOMINAL_WIDTH: f32 = 120.0;
/// 平铺卡片最小宽度；窄窗口下减少列数而不是继续压缩。
pub(super) const TILE_MIN_WIDTH: f32 = 96.0;
/// 平铺卡片水平间距。
pub(super) const TILE_GAP: f32 = 8.0;
/// 平铺视图左右内边距。
pub(super) const TILE_PADDING_X: f32 = 12.0;
/// 右侧滚动条避让宽度；落在滚动条上的按下不开始框选。
pub(super) const SCROLLBAR_GUARD: f32 = 14.0;
/// 小于该位移视为普通点击而不是框选。
pub(super) const MIN_MARQUEE_DRAG: f32 = 3.0;
/// 靠近上下边缘时触发自动滚动的距离。
pub(super) const AUTOSCROLL_EDGE: f32 = 24.0;

/// 平铺视图的列布局；卡片宽度按可用宽度均分，避免右侧留下空白。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TileMetrics {
    /// 每行列数。
    pub(super) columns: usize,
    /// 卡片宽度。
    pub(super) tile_width: f32,
    /// 卡片中心距（宽度 + 间距）。
    pub(super) pitch: f32,
    /// 行高。
    pub(super) row_height: f32,
    /// 左右内边距。
    pub(super) padding: f32,
}

impl TileMetrics {
    /// 根据文件表可用宽度计算平铺布局；列表视图返回单列。
    pub(super) fn for_width(width: f32, tile: bool) -> Self {
        if !tile {
            return Self {
                columns: 1,
                tile_width: width.max(1.0),
                pitch: width.max(1.0),
                row_height: LIST_ROW_HEIGHT,
                padding: 0.0,
            };
        }
        let available = (width - TILE_PADDING_X * 2.0).max(TILE_MIN_WIDTH);
        let mut columns = ((available + TILE_GAP) / (TILE_NOMINAL_WIDTH + TILE_GAP))
            .floor()
            .max(1.0) as usize;
        let tile_width_for = |columns: usize| {
            (available - TILE_GAP * columns.saturating_sub(1) as f32) / columns as f32
        };
        let mut tile_width = tile_width_for(columns);
        // 窄窗口先减列，保证卡片不小于最小可读宽度。
        while tile_width < TILE_MIN_WIDTH && columns > 1 {
            columns -= 1;
            tile_width = tile_width_for(columns);
        }
        Self {
            columns,
            tile_width,
            pitch: tile_width + TILE_GAP,
            row_height: TILE_ROW_HEIGHT,
            padding: TILE_PADDING_X,
        }
    }
}

/// 一次框选所需的文件表布局快照。
pub(super) struct MarqueeLayout {
    /// 滚动视口左边界（窗口坐标）。
    pub(super) left: f32,
    /// 滚动视口上边界（窗口坐标）。
    pub(super) top: f32,
    /// 滚动视口宽度。
    pub(super) width: f32,
    /// 滚动视口高度。
    pub(super) height: f32,
    /// 滚动偏移（内容向上移动时为负值）。
    pub(super) scroll_y: f32,
    /// 行高。
    pub(super) row_height: f32,
    /// 是否包含最上方的“..”行/卡片。
    pub(super) parent_row: bool,
    /// 虚拟行数（包含“..”行）。
    pub(super) row_count: usize,
    /// 平铺列数；详情列表固定为 1。
    pub(super) tile_columns: usize,
    /// 平铺卡片宽度（与渲染使用的动态宽度一致）。
    pub(super) tile_width: f32,
    /// 平铺卡片中心距（宽度 + 间距）。
    pub(super) tile_pitch: f32,
    /// 平铺视图左右内边距。
    pub(super) tile_padding: f32,
    /// 是否为平铺视图。
    pub(super) tile: bool,
}

impl MarqueeLayout {
    /// 判断窗口坐标是否落在滚动视口内，并避开右侧滚动条。
    pub(super) fn inside_viewport(&self, x: f32, y: f32) -> bool {
        x >= self.left
            && x <= self.left + self.width - SCROLLBAR_GUARD
            && y >= self.top
            && y <= self.top + self.height
    }

    /// 内容坐标（相对列表顶部、已计入滚动偏移）。
    fn content_y(&self, y: f32) -> f32 {
        y - self.top - self.scroll_y
    }

    /// 返回坐标对应的虚拟行号。
    fn virtual_row(&self, y: f32) -> usize {
        (self.content_y(y) / self.row_height).floor().max(0.0) as usize
    }

    /// 返回坐标命中的虚拟索引；`None` 表示空白区域。
    ///
    /// 详情列表整行都算命中；平铺视图只在实际存在的卡片矩形内命中。
    pub(super) fn virtual_index_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.inside_viewport(x, y) {
            return None;
        }
        let row = self.virtual_row(y);
        if self.tile {
            let content_x = x - self.left - self.tile_padding;
            if content_x < 0.0 {
                return None;
            }
            let column = (content_x / self.tile_pitch).floor() as usize;
            if column >= self.tile_columns {
                return None;
            }
            // 卡片之间的水平间隙属于空白区域，不命中卡片。
            if content_x - column as f32 * self.tile_pitch >= self.tile_width {
                return None;
            }
            let index = row * self.tile_columns + column;
            (index < self.row_count).then_some(index)
        } else {
            (row < self.row_count).then_some(row)
        }
    }

    /// 判断按下坐标是否位于可开始框选的空白区域。
    pub(super) fn starts_blank(&self, x: f32, y: f32) -> bool {
        self.inside_viewport(x, y) && self.virtual_index_at(x, y).is_none()
    }

    /// 把虚拟索引换算为 `displayed_entries` 下标；“..”行返回 `None`。
    fn entry_index(&self, virtual_index: usize) -> Option<usize> {
        let offset = usize::from(self.parent_row);
        virtual_index
            .checked_sub(offset)
            .filter(|_| virtual_index >= offset)
    }

    /// 返回矩形覆盖的条目下标（按显示顺序）。
    pub(super) fn selected_entries(&self, start: (f32, f32), current: (f32, f32)) -> Vec<usize> {
        let (left, right) = ordered(start.0, current.0);
        let (mut top, mut bottom) = ordered(start.1, current.1);
        // 限制在视口内，避免拖出窗口后选中不可见内容。
        top = top.max(self.top);
        bottom = bottom.min(self.top + self.height);
        let left = left.max(self.left);
        let right = right.min(self.left + self.width - SCROLLBAR_GUARD);
        if bottom < top || right < left {
            return Vec::new();
        }

        let first_row = self.virtual_row(top);
        let last_row = self.virtual_row(bottom);
        let mut entries = Vec::new();
        if self.tile {
            let content_left = left - self.left - self.tile_padding;
            let content_right = right - self.left - self.tile_padding;
            let first_column = (content_left.max(0.0) / self.tile_pitch).floor() as usize;
            let last_column = (content_right.max(0.0) / self.tile_pitch).floor() as usize;
            for row in first_row..=last_row.min(self.row_count.saturating_sub(1)) {
                for column in first_column..=last_column.min(self.tile_columns.saturating_sub(1)) {
                    let index = row * self.tile_columns + column;
                    if index >= self.row_count {
                        continue;
                    }
                    if let Some(entry) = self.entry_index(index) {
                        entries.push(entry);
                    }
                }
            }
        } else {
            for row in first_row..=last_row.min(self.row_count.saturating_sub(1)) {
                if let Some(entry) = self.entry_index(row) {
                    entries.push(entry);
                }
            }
        }
        entries.sort_unstable();
        entries.dedup();
        entries
    }

    /// 计算选框在列表容器内的相对矩形；位移过小时返回 `None`。
    pub(super) fn overlay_rect(
        &self,
        start: (f32, f32),
        current: (f32, f32),
    ) -> Option<(f32, f32, f32, f32)> {
        let (left, right) = ordered(start.0, current.0);
        let (top, bottom) = ordered(start.1, current.1);
        if right - left < MIN_MARQUEE_DRAG && bottom - top < MIN_MARQUEE_DRAG {
            return None;
        }
        let viewport_right = self.left + self.width - SCROLLBAR_GUARD;
        let viewport_bottom = self.top + self.height;
        let left = left.clamp(self.left, viewport_right) - self.left;
        let right = right.clamp(self.left, viewport_right) - self.left;
        let top = top.clamp(self.top, viewport_bottom) - self.top;
        let bottom = bottom.clamp(self.top, viewport_bottom) - self.top;
        Some((left, top, (right - left).max(0.0), (bottom - top).max(0.0)))
    }

    /// 返回自动滚动方向：-1 向上、1 向下、0 不滚动。
    pub(super) fn autoscroll_direction(&self, y: f32) -> i32 {
        if y < self.top + AUTOSCROLL_EDGE {
            -1
        } else if y > self.top + self.height - AUTOSCROLL_EDGE {
            1
        } else {
            0
        }
    }
}

/// 返回排序后的两个浮点数。
fn ordered(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个详情列表布局：视口 100,50 起，高 310（10 行），含“..”行。
    fn list_layout(scroll_y: f32, parent_row: bool) -> MarqueeLayout {
        MarqueeLayout {
            left: 100.0,
            top: 50.0,
            width: 600.0,
            height: 310.0,
            scroll_y,
            row_height: LIST_ROW_HEIGHT,
            parent_row,
            row_count: 10,
            tile_columns: 1,
            tile_width: 600.0,
            tile_pitch: 600.0,
            tile_padding: 0.0,
            tile: false,
        }
    }

    /// 平铺布局：各列宽度均分可用宽度，窄窗口通过减少列数保证最小宽度。
    #[test]
    fn tile_metrics_fill_available_width() {
        for width in [420.0_f32, 700.0, 1024.0, 1600.0] {
            let metrics = TileMetrics::for_width(width, true);
            assert!(metrics.columns >= 1);
            assert!(
                metrics.tile_width >= TILE_MIN_WIDTH,
                "{width}: {:?}",
                metrics
            );
            let used = metrics.padding * 2.0
                + metrics.tile_width * metrics.columns as f32
                + TILE_GAP * metrics.columns.saturating_sub(1) as f32;
            assert!(
                (used - width).abs() < 0.5,
                "{width} 宽应铺满：used={used} metrics={metrics:?}"
            );
        }
        // 极窄宽度至少保留一列，不产生 0 宽度卡片。
        let narrow = TileMetrics::for_width(120.0, true);
        assert_eq!(narrow.columns, 1);
        assert!(narrow.tile_width >= TILE_MIN_WIDTH);
        // 列表视图固定单列并使用列表行高。
        let list = TileMetrics::for_width(640.0, false);
        assert_eq!(list.columns, 1);
        assert_eq!(list.row_height, LIST_ROW_HEIGHT);
    }

    /// 验证详情列表按行命中，滚动偏移参与换算。
    #[test]
    fn list_hit_test_uses_scroll_offset() {
        let layout = list_layout(0.0, true);
        assert_eq!(layout.virtual_index_at(150.0, 60.0), Some(0));
        assert_eq!(layout.virtual_index_at(150.0, 95.0), Some(1));
        assert_eq!(layout.virtual_index_at(150.0, 400.0), None);

        let scrolled = list_layout(-62.0, true);
        assert_eq!(scrolled.virtual_index_at(150.0, 60.0), Some(2));
    }

    /// 验证空白区域才能开始框选，滚动条和行内点击都不开始。
    #[test]
    fn blank_start_excludes_rows_and_scrollbar() {
        let layout = MarqueeLayout {
            row_count: 8,
            ..list_layout(0.0, false)
        };
        assert!(!layout.starts_blank(150.0, 60.0));
        assert!(layout.starts_blank(150.0, 320.0));
        assert!(!layout.starts_blank(695.0, 320.0));
        assert!(!layout.starts_blank(80.0, 60.0));
    }

    /// 验证含“..”行时框选不会把占位行算成条目。
    #[test]
    fn parent_row_is_never_selected() {
        let layout = list_layout(0.0, true);
        let entries = layout.selected_entries((100.0, 50.0), (700.0, 145.0));
        assert_eq!(entries, vec![0, 1, 2]);
    }

    /// 验证向上拖动和部分相交的行也能正确选择。
    #[test]
    fn reverse_drag_selects_intersecting_rows() {
        let layout = list_layout(0.0, false);
        let entries = layout.selected_entries((400.0, 200.0), (150.0, 95.0));
        assert_eq!(entries, vec![1, 2, 3, 4]);
    }

    /// 验证平铺视图按行列命中并跳过“..”卡片。
    #[test]
    fn tile_hit_test_and_selection() {
        let layout = MarqueeLayout {
            left: 100.0,
            top: 50.0,
            width: 600.0,
            height: 264.0,
            scroll_y: 0.0,
            row_height: TILE_ROW_HEIGHT,
            parent_row: true,
            row_count: 7,
            tile_columns: 3,
            tile_width: 120.0,
            tile_pitch: 128.0,
            tile_padding: TILE_PADDING_X,
            tile: true,
        };
        assert_eq!(layout.virtual_index_at(112.0, 60.0), Some(0));
        assert_eq!(layout.virtual_index_at(240.0, 60.0), Some(1));
        assert_eq!(layout.virtual_index_at(234.0, 60.0), None);
        assert_eq!(layout.virtual_index_at(500.0, 60.0), None);
        let entries = layout.selected_entries((100.0, 50.0), (700.0, 400.0));
        assert_eq!(entries, vec![0, 1, 2, 3, 4, 5]);
    }

    /// 验证选框相对视口裁切，过小位移不绘制。
    #[test]
    fn overlay_rect_is_clamped() {
        let layout = list_layout(0.0, false);
        assert!(layout.overlay_rect((150.0, 60.0), (152.0, 61.0)).is_none());
        let rect = layout.overlay_rect((200.0, 100.0), (50.0, 30.0));
        assert_eq!(rect, Some((0.0, 0.0, 100.0, 50.0)));
    }

    /// 验证边缘自动滚动方向。
    #[test]
    fn autoscroll_direction_checks_edges() {
        let layout = list_layout(0.0, false);
        assert_eq!(layout.autoscroll_direction(55.0), -1);
        assert_eq!(layout.autoscroll_direction(200.0), 0);
        assert_eq!(layout.autoscroll_direction(355.0), 1);
    }
}
