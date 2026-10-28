//! 主文件表、搜索结果条和侧栏详情检查器入口。

use super::super::actions::TempExtractMode;
use super::super::*;
use super::components::{AppIcon, entry_icon, entry_icon_color, hint_text, icon_button};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;

use columns::*;
use drag::*;
use menu::*;
use rows::*;
use tiles::*;

mod columns;
mod details;
mod drag;
mod menu;
mod rows;
mod tiles;

impl LumarkApp {
    /// 渲染主文件表，并在搜索结果激活时显示一条结果摘要。
    ///
    /// 选择按稳定条目 ID 跟踪，而不是行索引，因此排序、过滤和虚拟列表回收
    /// 都不会让选择静默跳到其他文件。详细信息已移到侧栏下半部分，
    /// 文件表因此可以使用窗口的全部剩余宽度。
    pub(crate) fn render_browser(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entry_count = self.displayed_count();
        let search_active = self.search_results.is_some();
        let show_parent = shows_parent_row(self);
        let row_count = entry_count + usize::from(show_parent);
        let window_width = window
            .inner_window_bounds()
            .get_bounds()
            .size
            .width
            .as_f32();
        let sidebar_width = if self.sidebar_visible { 252. } else { 0. };
        let main_width = (window_width - sidebar_width - 24.).max(420.);
        let (size_width, modified_width, kind_width) = column_layout_widths(self.column_layout);
        let inside_archive = matches!(&self.browser.location, BrowserLocation::Archive { .. });
        let archive_columns = if inside_archive {
            archive_columns(main_width)
        } else {
            ArchiveColumns::default()
        };
        let tile_view = self.tile_view;
        let tile_columns = if tile_view {
            ((main_width - 24.0) / 132.0).floor().max(1.0) as usize
        } else {
            1
        };
        let virtual_rows = if tile_view {
            row_count.div_ceil(tile_columns)
        } else {
            row_count
        };
        let row_height = if tile_view { px(132.) } else { px(31.) };
        let item_sizes = Rc::new(vec![
            gpui_kit::size(px(main_width), row_height);
            virtual_rows
        ]);
        let total_size = self
            .displayed_entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();

        v_flex()
            .id("browser-page")
            .w_full()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(cx.theme().table)
            .track_focus(&self.browser_focus)
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if keystroke.modifiers.control && keystroke.key.eq_ignore_ascii_case("a") {
                        this.select_all_displayed();
                        cx.notify();
                    } else if keystroke.modifiers.control && keystroke.key.eq_ignore_ascii_case("c")
                    {
                        if this.browser_focus.is_focused(window) {
                            this.copy_selected_items(cx);
                        }
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("escape") {
                        if search_active {
                            this.clear_search();
                        } else {
                            this.browser.selection.clear();
                        }
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("enter") {
                        this.open_selected(window, cx);
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("delete") {
                        this.request_delete();
                        cx.notify();
                    } else if keystroke.key.eq_ignore_ascii_case("f2") {
                        this.start_rename(window, cx);
                        cx.notify();
                    }
                }),
            )
            .on_drop(
                cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| {
                    let paths = paths.0.to_vec();
                    this.handle_external_drop(&paths, window, cx);
                    cx.notify();
                }),
            )
            .flex_1()
            .when(search_active, |this| {
                this.child(
                    h_flex()
                        .h(px(30.))
                        .w_full()
                        .flex_shrink_0()
                        .px_3()
                        .gap_2()
                        .items_center()
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .text_xs()
                        .child(AppIcon::Search.element(13.))
                        .child(format!(
                            "搜索结果 · {entry_count} 项 · {}",
                            format_size(total_size)
                        ))
                        .child(
                            button("search-result-clear")
                                .link()
                                .xsmall()
                                .ml_auto()
                                .label("清除搜索")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_search();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .h(px(30.))
                    .flex_shrink_0()
                    .gap_2()
                    .px_3()
                    .items_center()
                    .bg(cx.theme().list_head)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(22.)).flex_shrink_0())
                    .child(if tile_view {
                        div().flex_1().child("名称与类型").into_any_element()
                    } else {
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child("名称"))
                            .child(div().w(px(size_width)).child("大小"))
                            .when(archive_columns.compressed, |this| {
                                this.child(div().w(px(COMPRESSED_WIDTH)).child("压缩后大小"))
                            })
                            .child(div().w(px(modified_width)).child("修改时间"))
                            .when(archive_columns.algorithm, |this| {
                                this.child(div().w(px(ALGORITHM_WIDTH)).child("压缩算法"))
                            })
                            .when(archive_columns.crc32, |this| {
                                this.child(div().w(px(CRC_WIDTH)).child("CRC32"))
                            })
                            .when(archive_columns.path, |this| {
                                this.child(div().w(px(PATH_WIDTH)).child("路径"))
                            })
                            .when(!inside_archive, |this| {
                                this.child(div().w(px(kind_width)).child("类型"))
                            })
                            .into_any_element()
                    }),
            )
            .when(entry_count == 0, |this| {
                this.when(show_parent, |this| {
                    let parent = if tile_view {
                        h_flex()
                            .w_full()
                            .px_3()
                            .child(render_parent_tile(cx))
                            .into_any_element()
                    } else {
                        render_parent_row(
                            cx,
                            row_height,
                            size_width,
                            modified_width,
                            kind_width,
                            archive_columns,
                            inside_archive,
                        )
                        .into_any_element()
                    };
                    this.child(parent)
                })
                .child(empty_state(cx, search_active))
            })
            .when(entry_count > 0, |this| {
                let scroll = self.file_list_scroll.clone();
                this.child(
                    v_flex()
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .relative()
                        .overflow_hidden()
                        .child(
                            gpui_kit::base::v_virtual_list(
                                cx.entity(),
                                "file-list",
                                item_sizes,
                                move |app, range, _window, cx| {
                                    range
                                        .map(|virtual_index| {
                                            if tile_view {
                                                render_tile_row(
                                                    app,
                                                    cx,
                                                    virtual_index,
                                                    tile_columns,
                                                    row_height,
                                                )
                                                .into_any_element()
                                            } else {
                                                render_file_row(
                                                    app,
                                                    cx,
                                                    virtual_index,
                                                    row_height,
                                                    size_width,
                                                    modified_width,
                                                    kind_width,
                                                    archive_columns,
                                                    inside_archive,
                                                )
                                                .into_any_element()
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                },
                            )
                            .track_scroll(&scroll),
                        )
                        .vertical_scrollbar(&scroll),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::columns::archive_columns;
    use super::drag::ArchiveDragItem;
    use super::menu::{FileMenuKind, file_menu_kind};
    use lumark_archive::ArchiveEntry;
    use lumark_browser::{BrowserEntry, BrowserLocation};
    use std::path::PathBuf;

    /// 验证归档列随宽度收窄，窄窗口先隐藏路径和 CRC32。
    #[test]
    fn archive_columns_shrink_with_width() {
        let wide = archive_columns(1200.);
        assert!(wide.compressed && wide.algorithm && wide.crc32 && wide.path);
        let medium = archive_columns(700.);
        assert!(medium.compressed && medium.algorithm && medium.crc32);
        assert!(!medium.path);
        let narrow = archive_columns(620.);
        assert!(narrow.compressed && narrow.crc32);
        assert!(!narrow.algorithm && !narrow.path);
        let tiny = archive_columns(500.);
        assert!(!tiny.compressed && !tiny.algorithm && !tiny.crc32 && !tiny.path);
    }

    /// 验证归档内、文件系统和压缩包文件使用不同菜单。
    #[test]
    fn menu_kind_depends_on_location_and_entry_type() {
        let inside = BrowserLocation::Archive {
            archive: PathBuf::from("demo.zip"),
            inner: String::new(),
        };
        let outside = BrowserLocation::FileSystem(PathBuf::from("C:/downloads"));
        let folder = entry("folder", true, None);
        let archive_file = entry("demo.zip", false, None);
        let plain = entry("notes.txt", false, None);
        let inside_file = entry("docs/readme.txt", false, Some("deflate"));
        let executable = entry("setup.exe", false, None);
        let iso = entry("image.iso", false, None);

        assert_eq!(
            file_menu_kind(&inside, &folder),
            FileMenuKind::ArchiveFolder
        );
        assert_eq!(
            file_menu_kind(&inside, &inside_file),
            FileMenuKind::ArchiveFile
        );
        assert_eq!(file_menu_kind(&outside, &folder), FileMenuKind::Folder);
        assert_eq!(
            file_menu_kind(&outside, &archive_file),
            FileMenuKind::ArchiveFileSystem
        );
        assert_eq!(file_menu_kind(&outside, &iso), FileMenuKind::IsoFile);
        assert_eq!(file_menu_kind(&outside, &plain), FileMenuKind::File);
        assert_eq!(
            file_menu_kind(&outside, &executable),
            FileMenuKind::ExecutableFile
        );
    }

    /// 伪装归档在文件系统里必须归类为可打开归档，菜单才提供打开/解压/还原入口。
    #[test]
    fn disguised_archive_is_classified_as_archive_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("hello.txt");
        std::fs::write(&source, b"hello").unwrap();
        let output = directory.path().join("cover.jpg");
        let options = lumark_archive::CompressionOptions {
            disguise: Some(lumark_archive::DisguiseOptions {
                mask: lumark_archive::DisguiseMask::PresetImage,
                original_name: None,
            }),
            ..Default::default()
        };
        lumark_archive::ArchiveService::create(&output, &[source], &options, |_| {}).unwrap();
        let disguised = BrowserEntry {
            id: "fs:cover.jpg".to_string(),
            name: "cover.jpg".to_string(),
            path: output,
            is_dir: false,
            size: 0,
            modified: None,
            kind: String::new(),
            archive_entry: None,
            encrypted: false,
        };
        assert_eq!(
            file_menu_kind(
                &BrowserLocation::FileSystem(directory.path().to_path_buf()),
                &disguised
            ),
            FileMenuKind::ArchiveFileSystem
        );
    }

    /// 构造一个只填充菜单判定所需字段的条目。
    fn entry(name: &str, is_dir: bool, method: Option<&str>) -> BrowserEntry {
        BrowserEntry {
            id: format!("archive:{name}"),
            name: name.rsplit('/').next().unwrap_or(name).to_string(),
            path: PathBuf::from(name),
            is_dir,
            size: 0,
            modified: None,
            kind: method.unwrap_or("").to_string(),
            archive_entry: method.map(|method| ArchiveEntry {
                id: 0,
                path: name.to_string(),
                is_dir,
                size: 0,
                compressed_size: Some(0),
                modified: None,
                crc32: Some(0),
                method: method.to_string(),
                encrypted: false,
            }),
            encrypted: false,
        }
    }

    /// 验证拖出归档目录时会先落盘临时文件，并把真实路径交给原生拖放。
    #[test]
    fn archive_drag_payload_extracts_selected_files() {
        use lumark_archive::{ArchiveService, CompressionOptions};
        use tempfile::tempdir;

        let directory = tempdir().unwrap();
        let source = directory.path().join("docs");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("readme.txt"), b"hello").unwrap();
        let archive = directory.path().join("docs.zip");
        ArchiveService::create(
            &archive,
            &[source],
            &CompressionOptions {
                format: lumark_archive::ArchiveFormat::Zip,
                path_mode: lumark_archive::PathMode::Relative,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        let item = ArchiveDragItem {
            archive,
            password: None,
            selected: vec![("docs".to_string(), true)],
            output: directory.path().join("drag"),
            label: "docs".to_string(),
        };
        let payload = item.materialize_paths().expect("拖出数据应可生成");
        assert_eq!(payload.len(), 1);
        assert!(payload[0].exists());
    }
}
