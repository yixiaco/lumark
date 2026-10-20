use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct SelectionModel {
    selected: BTreeSet<String>,
    anchor: Option<String>,
    focus: Option<String>,
}

impl SelectionModel {
    pub fn selected_ids(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    pub fn select_only(&mut self, id: impl Into<String>) {
        let id = id.into();
        self.selected.clear();
        self.selected.insert(id.clone());
        self.anchor = Some(id.clone());
        self.focus = Some(id);
    }

    pub fn toggle(&mut self, id: impl Into<String>) {
        let id = id.into();
        if !self.selected.remove(&id) {
            self.selected.insert(id.clone());
        }
        self.anchor = Some(id.clone());
        self.focus = Some(id);
    }

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

    pub fn select_all(&mut self, ordered_ids: &[String]) {
        self.selected = ordered_ids.iter().cloned().collect();
        self.focus = ordered_ids.last().cloned();
        self.anchor = ordered_ids.first().cloned();
    }

    pub fn focus(&self) -> Option<&str> {
        self.focus.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn ctrl_toggle() {
        let mut selection = SelectionModel::default();
        selection.toggle("a");
        selection.toggle("b");
        assert_eq!(selection.count(), 2);
        selection.toggle("a");
        assert_eq!(selection.count(), 1);
    }
}
