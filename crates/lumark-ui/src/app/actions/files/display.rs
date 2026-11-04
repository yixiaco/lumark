//! 文件表显示视图：过滤顺序、条目摘要与选中条目遍历。
//!
//! 渲染层每帧只调用一次 [`LumarkApp::displayed_order`] / [`LumarkApp::displayed_summary`]，
//! 行渲染按 `source_index` O(1) 取条目；动作层需要完整列表时才使用
//! [`LumarkApp::displayed_entries`]（会克隆整表，禁止在渲染路径调用）。

use super::*;

impl LumarkApp {
    /// 当前位置是否会隐藏 UE 包的伴随文件。
    ///
    /// 只有游戏容器（`game_session` + 归档位置）才隐藏 `.uexp/.ubulk/.uptnl`；
    /// 普通归档浏览保留全部条目，避免影响既有解压/编辑操作。
    fn hides_game_sidecar(&self) -> bool {
        self.game_session.is_some()
            && matches!(&self.browser.location, BrowserLocation::Archive { .. })
    }

    /// 判断游戏容器内的伴随文件是否应隐藏。
    fn hide_game_sidecar(&self, entry: &BrowserEntry) -> bool {
        self.hides_game_sidecar() && lumark_browser::is_ue_sidecar(&entry.name)
    }

    /// 当前显示条目的底层表：搜索结果优先，否则是文件表。
    pub(crate) fn displayed_source(&self) -> &[BrowserEntry] {
        self.search_results
            .as_deref()
            .unwrap_or(&self.browser.entries)
    }

    /// 按显示顺序遍历条目；游戏容器内跳过包伴随文件。
    pub(crate) fn displayed_iter(&self) -> impl Iterator<Item = &BrowserEntry> + '_ {
        self.displayed_source()
            .iter()
            .filter(|entry| !self.hide_game_sidecar(entry))
    }

    /// 构建显示下标 → 源下标的映射；无伴随过滤时零分配。
    pub(crate) fn displayed_order(&self) -> DisplayedOrder {
        let source = self.displayed_source();
        if !self.hides_game_sidecar() {
            return DisplayedOrder::Direct(source.len());
        }
        DisplayedOrder::from_source(source.len(), |index| {
            source
                .get(index)
                .is_some_and(|entry| !self.hide_game_sidecar(entry))
        })
    }

    /// 一趟遍历得到条目数、总大小与压缩后总大小。
    pub(crate) fn displayed_summary(&self) -> DisplayedSummary {
        let mut summary = DisplayedSummary::default();
        for entry in self.displayed_iter() {
            summary.count += 1;
            summary.total_size = summary.total_size.saturating_add(entry.size);
            if let Some(compressed) = entry
                .archive_entry
                .as_ref()
                .and_then(|archive| archive.compressed_size)
            {
                summary.compressed_size = Some(
                    summary
                        .compressed_size
                        .unwrap_or_default()
                        .saturating_add(compressed),
                );
            }
        }
        summary
    }

    /// 按稳定 ID 查找当前显示条目；不克隆整表。
    pub(crate) fn displayed_entry_by_id(&self, id: &str) -> Option<&BrowserEntry> {
        self.displayed_iter().find(|entry| entry.id == id)
    }

    /// 返回当前显示条目（整表克隆）；仅供测试与诊断脚本使用。
    ///
    /// 渲染回调请改用 [`Self::displayed_order`] + [`Self::displayed_source`]，
    /// 动作层请优先用 [`Self::displayed_iter`]，避免为几万条目分配内存。
    #[cfg(test)]
    pub(crate) fn displayed_entries(&self) -> Vec<BrowserEntry> {
        self.displayed_iter().cloned().collect()
    }

    /// 当前显示条目数量。
    pub(crate) fn displayed_count(&self) -> usize {
        self.displayed_iter().count()
    }

    /// 按显示顺序返回用于范围选择的稳定 ID。
    pub(crate) fn displayed_ids(&self) -> Vec<String> {
        self.displayed_iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// 按显示顺序返回当前选中的条目（动作层一次性使用）。
    pub(crate) fn selected_entries(&self) -> Vec<BrowserEntry> {
        let selection = &self.browser.selection;
        self.displayed_iter()
            .filter(|entry| selection.is_selected(&entry.id))
            .cloned()
            .collect()
    }

    /// 选择当前过滤视图中的全部项目。
    pub(crate) fn select_all_displayed(&mut self) {
        let ids = self.displayed_ids();
        self.browser.selection.select_all(&ids);
    }
}
