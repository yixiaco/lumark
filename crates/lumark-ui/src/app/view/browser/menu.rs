//! 文件表右键菜单和菜单动作。

use super::*;
use crate::i18n::tr;

/// 右键菜单按位置和项目类型区分的动作集合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileMenuKind {
    /// 归档内的文件夹。
    ArchiveFolder,
    /// 归档内的文件。
    ArchiveFile,
    /// 文件系统中的文件夹。
    Folder,
    /// 文件系统中的压缩包。
    ArchiveFileSystem,
    /// 文件系统中的 ISO 光盘映像。
    IsoFile,
    /// 文件系统中的可执行文件。
    ExecutableFile,
    /// 文件系统中的 PE 可执行文件，作为只读归档浏览。
    PeFile,
    /// 文件系统中的普通文件。
    File,
}
/// 根据当前位置和条目类型选择右键菜单。
pub(super) fn file_menu_kind(location: &BrowserLocation, entry: &BrowserEntry) -> FileMenuKind {
    match location {
        BrowserLocation::Archive { .. } => {
            if entry.is_dir {
                FileMenuKind::ArchiveFolder
            } else {
                FileMenuKind::ArchiveFile
            }
        }
        BrowserLocation::FileSystem(_) => {
            if entry.is_dir {
                FileMenuKind::Folder
            } else if is_iso_path(&entry.path) {
                FileMenuKind::IsoFile
            } else {
                match ArchiveService::detect(&entry.path) {
                    ArchiveFormat::Pe => FileMenuKind::PeFile,
                    ArchiveFormat::Unknown if is_executable_like(&entry.path) => {
                        FileMenuKind::ExecutableFile
                    }
                    ArchiveFormat::Unknown => FileMenuKind::File,
                    _ => FileMenuKind::ArchiveFileSystem,
                }
            }
        }
    }
}
/// 构建文件行使用的官方右键菜单。
///
/// 菜单项通过 `PopupMenuItem::on_click` 直接调用应用动作，避免再维护一套
/// 自绘菜单和透明关闭层；菜单的定位、键盘导航、分隔线和阴影由组件库负责。
/// 归档内和文件系统里的条目拥有不同的动作集合：归档内不能新建文件夹、
/// 也不提供新窗口入口；文件系统内的压缩包才提供解压动作。
pub(super) fn file_context_menu(
    app: gpui_kit::Entity<LumarkApp>,
    location: BrowserLocation,
    entry: BrowserEntry,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, _| {
        let kind = file_menu_kind(&location, &entry);
        // 伪装探测只在菜单真正打开时执行；文件行渲染不再逐帧读取文件尾。
        let disguised = !entry.is_dir
            && matches!(location, BrowserLocation::FileSystem(_))
            && ArchiveService::probe_disguise(&entry.path)
                .ok()
                .flatten()
                .is_some();
        match kind {
            FileMenuKind::ArchiveFolder | FileMenuKind::ArchiveFile => {
                let icon = if kind == FileMenuKind::ArchiveFolder {
                    AppIcon::FolderOpen
                } else {
                    AppIcon::Doc
                };
                let open_label =
                    if kind == FileMenuKind::ArchiveFile && is_nested_archive_name(&entry.name) {
                        tr("在新窗口中打开压缩包", "Open archive in new window")
                    } else {
                        tr("打开", "Open")
                    };
                menu.item(popup_action(
                    open_label,
                    icon,
                    &app,
                    window,
                    |app, window, cx| app.open_selected(window, cx),
                ))
                .item(popup_action(
                    tr("使用内置查看器打开", "Open in built-in viewer"),
                    AppIcon::Doc,
                    &app,
                    window,
                    |app, window, cx| app.open_selected_in_viewer(window, cx),
                ))
                .item(popup_action(
                    tr("解压到…", "Extract to..."),
                    AppIcon::Zip,
                    &app,
                    window,
                    |app, window, cx| app.open_extract_dialog(window, cx),
                ))
                .separator()
                .item(popup_action(
                    tr("复制文件", "Copy files"),
                    AppIcon::Copy,
                    &app,
                    window,
                    |app, _, cx| app.copy_selected_items(cx),
                ))
                .item(popup_action(
                    tr("复制路径", "Copy path"),
                    AppIcon::Copy,
                    &app,
                    window,
                    |app, _, cx| app.copy_selected_path(cx),
                ))
                .item(popup_action(
                    tr("打开压缩包所在目录", "Open archive folder"),
                    AppIcon::Search,
                    &app,
                    window,
                    |app, _, _| app.reveal_selected(),
                ))
                .separator()
                .item(popup_action(
                    tr("重命名", "Rename"),
                    AppIcon::Edit,
                    &app,
                    window,
                    |app, window, cx| app.start_rename(window, cx),
                ))
                .item(popup_action(
                    tr("删除", "Delete"),
                    AppIcon::Delete,
                    &app,
                    window,
                    |app, window, cx| app.request_delete(window, cx),
                ))
            }
            FileMenuKind::Folder => menu
                .item(popup_action(
                    tr("打开", "Open"),
                    AppIcon::FolderOpen,
                    &app,
                    window,
                    |app, window, cx| app.open_selected(window, cx),
                ))
                .item(popup_action(
                    tr("在新窗口打开", "Open in new window"),
                    AppIcon::Grid,
                    &app,
                    window,
                    |app, _, cx| app.open_directory_window(cx),
                ))
                .separator()
                .item(popup_action(
                    tr("压缩为 Lumark 归档", "Compress to Lumark archive"),
                    AppIcon::Compression,
                    &app,
                    window,
                    |app, window, cx| app.open_compress_dialog(window, cx),
                ))
                .item(popup_action(
                    tr("伪装压缩…", "Disguise compression..."),
                    AppIcon::Video,
                    &app,
                    window,
                    |app, window, cx| app.open_compress_dialog_for_disguise(window, cx),
                ))
                .separator()
                .item(popup_action(
                    tr("新建文件夹", "New folder"),
                    AppIcon::FolderPlus,
                    &app,
                    window,
                    |app, window, cx| app.start_new_folder(window, cx),
                ))
                .item(popup_action(
                    tr("复制文件", "Copy files"),
                    AppIcon::Copy,
                    &app,
                    window,
                    |app, _, cx| app.copy_selected_items(cx),
                ))
                .item(popup_action(
                    tr("复制路径", "Copy path"),
                    AppIcon::Copy,
                    &app,
                    window,
                    |app, _, cx| app.copy_selected_path(cx),
                ))
                .item(popup_action(
                    tr("在资源管理器中显示", "Show in Explorer"),
                    AppIcon::Search,
                    &app,
                    window,
                    |app, _, _| app.reveal_selected(),
                ))
                .separator()
                .item(popup_action(
                    tr("重命名", "Rename"),
                    AppIcon::Edit,
                    &app,
                    window,
                    |app, window, cx| app.start_rename(window, cx),
                ))
                .item(popup_action(
                    tr("删除", "Delete"),
                    AppIcon::Delete,
                    &app,
                    window,
                    |app, window, cx| app.request_delete(window, cx),
                )),
            FileMenuKind::ArchiveFileSystem | FileMenuKind::IsoFile => {
                let menu = menu
                    .item(popup_action(
                        tr("用 Lumark 打开", "Open with Lumark"),
                        AppIcon::FolderOpen,
                        &app,
                        window,
                        |app, window, cx| app.open_selected(window, cx),
                    ))
                    .item(popup_action(
                        tr("解压到…", "Extract to..."),
                        AppIcon::Zip,
                        &app,
                        window,
                        |app, window, cx| app.open_extract_dialog(window, cx),
                    ))
                    .item(popup_action(
                        tr("伪装为媒体文件…", "Disguise as media file..."),
                        AppIcon::Video,
                        &app,
                        window,
                        |app, window, cx| app.open_disguise_dialog(window, cx),
                    ))
                    .separator()
                    .item(popup_action(
                        tr("复制文件", "Copy files"),
                        AppIcon::Copy,
                        &app,
                        window,
                        |app, _, cx| app.copy_selected_items(cx),
                    ))
                    .item(popup_action(
                        tr("复制路径", "Copy path"),
                        AppIcon::Copy,
                        &app,
                        window,
                        |app, _, cx| app.copy_selected_path(cx),
                    ))
                    .item(popup_action(
                        tr("在资源管理器中显示", "Show in Explorer"),
                        AppIcon::Search,
                        &app,
                        window,
                        |app, _, _| app.reveal_selected(),
                    ));
                let menu = if kind == FileMenuKind::IsoFile {
                    menu.separator()
                        .item(popup_action(
                            tr("加载至虚拟光驱", "Mount to virtual drive"),
                            AppIcon::Download,
                            &app,
                            window,
                            |app, window, cx| app.mount_selected_iso(window, cx),
                        ))
                        .item(popup_action(
                            tr("卸载虚拟光驱", "Unmount virtual drive"),
                            AppIcon::Close,
                            &app,
                            window,
                            |app, window, cx| app.unmount_selected_iso(window, cx),
                        ))
                } else {
                    menu
                };
                let menu = if disguised {
                    menu.separator().item(popup_action(
                        tr("还原伪装文件", "Restore disguised file"),
                        AppIcon::Unlock,
                        &app,
                        window,
                        |app, window, cx| app.restore_selected_disguise(window, cx),
                    ))
                } else {
                    menu
                };
                menu.separator()
                    .item(popup_action(
                        tr("重命名", "Rename"),
                        AppIcon::Edit,
                        &app,
                        window,
                        |app, window, cx| app.start_rename(window, cx),
                    ))
                    .item(popup_action(
                        tr("删除", "Delete"),
                        AppIcon::Delete,
                        &app,
                        window,
                        |app, window, cx| app.request_delete(window, cx),
                    ))
            }
            FileMenuKind::PeFile => menu
                .item(popup_action(
                    tr("用 Lumark 打开", "Open with Lumark"),
                    AppIcon::FolderOpen,
                    &app,
                    window,
                    |app, window, cx| app.open_selected(window, cx),
                ))
                .item(popup_action(
                    tr("解压到…", "Extract to..."),
                    AppIcon::Zip,
                    &app,
                    window,
                    |app, window, cx| app.open_extract_dialog(window, cx),
                ))
                .separator()
                .item(popup_action(
                    tr("运行", "Run"),
                    AppIcon::Doc,
                    &app,
                    window,
                    |app, window, cx| app.open_selected_external(window, cx),
                ))
                .item(popup_action(
                    tr("复制路径", "Copy path"),
                    AppIcon::Copy,
                    &app,
                    window,
                    |app, _, cx| app.copy_selected_path(cx),
                ))
                .item(popup_action(
                    tr("在资源管理器中显示", "Show in Explorer"),
                    AppIcon::Search,
                    &app,
                    window,
                    |app, _, _| app.reveal_selected(),
                ))
                .separator()
                .item(popup_action(
                    tr("重命名", "Rename"),
                    AppIcon::Edit,
                    &app,
                    window,
                    |app, window, cx| app.start_rename(window, cx),
                ))
                .item(popup_action(
                    tr("删除", "Delete"),
                    AppIcon::Delete,
                    &app,
                    window,
                    |app, window, cx| app.request_delete(window, cx),
                )),
            FileMenuKind::ExecutableFile | FileMenuKind::File => {
                let executable = kind == FileMenuKind::ExecutableFile;
                let menu = menu
                    .item(popup_action(
                        if executable {
                            tr("运行", "Run")
                        } else {
                            tr("打开", "Open")
                        },
                        AppIcon::Doc,
                        &app,
                        window,
                        move |app, window, cx| {
                            if executable {
                                app.open_selected_external(window, cx);
                            } else {
                                app.open_selected(window, cx);
                            }
                        },
                    ))
                    .item(popup_action(
                        tr("使用内置查看器打开", "Open in built-in viewer"),
                        AppIcon::Doc,
                        &app,
                        window,
                        |app, window, cx| app.open_selected_in_viewer(window, cx),
                    ));
                let menu = menu
                    .item(popup_action(
                        tr("压缩为 Lumark 归档", "Compress to Lumark archive"),
                        AppIcon::Compression,
                        &app,
                        window,
                        |app, window, cx| app.open_compress_dialog(window, cx),
                    ))
                    .item(popup_action(
                        tr("伪装为媒体文件…", "Disguise as media file..."),
                        AppIcon::Video,
                        &app,
                        window,
                        |app, window, cx| app.open_disguise_dialog(window, cx),
                    ));
                let menu = if disguised {
                    menu.separator().item(popup_action(
                        tr("还原伪装文件", "Restore disguised file"),
                        AppIcon::Unlock,
                        &app,
                        window,
                        |app, window, cx| app.restore_selected_disguise(window, cx),
                    ))
                } else {
                    menu
                };
                menu.separator()
                    .item(popup_action(
                        tr("复制文件", "Copy files"),
                        AppIcon::Copy,
                        &app,
                        window,
                        |app, _, cx| app.copy_selected_items(cx),
                    ))
                    .item(popup_action(
                        tr("复制路径", "Copy path"),
                        AppIcon::Copy,
                        &app,
                        window,
                        |app, _, cx| app.copy_selected_path(cx),
                    ))
                    .item(popup_action(
                        tr("在资源管理器中显示", "Show in Explorer"),
                        AppIcon::Search,
                        &app,
                        window,
                        |app, _, _| app.reveal_selected(),
                    ))
                    .separator()
                    .item(popup_action(
                        tr("重命名", "Rename"),
                        AppIcon::Edit,
                        &app,
                        window,
                        |app, window, cx| app.start_rename(window, cx),
                    ))
                    .item(popup_action(
                        tr("删除", "Delete"),
                        AppIcon::Delete,
                        &app,
                        window,
                        |app, window, cx| app.request_delete(window, cx),
                    ))
            }
        }
    }
}

/// 创建一个带图标、点击后调用应用动作的官方菜单项。
pub(super) fn popup_action(
    label: &'static str,
    icon: AppIcon,
    app: &gpui_kit::Entity<LumarkApp>,
    window: &mut Window,
    action: impl Fn(&mut LumarkApp, &mut Window, &mut Context<LumarkApp>) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(icon.element(14.))
        .on_click(
            window.listener_for(app, move |app, _: &gpui_kit::ClickEvent, window, cx| {
                action(app, window, cx);
                cx.notify();
            }),
        )
}
