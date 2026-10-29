//! 内置文本、十六进制和 PE 查看器窗口。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::super::*;
use super::components::AppIcon;
use crate::i18n::tr;
use gpui_kit::component::TitleBar;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tag::Tag;
use lumark_viewer::{
    ContentKind, Eol, HexDocument, PeImage, PeInfo, TextDocument, classify_content, open_source,
    save_text,
};

/// 查看器当前内容视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerMode {
    /// 文本。
    Text,
    /// 十六进制。
    Hex,
    /// PE 结构。
    Pe,
}

/// 归档条目的保存目标；源文件是临时解压副本，保存后需写回归档。
#[derive(Clone)]
pub(crate) struct ViewerArchiveTarget {
    /// 原归档路径。
    pub archive: PathBuf,
    /// 归档内相对路径。
    pub entry: String,
    /// 原归档密码。
    pub password: Option<zeroize::Zeroizing<String>>,
    /// 是否保留 `.lumark.bak`。
    pub keep_backup: bool,
}

/// 内置查看器窗口。
pub(crate) struct TextViewerWindowView {
    name: String,
    path: PathBuf,
    path_text: String,
    size: u64,
    saved_modified: Option<std::time::SystemTime>,
    can_save: bool,
    archive_target: Option<ViewerArchiveTarget>,
    mode: ViewerMode,
    document: Option<TextDocument>,
    editor: Option<gpui_kit::Entity<EditorState>>,
    line_sizes: Rc<Vec<gpui_kit::Size<gpui_kit::Pixels>>>,
    text_scroll: gpui_kit::base::VirtualListScrollHandle,
    hex_scroll: gpui_kit::base::VirtualListScrollHandle,
    hex: Option<HexDocument>,
    hex_sizes: Rc<Vec<gpui_kit::Size<gpui_kit::Pixels>>>,
    pe: Option<PeImage>,
    error: Option<String>,
    status: String,
}

impl TextViewerWindowView {
    /// 打开文件并按内容决定初始视图。
    pub(crate) fn new(
        path: PathBuf,
        name: String,
        can_save: bool,
        archive_target: Option<ViewerArchiveTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_text = path.display().to_string();
        let size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let saved_modified = std::fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let source = match open_source(&path) {
            Ok(source) => source,
            Err(error) => {
                return Self::failed(
                    path,
                    name,
                    path_text,
                    size,
                    saved_modified,
                    can_save,
                    archive_target,
                    error.to_string(),
                );
            }
        };
        let kind = classify_content(source.as_slice(), &path);
        let (mode, document, editor, line_sizes, pe, error) = match kind {
            ContentKind::Text => match TextDocument::open(&path) {
                Ok(document) => {
                    let line_count = document.line_count();
                    let line_sizes = Rc::new(vec![gpui_kit::size(px(1.), px(18.)); line_count]);
                    let editor = document.editable().then(|| {
                        let language = language_for_name(&name);
                        let text = document.text();
                        cx.new(|cx| {
                            EditorState::new(window, cx)
                                .default_value(text)
                                .language(language)
                        })
                    });
                    (
                        ViewerMode::Text,
                        Some(document),
                        editor,
                        line_sizes,
                        None,
                        None,
                    )
                }
                Err(error) => (
                    ViewerMode::Hex,
                    None,
                    None,
                    Rc::new(Vec::new()),
                    None,
                    Some(error.to_string()),
                ),
            },
            ContentKind::Pe => {
                let pe = PeImage::open(&path);
                let error = pe.as_ref().err().map(ToString::to_string);
                (
                    ViewerMode::Pe,
                    None,
                    None,
                    Rc::new(Vec::new()),
                    pe.ok(),
                    error,
                )
            }
            ContentKind::Binary => (ViewerMode::Hex, None, None, Rc::new(Vec::new()), None, None),
        };
        let (hex, hex_sizes) = if mode == ViewerMode::Hex || pe.is_some() || document.is_none() {
            match HexDocument::open(&path) {
                Ok(hex) => {
                    let sizes = Rc::new(vec![gpui_kit::size(px(1.), px(18.)); hex.row_count()]);
                    (Some(hex), sizes)
                }
                Err(error) => {
                    return Self::failed(
                        path,
                        name,
                        path_text,
                        size,
                        saved_modified,
                        can_save,
                        archive_target,
                        error.to_string(),
                    );
                }
            }
        } else {
            (None, Rc::new(Vec::new()))
        };
        Self {
            name,
            path,
            path_text,
            size,
            saved_modified,
            can_save,
            archive_target,
            mode,
            document,
            editor,
            line_sizes,
            text_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex,
            hex_sizes,
            pe,
            error,
            status: String::new(),
        }
    }

