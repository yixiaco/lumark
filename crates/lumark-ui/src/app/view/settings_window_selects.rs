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
const FORMAT_OPTIONS: [(&str, &str); 19] = [
    ("zip", "ZIP"),
    ("7z", "7Z"),
    ("zpaq", "ZPAQ"),
    ("lzh", "LZH"),
    ("tar", "TAR"),
    ("tar.gz", "TAR.GZ"),
    ("tar.bz2", "TAR.BZ2"),
    ("tar.xz", "TAR.XZ"),
    ("tar.zst", "TAR.ZST"),
    ("tar.br", "TAR.BR"),
    ("tar.lz4", "TAR.LZ4"),
    ("tar.lz", "TAR.LZ"),
    ("tar.lzma", "TAR.LZMA"),
    ("gz", "GZ"),
    ("bz2", "BZ2"),
    ("xz", "XZ"),
    ("zst", "ZST"),
    ("br", "BROTLI"),
    ("lz4", "LZ4"),
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
    /// 按当前设置创建四个下拉状态，并订阅用户的确认操作。
    pub(super) fn new_compression_selects(
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        CompressionSelect,
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
        let disguise_select = new_select(
            window,
            cx,
            DISGUISE_MASK_LABELS.to_vec(),
            disguise_mask_index(settings.disguise_mask),
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
        subscriptions.push(cx.subscribe(
            &disguise_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(label)) = event {
                    this.update_app(cx, |app, _| {
                        app.set_disguise_mask(disguise_mask_from_label(label));
                    });
                }
            },
        ));
        (
            format_select,
            preset_select,
            overwrite_select,
            disguise_select,
            subscriptions,
        )
    }

    /// 设置值被外部重置后，把四个下拉的选中项同步回来。
    pub(super) fn sync_compression_selects(&self, window: &mut Window, cx: &mut Context<Self>) {
        let format = format_index(&self.settings.default_format);
        let preset = preset_index(self.settings.default_preset);
        let overwrite = overwrite_index(self.settings.overwrite_policy);
        let disguise = disguise_mask_index(self.settings.disguise_mask);
        self.format_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(format)), window, cx);
        });
        self.preset_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(preset)), window, cx);
        });
        self.overwrite_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(overwrite)), window, cx);
        });
        self.disguise_select.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(disguise)), window, cx);
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
        assert_eq!(format_index("zpaq"), 2);
        assert_eq!(format_index("lzh"), 3);
        assert_eq!(format_index("tar.zst"), 8);
        assert_eq!(format_index("tar.br"), 9);
        assert_eq!(format_index("tar.lz4"), 10);
        assert_eq!(format_index("tar.lz"), 11);
        assert_eq!(format_index("tar.lzma"), 12);
        assert_eq!(format_index("gz"), 13);
        assert_eq!(format_index("br"), 17);
        assert_eq!(format_index("lz4"), 18);
        assert_eq!(format_index("unknown-format"), 0);
        assert_eq!(preset_index(SettingsPreset::Balanced), 1);
        assert_eq!(overwrite_index(OverwritePolicy::Rename), 3);
    }

    /// 设置里的每个默认格式都必须映射回可写格式，且扩展名一致。
    #[test]
    fn format_options_map_to_writable_formats() {
        for (value, _) in FORMAT_OPTIONS {
            let settings = lumark_settings::Settings {
                default_format: value.to_string(),
                ..Default::default()
            };
            let format = crate::app::util::format_from_settings(&settings);
            assert!(format.is_writable(), "{value} 未映射到可写格式：{format:?}");
            assert_eq!(format.extension(), value, "{value} 与扩展名不一致");
        }
    }

    /// 验证设置窗口的压缩选项卡能创建并渲染三个下拉选择。
    #[gpui_kit::test]
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
                let select = window.find(id);
                assert!(select.visible(), "设置下拉未渲染：{id}");
                assert!(
                    (select.bounds().size.width - px(160.)).abs() < px(1.),
                    "设置下拉宽度不正确：{id} {:?}",
                    select.bounds().size.width
                );
                assert!(
                    select.bounds().origin.x > px(400.),
                    "设置下拉挤压了左侧说明：{id} {:?}",
                    select.bounds().origin.x
                );
            }

            let tab = window.find(("settings-window-tab", 0usize));
            assert!(tab.visible(), "设置选项卡未渲染");
            assert!(
                (tab.bounds().size.height - px(32.)).abs() < px(1.),
                "设置选项卡高度不正确：{:?}",
                tab.bounds().size.height
            );

            window.click(("settings-window-tab", 3usize), cx);
            assert_eq!(view.read(cx).tab, 3, "点击选项卡后未切换页面");
        });
        let _ = app;
    }
}
