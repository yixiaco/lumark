//! 设置窗口压缩选项的下拉状态：选项表、选中同步和确认事件。
use super::super::*;
use super::settings_window::SettingsWindowView;
use gpui_kit::Entity;
use gpui_kit::Subscription;
use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use lumark_settings::OverwritePolicy;

/// 设置窗口里压缩选项共用的下拉状态。
pub(super) type CompressionSelect = Entity<SelectState<SearchableVec<&'static str>>>;

/// 默认格式下拉项：第一项写入设置，第二项显示给用户。
const FORMAT_OPTIONS: [(&str, &str); 11] = [
    ("zip", "ZIP"),
    ("7z", "7Z"),
    ("tar", "TAR"),
    ("tar.gz", "TAR.GZ"),
    ("tar.bz2", "TAR.BZ2"),
    ("tar.xz", "TAR.XZ"),
    ("tar.zst", "TAR.ZST"),
    ("gz", "GZ"),
    ("bz2", "BZ2"),
    ("xz", "XZ"),
    ("zst", "ZST"),
];

/// 压缩预设下拉项，顺序与设置枚举保持一致。
const PRESET_OPTIONS: [(SettingsPreset, &str); 3] = [
    (SettingsPreset::Speed, "速度优先"),
    (SettingsPreset::Balanced, "平衡"),
    (SettingsPreset::Size, "体积优先"),
];

/// 覆盖策略下拉项，包含询问、覆盖、跳过和重命名。
const OVERWRITE_OPTIONS: [(OverwritePolicy, &str); 4] = [
    (OverwritePolicy::Ask, "询问"),
    (OverwritePolicy::Overwrite, "覆盖"),
    (OverwritePolicy::Skip, "跳过"),
    (OverwritePolicy::Rename, "重命名"),
];

impl SettingsWindowView {
    /// 按当前设置创建三个下拉状态，并订阅用户的确认操作。
    pub(super) fn new_compression_selects(
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        CompressionSelect,
        CompressionSelect,
        CompressionSelect,
        Vec<Subscription>,
    ) {
        let format_select = new_select(
            window,
            cx,
            FORMAT_OPTIONS.iter().map(|(_, label)| *label).collect(),
            format_index(&settings.default_format),
        );
        let preset_select = new_select(
            window,
            cx,
            PRESET_OPTIONS.iter().map(|(_, label)| *label).collect(),
            preset_index(settings.default_preset),
        );
        let overwrite_select = new_select(
            window,
            cx,
            OVERWRITE_OPTIONS.iter().map(|(_, label)| *label).collect(),
            overwrite_index(settings.overwrite_policy),
        );
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(
            &format_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some((format, _)) =
                        FORMAT_OPTIONS.iter().find(|(_, option)| *option == *label)
                {
                    this.update_app(cx, |app, _| app.set_default_format(format));
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            &preset_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some((preset, _)) =
                        PRESET_OPTIONS.iter().find(|(_, option)| *option == *label)
                {
                    this.update_app(cx, |app, _| app.set_default_preset(*preset));
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            &overwrite_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event
                    && let Some((policy, _)) = OVERWRITE_OPTIONS
                        .iter()
                        .find(|(_, option)| *option == *label)
                {
                    this.update_app(cx, |app, _| app.set_overwrite_policy(*policy));
                }
            },
        ));
        (
            format_select,
            preset_select,
            overwrite_select,
            subscriptions,
        )
    }

    /// 设置值被外部重置后，把三个下拉的选中项同步回来。
    pub(super) fn sync_compression_selects(&self, window: &mut Window, cx: &mut Context<Self>) {
        let format = format_index(&self.settings.default_format);
        let preset = preset_index(self.settings.default_preset);
        let overwrite = overwrite_index(self.settings.overwrite_policy);
        self.format_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(format)), window, cx);
        });
        self.preset_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(preset)), window, cx);
        });
        self.overwrite_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(overwrite)), window, cx);
        });
    }
}

/// 创建带默认选中项的下拉状态。
fn new_select(
    window: &mut Window,
    cx: &mut Context<SettingsWindowView>,
    labels: Vec<&'static str>,
    index: usize,
) -> CompressionSelect {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(labels),
            Some(IndexPath::new(index)),
            window,
            cx,
        )
    })
}

/// 查找默认格式在选项表中的位置，未知值回落到第一项。
fn format_index(format: &str) -> usize {
    FORMAT_OPTIONS
        .iter()
        .position(|(value, _)| value.eq_ignore_ascii_case(format))
        .unwrap_or(0)
}

/// 查找压缩预设的位置，未知值回落到第一项。
fn preset_index(preset: SettingsPreset) -> usize {
    PRESET_OPTIONS
        .iter()
        .position(|(value, _)| *value == preset)
        .unwrap_or(0)
}

/// 查找覆盖策略的位置，未知值回落到第一项。
fn overwrite_index(policy: OverwritePolicy) -> usize {
    OVERWRITE_OPTIONS
        .iter()
        .position(|(value, _)| *value == policy)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LumarkApp;
    use crate::app::view::settings_window::SettingsWindowSnapshot;
    use gpui_kit::test::TestWindowExt;
    use tempfile::tempdir;

    /// 验证设置中的默认值和常用格式都能定位到对应下拉项。
    #[test]
    fn option_tables_map_saved_settings() {
        assert_eq!(format_index("zip"), 0);
        assert_eq!(format_index("7Z"), 1);
        assert_eq!(format_index("tar.zst"), 6);
        assert_eq!(format_index("unknown-format"), 0);
        assert_eq!(preset_index(SettingsPreset::Balanced), 1);
        assert_eq!(overwrite_index(OverwritePolicy::Rename), 3);
    }

    #[gpui_kit::test]
    /// 验证设置窗口的压缩选项卡能创建并渲染三个下拉选择。
    fn settings_window_renders_compression_selects(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let directory = tempdir().unwrap();
        let settings_path = directory.path().join("settings.json");
        let vault_path = directory.path().join("vault.dat");
        let app = {
            let (app, _first_window_cx) = cx.add_window_view(|window, cx| {
                LumarkApp::new_with_stores(
                    window,
                    cx,
                    vault_path.clone(),
                    settings_path.clone(),
                    Some(directory.path().to_path_buf()),
                )
            });
            app
        };
        let snapshot = SettingsWindowSnapshot {
            settings: Settings::default(),
            settings_path,
            vault_path,
            vault_state: VaultState::Unlocked,
            vault_has_master: false,
            vault_error: None,
            file_association_count: 0,
            selected_formats: HashSet::new(),
            status: "就绪".to_string(),
        };
        let (view, window_cx) = cx.add_window_view(|window, cx| {
            SettingsWindowView::new(app.clone(), snapshot.clone(), window, cx)
        });
        view.update(window_cx, |view, cx| {
            view.tab = 2;
            cx.notify();
        });
        window_cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            for id in [
                "settings-format-select",
                "settings-preset-select",
                "settings-overwrite-select",
            ] {
                assert!(window.find(id).visible(), "设置下拉未渲染：{id}");
            }
        });
        let _ = app;
    }
}