    /// 构造错误占位视图。
    #[allow(clippy::too_many_arguments)]
    fn failed(
        path: PathBuf,
        name: String,
        path_text: String,
        size: u64,
        saved_modified: Option<std::time::SystemTime>,
        can_save: bool,
        archive_target: Option<ViewerArchiveTarget>,
        error: String,
    ) -> Self {
        Self {
            name,
            path,
            path_text,
            size,
            saved_modified,
            can_save,
            archive_target,
            mode: ViewerMode::Hex,
            document: None,
            editor: None,
            line_sizes: Rc::new(Vec::new()),
            text_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex_scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            hex: None,
            hex_sizes: Rc::new(Vec::new()),
            pe: None,
            error: Some(error),
            status: String::new(),
        }
    }

    /// 切换模式；PE 文件的十六进制按需打开。
    fn switch_mode(&mut self, mode: ViewerMode, cx: &mut Context<Self>) {
        if mode == ViewerMode::Hex && self.hex.is_none() && self.error.is_none() {
            match HexDocument::open(&self.path) {
                Ok(hex) => {
                    self.hex_sizes =
                        Rc::new(vec![gpui_kit::size(px(1.), px(18.)); hex.row_count()]);
                    self.hex = Some(hex);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        self.mode = mode;
        cx.notify();
    }

    /// 保存编辑器内容到原文件。
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let text = editor.read(cx).text().to_string();
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let had_bom = document.encoding().bom_len(document.bytes()) > 0;
        if let Some(target) = &self.archive_target {
            let bytes = document
                .encoding()
                .encode(&document.eol().normalize(&text), had_bom);
            if let Err(error) = std::fs::write(&self.path, &bytes) {
                self.status = error.to_string();
                cx.notify();
                return;
            }
            match lumark_archive::ArchiveService::replace_entry(
                &target.archive,
                &target.entry,
                &self.path,
                target.password.as_deref().map(String::as_str),
                target.keep_backup,
                None,
                |_| {},
            ) {
                Ok(report) => {
                    self.size = std::fs::metadata(&self.path)
                        .map(|metadata| metadata.len())
                        .unwrap_or(self.size);
                    self.status = format!(
                        "{}{}",
                        tr("已写回归档：", "Saved to archive: "),
                        target.entry
                    );
                    let _ = report;
                }
                Err(error) => self.status = error.to_string(),
            }
            cx.notify();
            return;
        }
        match save_text(
            &self.path,
            &text,
            document.encoding(),
            document.eol(),
            had_bom,
            Some(self.size),
            self.saved_modified,
        ) {
            Ok(outcome) => {
                self.size = outcome.bytes_written;
                self.saved_modified = std::fs::metadata(&self.path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                self.status = tr("已保存", "Saved").to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }

    /// 渲染标题栏。
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(38.))
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().button_primary)
                            .text_color(cx.theme().button_primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(AppIcon::Doc.element(14.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .text_sm()
                            .font_semibold()
                            .child(format!(
                                "{} - {} - Lumark",
                                self.name,
                                tr("查看器", "Viewer")
                            )),
                    )
                    .when(self.can_save && self.editor.is_some(), |this| {
                        this.child(
                            button("viewer-save")
                                .primary()
                                .icon(AppIcon::Save.element(13.))
                                .label(tr("保存", "Save"))
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        )
                    })
                    .when_some(self.document.as_ref(), |this, document| {
                        this.child(Tag::secondary().xsmall().child(document.encoding().label()))
                    }),
            )
    }

    /// 渲染视图模式工具栏。
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .when(self.document.is_some(), |this| {
                this.child(
                    button("viewer-mode-text")
                        .ghost()
                        .selected(self.mode == ViewerMode::Text)
                        .label(tr("文本", "Text"))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.switch_mode(ViewerMode::Text, cx)),
                        ),
                )
            })
            .child(
                button("viewer-mode-hex")
                    .ghost()
                    .selected(self.mode == ViewerMode::Hex)
                    .label(tr("十六进制", "Hex"))
                    .on_click(cx.listener(|this, _, _, cx| this.switch_mode(ViewerMode::Hex, cx))),
            )
            .when(self.pe.is_some(), |this| {
                this.child(
                    button("viewer-mode-pe")
                        .ghost()
                        .selected(self.mode == ViewerMode::Pe)
                        .label("PE")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.switch_mode(ViewerMode::Pe, cx)),
                        ),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.status.clone()),
            )
    }

    /// 渲染文本行；大文件只解码当前视口内的行。
    fn render_text_line(&self, index: usize, cx: &Context<Self>) -> impl IntoElement {
        let line = self
            .document
            .as_ref()
            .and_then(|document| document.line(index))
            .unwrap_or_default();
        let line_number = format!("{:>6}", index + 1);
        h_flex()
            .w_full()
            .h(px(18.))
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_2()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .child(
                div()
                    .w(px(52.))
                    .flex_shrink_0()
                    .text_right()
                    .text_color(cx.theme().muted_foreground)
                    .child(line_number),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(cx.theme().foreground)
                    .child(line),
            )
    }

    /// 渲染十六进制行。
    fn render_hex_line(&self, index: usize, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(hex) = self.hex.as_ref() else {
            return div().into_any_element();
        };
        let Some(row) = hex.row(index) else {
            return div().into_any_element();
        };
        let offset = HexDocument::offset_text(row.offset, hex.byte_len() > u32::MAX as u64);
        h_flex()
            .w_full()
            .h(px(18.))
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_2()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .child(
                div()
                    .w(px(112.))
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(offset),
            )
            .child(
                div()
                    .w(px(420.))
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .text_color(cx.theme().foreground)
                    .child(row.hex),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_color(cx.theme().muted_foreground)
                    .child(row.ascii),
            )
            .into_any_element()
    }

    /// 渲染文本视图。
    fn render_text(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let Some(editor) = &self.editor {
            return v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .p_2()
                .child(
                    Editor::new(editor)
                        .bordered(false)
                        .readonly(!self.can_save)
                        .h_full(),
                )
                .into_any_element();
        }
        let sizes = self.line_sizes.clone();
        let scroll = self.text_scroll.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .relative()
            .overflow_hidden()
            .child(
                gpui_kit::base::v_virtual_list(
                    cx.entity(),
                    "viewer-lines",
                    sizes,
                    move |this, range, _, cx| {
                        range
                            .map(|index| this.render_text_line(index, cx).into_any_element())
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&scroll),
            )
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    /// 渲染十六进制视图。
    fn render_hex(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(hex) = self.hex.as_ref() else {
            return render_error(cx, self.error.as_deref().unwrap_or("没有可显示的数据"));
        };
        let sizes = self.hex_sizes.clone();
        let scroll = self.hex_scroll.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .relative()
            .overflow_hidden()
            .child(
                gpui_kit::base::v_virtual_list(
                    cx.entity(),
                    "viewer-hex",
                    sizes,
                    move |this, range, _, cx| {
                        range
                            .map(|index| this.render_hex_line(index, cx))
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&scroll),
            )
            .vertical_scrollbar(&scroll)
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} B", hex.byte_len())),
            )
            .into_any_element()
    }

    /// 渲染 PE 结构摘要。
    fn render_pe(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(pe) = self.pe.as_ref() else {
            return render_error(cx, self.error.as_deref().unwrap_or("无法解析 PE"));
        };
        let info = &pe.info;
        let mut sections = v_flex().w_full().gap_1();
        for section in info.sections.iter().take(128) {
            sections = sections.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .w(px(100.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(section.name.clone()),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format!("RVA {:08x}", section.virtual_address)),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(format!("RAW {:08x}", section.raw_offset)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(cx.theme().muted_foreground)
                            .child(PeInfo::section_flags(section)),
                    ),
            );
        }
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_5()
            .gap_3()
            .overflow_hidden()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Tag::primary().xsmall().child(format!(
                        "{} · {}",
                        info.bitness_label(),
                        info.machine_label()
                    )))
                    .child(
                        Tag::secondary()
                            .xsmall()
                            .child(format!("{} 个节", info.sections.len())),
                    ),
            )
            .child(pe_metric_row(
                cx,
                tr("入口点", "Entry point"),
                &format!("0x{:08x}", info.entry_point),
            ))
            .child(pe_metric_row(
                cx,
                tr("映像基址", "Image base"),
                &format!("0x{:x}", info.image_base),
            ))
            .child(pe_metric_row(
                cx,
                tr("子系统", "Subsystem"),
                subsystem_label(info.subsystem),
            ))
            .child(pe_metric_row(
                cx,
                tr("文件大小", "File size"),
                &format_size(info.file_size),
            ))
            .when_some(info.certificate, |this, (offset, size)| {
                this.child(pe_metric_row(
                    cx,
                    tr("证书表", "Certificate"),
                    &format!("0x{offset:08x} ({})", format_size(u64::from(size))),
                ))
            })
            .when_some(info.overlay_offset, |this, offset| {
                this.child(pe_metric_row(
                    cx,
                    tr("覆盖数据", "Overlay"),
                    &format!("0x{offset:08x}"),
                ))
            })
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(tr("节表", "Sections")),
            )
            .child(sections)
            .into_any_element()
    }

    /// 渲染内容区域。
    fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let Some(error) = &self.error {
            return render_error(cx, error);
        }
        match self.mode {
            ViewerMode::Text if self.document.is_some() => self.render_text(cx),
            ViewerMode::Hex => self.render_hex(cx),
            ViewerMode::Pe if self.pe.is_some() => self.render_pe(cx),
            _ => self.render_hex(cx),
        }
    }

    /// 渲染底部状态栏。
    fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let (mode, detail) = match self.mode {
            ViewerMode::Text => (
                tr("文本", "Text"),
                self.document
                    .as_ref()
                    .map(|document| format!("{} {}", document.line_count(), tr("行", "lines")))
                    .unwrap_or_default(),
            ),
            ViewerMode::Hex => (
                tr("十六进制", "Hex"),
                self.hex
                    .as_ref()
                    .map(|hex| format!("{} B", hex.byte_len()))
                    .unwrap_or_default(),
            ),
            ViewerMode::Pe => (
                "PE",
                self.pe
                    .as_ref()
                    .map(|pe| format!("{} {}", pe.info.sections.len(), tr("个节", "sections")))
                    .unwrap_or_default(),
            ),
        };
        h_flex()
            .h(px(26.))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(self.path_text.clone()),
            )
            .child(mode)
            .child(detail)
            .when_some(self.document.as_ref(), |this, document| {
                this.child(format!(
                    "{} / {}",
                    document.encoding().label(),
                    eol_label(document.eol())
                ))
            })
            .child(format_size(self.size))
            .when(!self.status.is_empty(), |this| {
                this.child(self.status.clone())
            })
    }
}

