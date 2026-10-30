//! 文件表多选、范围选择和锚点状态。

use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
/// 按稳定条目 ID 维护的文件表选择状态。
pub struct SelectionModel {
    selected: BTreeSet<String>,
    anchor: Option<String>,
    focus: Option<String>,
}

impl SelectionModel {
    /// 返回当前选中的条目 ID。
    pub fn selected_ids(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    /// 判断条目是否被选中。
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    /// 返回选中数量。
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// 清空选择。
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// 只选中指定条目并重置锚点。
    pub fn select_only(&mut self, id: impl Into<String>) {
        let id = id.into();
        self.selected.clear();
        self.selected.insert(id.clone());
        self.anchor = Some(id.clone());
        self.focus = Some(id);
    }

    /// 切换单个条目的选中状态。
    pub fn toggle(&mut self, id: impl Into<String>) {
        let id = id.into();
        if !self.selected.remove(&id) {
            self.selected.insert(id.clone());
        }
        self.anchor = Some(id.clone());
        self.focus = Some(id);
    }

    /// 按锚点到目标的范围选择。
    pub fn select_range(&mut self, ordered_ids: &[String], target: &str, additive: bool) {
        let Some(target_index) = ordered_ids.iter().position(|id| id == target) else {
            return;
        };
        let anchor_index = self
            .anchor
            .as_ref()
            .and_then(|anchor| ordered_ids.iter().position(|id| id == anchor))
            .unwrap_or(target_index);
        if !additive {
            self.selected.clear();
        }
        let (start, end) = if anchor_index <= target_index {
            (anchor_index, target_index)
        } else {
            (target_index, anchor_index)
        };
        for id in &ordered_ids[start..=end] {
            self.selected.insert(id.clone());
        }
        self.focus = Some(target.to_string());
    }

    /// 选中给定范围内的全部条目。
    pub fn select_all(&mut self, ordered_ids: &[String]) {
        self.selected = ordered_ids.iter().cloned().collect();
        self.focus = ordered_ids.last().cloned();
        self.anchor = ordered_ids.first().cloned();
    }

    /// 用一组条目 ID 替换或追加选择。
    ///
    /// 框选会一次性产生一组 ID，因此不能继续用逐条 `toggle`；这里保持
    /// “锚点=首个新条目、焦点=最后新条目”的约定，空集合在替换模式下
    /// 清空选择并重置锚点。
    pub fn select_ids(&mut self, ids: impl IntoIterator<Item = String>, additive: bool) {
        let ids: Vec<String> = ids.into_iter().collect();
        if !additive {
            self.selected.clear();
            self.anchor = None;
            self.focus = None;
        }
        if ids.is_empty() {
            return;
        }
        if !additive {
            self.anchor = ids.first().cloned();
        }
        self.focus = ids.last().cloned();
        self.selected.extend(ids);
    }

    /// 记录键盘焦点条目和选择锚点。
    pub fn focus(&self) -> Option<&str> {
        self.focus.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证范围选择保留锚点。
    fn range_selection_keeps_anchor() {
        let ids = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let mut selection = SelectionModel::default();
        selection.select_only("b");
        selection.select_range(&ids, "d", false);
        assert_eq!(selection.count(), 3);
        assert!(selection.is_selected("c"));
        assert!(!selection.is_selected("a"));
    }

    #[test]
    /// 验证 Ctrl 切换不会清除其他选择。
    fn ctrl_toggle() {
        let mut selection = SelectionModel::default();
        selection.toggle("a");
        selection.toggle("b");
        assert_eq!(selection.count(), 2);
        selection.toggle("a");
        assert_eq!(selection.count(), 1);
    }

    /// 验证框选替换、追加和空集合清空三种语义。
    #[test]
    fn select_ids_replaces_or_appends() {
        let mut selection = SelectionModel::default();
        selection.select_ids(vec!["a".into(), "b".into()], false);
        assert_eq!(
            selection.selected_ids(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(selection.focus(), Some("b"));

        selection.select_ids(vec!["c".into()], true);
        assert_eq!(
            selection.selected_ids(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(selection.focus(), Some("c"));

        selection.select_ids(Vec::new(), false);
        assert_eq!(selection.count(), 0);
        assert!(selection.focus().is_none());
    }
}