impl Render for TextViewerWindowView {
    /// 组合标题、模式工具栏、内容区和状态栏。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(self.render_body(cx))
            .child(self.render_status_bar(cx))
    }
}

fn render_error(cx: &Context<impl Render>, message: &str) -> gpui_kit::AnyElement {
    v_flex()
        .flex_1()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .bg(cx.theme().background)
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().danger)
                .child(tr("无法打开内容", "Cannot open content")),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(message.to_string()),
        )
        .into_any_element()
}

fn pe_metric_row(cx: &Context<impl Render>, label: &str, value: &str) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_3()
        .text_sm()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .child(value.to_string()),
        )
}

fn subsystem_label(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "Native",
        2 => "Windows GUI",
        3 => "Windows CUI",
        9 => "Windows CE GUI",
        10 => "EFI Application",
        11 => "EFI Boot Service Driver",
        12 => "EFI Runtime Driver",
        14 => "Xbox",
        _ => "Other",
    }
}

fn eol_label(eol: Eol) -> &'static str {
    match eol {
        Eol::Lf => "LF",
        Eol::Crlf => "CRLF",
        Eol::Cr => "CR",
    }
}

/// 把扩展名映射到 syntect 语言名；未知扩展名使用纯文本。
fn language_for_name(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "rs" => "Rust",
        "java" => "Java",
        "sql" => "SQL",
        "xml" => "XML",
        "html" | "htm" | "vue" => "HTML",
        "css" => "CSS",
        "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "cs" => "C#",
        "go" => "Go",
        "py" => "Python",
        "sh" | "bash" => "Bourne Again Shell (bash)",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "md" | "markdown" => "Markdown",
        _ => "Plain Text",
    }
}

impl LumarkApp {
    /// 在独立的查看器窗口中打开文件。
    ///
    /// 文本、二进制和 PE 由查看器按内容决定；归档条目由调用方传入
    /// `can_save=false`，避免只保存到临时解压副本。
    pub(crate) fn open_text_viewer(
        &mut self,
        target: PathBuf,
        display: String,
        can_save: bool,
        archive_target: Option<ViewerArchiveTarget>,
        parent: &Window,
        cx: &mut Context<Self>,
    ) {
        let name = target
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| display.clone());
        let window_size = gpui_kit::size(px(980.), px(680.));
        let parent_bounds = parent.inner_window_bounds().get_bounds();
        let origin = gpui_kit::point(
            px(parent_bounds.origin.x.as_f32()
                + (parent_bounds.size.width.as_f32() - window_size.width.as_f32()) / 2.0),
            px(parent_bounds.origin.y.as_f32()
                + (parent_bounds.size.height.as_f32() - window_size.height.as_f32()) / 2.0),
        );
        let options = gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                origin,
                size: window_size,
            })),
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("查看器 - Lumark".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            kind: gpui_kit::WindowKind::Normal,
            focus: true,
            show: true,
            is_resizable: true,
            is_minimizable: true,
            is_movable: true,
            window_min_size: Some(gpui_kit::size(px(620.), px(420.))),
            ..TitleBar::window_options()
        };
        let viewer_path = target.clone();
        let result = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                TextViewerWindowView::new(
                    viewer_path.clone(),
                    name.clone(),
                    can_save,
                    archive_target,
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok(_) => self.status = format!("已在内置查看器中打开 {display}"),
            Err(error) => self.status = format!("无法打开查看器：{error}"),
        }
    }
}
