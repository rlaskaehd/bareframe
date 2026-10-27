//! Warp's Markdown viewer: a `.md` file rendered read-only in a pane and read again when it
//! changes on disk. Inline HTML and Mermaid are shown as plain text or left out, as planned.
use crate::{
    theme::{self, Theme, hsla},
    watch::{Watch, watch_file},
};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Font, FontStyle, FontWeight,
    InteractiveText, ScrollHandle, StrikethroughStyle, StyledText, TextRun, UnderlineStyle, Window,
    div, img, prelude::*, px,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

gpui::actions!(markdown, [ToggleRaw]);

/// Keys of a Markdown file's pane, rendered or raw.
pub const KEY_CONTEXT: &str = "Markdown";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    vec![gpui::KeyBinding::new("cmd-e", ToggleRaw, Some(KEY_CONTEXT))]
}

/// Warp's `Rendered | Raw` switch above a Markdown file, in both of its views.
pub fn view_switch(
    theme: &'static Theme,
    raw: bool,
    on_switch: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_switch = std::rc::Rc::new(on_switch);
    let segment = |id: &'static str, label: &'static str, chosen: bool| {
        let on_switch = on_switch.clone();
        div()
            .id(id)
            .px_2()
            .py(px(1.0))
            .rounded_sm()
            .when(chosen, |this| {
                this.bg(hsla(theme.tab_active)).text_color(hsla(theme.text))
            })
            .when(!chosen, |this| {
                this.text_color(hsla(theme.text_muted))
                    .hover(|style| style.text_color(hsla(theme.text)))
                    .on_click(move |_, window, cx| on_switch(window, cx))
            })
            .child(label)
    };
    div()
        .flex()
        .flex_none()
        .justify_end()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(hsla(theme.border))
        .font_family(theme::UI_FONT)
        .text_size(px(12.0))
        .child(
            div()
                .flex()
                .gap_1()
                .p(px(2.0))
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.border))
                .child(segment("view-rendered", "Rendered", !raw))
                .child(segment("view-raw", "Raw", raw)),
        )
}

const TEXT_SIZE: f32 = 14.0;
/// Quotes and lists nested deeper than this are read into the level above, so a hostile
/// file can't exhaust the stack while parsing or drawing.
const MAX_DEPTH: usize = 32;
/// Larger files are not rendered; reading and laying them out would stall the window.
const MAX_FILE: u64 = 8 << 20;
const MAX_IMAGE: u64 = 20 << 20;
/// Pixels a picture may decode to; a small file can claim a huge canvas.
const MAX_PIXELS: u64 = 50_000_000;
/// Readable line length; wider panes center the text, as Warp does.
const MAX_WIDTH: f32 = 860.0;

#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(u8, Inline),
    Paragraph(Inline),
    Code {
        language: String,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Rule,
    /// The first row is the header.
    Table(Vec<Vec<Inline>>),
}

#[derive(Debug, PartialEq)]
pub struct Item {
    /// `Some` for a task list item: whether it is checked.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// One run of text with its styles, covering the text end to end, plus its links and the
/// images that sat in it (drawn below the text).
#[derive(Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub runs: Vec<(Range<usize>, Style)>,
    pub links: Vec<(Range<usize>, String)>,
    pub images: Vec<Image>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
}

#[derive(Debug, PartialEq)]
pub struct Image {
    pub source: String,
    pub alt: String,
}

impl Inline {
    fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.runs.push((start..self.text.len(), style));
    }
}

pub fn parse(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    blocks(&mut Parser::new_ext(text, options).peekable(), 0)
}

type Events<'a> = std::iter::Peekable<Parser<'a>>;

fn is_inline(event: &Event) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::InlineHtml(_)
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_) => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. }
                | Tag::Superscript
                | Tag::Subscript
        ),
        Event::End(tag) => matches!(
            tag,
            TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image
                | TagEnd::Superscript
                | TagEnd::Subscript
        ),
        _ => false,
    }
}

/// The blocks of a container up to its end (consumed), or to the end of the input. Loose
/// text, as in a tight list item, becomes a paragraph.
fn blocks(events: &mut Events, depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    // Containers past `MAX_DEPTH` whose contents are read into this level.
    let mut flattened = 0;
    while let Some(event) = events.peek() {
        if is_inline(event) {
            out.push(Block::Paragraph(inline(events)));
            continue;
        }
        let Some(event) = events.next() else {
            break;
        };
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item) if depth >= MAX_DEPTH => {
                flattened += 1
            }
            Event::End(_) if flattened > 0 => flattened -= 1,
            Event::End(_) => break,
            Event::Rule => out.push(Block::Rule),
            Event::Start(Tag::Paragraph) => {
                out.push(Block::Paragraph(inline(events)));
                events.next();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                out.push(Block::Heading(level, inline(events)));
                events.next();
            }
            Event::Start(Tag::BlockQuote(_)) => out.push(Block::Quote(blocks(events, depth + 1))),
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let mut text = String::new();
                for event in events.by_ref() {
                    match event {
                        Event::Text(part) => text.push_str(&part),
                        Event::End(_) => break,
                        _ => {}
                    }
                }
                let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
                out.push(Block::Code { language, text });
            }
            Event::Start(Tag::List(start)) => {
                let mut items = Vec::new();
                while let Some(Event::Start(Tag::Item)) = events.peek() {
                    events.next();
                    let checked = match events.peek() {
                        Some(Event::TaskListMarker(checked)) => {
                            let checked = *checked;
                            events.next();
                            Some(checked)
                        }
                        _ => None,
                    };
                    items.push(Item {
                        checked,
                        blocks: blocks(events, depth + 1),
                    });
                }
                events.next();
                out.push(Block::List { start, items });
            }
            Event::Start(Tag::Table(_)) => out.push(Block::Table(table(events))),
            // Raw HTML is left out (as planned); its text would only be markup.
            Event::Start(Tag::HtmlBlock) => {
                for event in events.by_ref() {
                    if let Event::End(TagEnd::HtmlBlock) = event {
                        break;
                    }
                }
            }
            // Footnote definitions, definition lists and the like show their contents.
            Event::Start(_) => out.extend(blocks(events, depth + 1)),
            _ => {}
        }
    }
    out
}

fn table(events: &mut Events) -> Vec<Vec<Inline>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::TableCell) => row.push(Inline::default()),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => rows.push(std::mem::take(&mut row)),
            Event::End(TagEnd::Table) => break,
            _ => {}
        }
        // A cell's text is read here so `inline` sees only what is inside it.
        if let (Some(cell), Some(event)) = (row.last_mut(), events.peek())
            && is_inline(event)
        {
            *cell = inline(events);
        }
    }
    rows
}

fn inline(events: &mut Events) -> Inline {
    let mut out = Inline::default();
    let mut style = Style::default();
    let mut link: Option<(usize, String)> = None;
    let mut image: Option<Image> = None;
    while events.peek().is_some_and(is_inline) {
        let Some(event) = events.next() else {
            break;
        };
        match event {
            Event::Text(text) | Event::InlineHtml(text) | Event::InlineMath(text) => {
                match &mut image {
                    Some(image) => image.alt.push_str(&text),
                    None => out.push(&text, style),
                }
            }
            Event::DisplayMath(text) => out.push(&text, style),
            Event::Code(text) => out.push(
                &text,
                Style {
                    code: true,
                    ..style
                },
            ),
            Event::SoftBreak => out.push(" ", style),
            Event::HardBreak => out.push("\n", style),
            Event::FootnoteReference(name) => out.push(&format!("[{name}]"), style),
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some((out.text.len(), dest_url.into_string()));
                style.link = true;
            }
            Event::End(TagEnd::Link) => {
                style.link = false;
                if let Some((start, url)) = link.take()
                    && start < out.text.len()
                {
                    out.links.push((start..out.text.len(), url));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some(Image {
                    source: dest_url.into_string(),
                    alt: String::new(),
                });
            }
            Event::End(TagEnd::Image) => out.images.extend(image.take()),
            _ => {}
        }
    }
    out
}

/// Where a link points, once resolved against the file it is in.
#[derive(Debug, PartialEq)]
pub enum Target {
    Web(String),
    /// Another Markdown file, opened in the viewer.
    Markdown(PathBuf),
    /// Any other local file, opened as the explorer would.
    File(PathBuf),
}

/// `None` for links bareframe won't follow: in-page anchors, and schemes other than
/// web pages and mail, which could start other programs.
pub fn resolve(link: &str, base: &Path) -> Option<Target> {
    let lower = link.to_ascii_lowercase();
    if ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(Target::Web(link.to_owned()));
    }
    if link.starts_with('#') || link.contains("://") || lower.starts_with("javascript:") {
        return None;
    }
    let path = link.split('#').next().unwrap_or(link);
    let path = base.join(percent_decode(path));
    if !path.exists() {
        return None;
    }
    Some(if is_markdown(&path) {
        Target::Markdown(path)
    } else {
        Target::File(path)
    })
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "mdown", "mkd"].contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// `docs/a%20b.md` → `docs/a b.md`; anything malformed stays as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Reads a regular file of at most `MAX_FILE` bytes; a pipe or device under a `.md` name
/// would otherwise block or never end. The cap holds even if the file grows meanwhile.
fn read_document(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err("not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("larger than {} MB", MAX_FILE >> 20));
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
}

/// A picture file of a sensible size: a link could name a device, a huge file, or a small
/// file that decodes to a huge canvas (only the header is read to check).
fn is_image(path: &Path) -> bool {
    let picture = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            // ponytail: animated GIF and WebP are left out, since frames multiply the
            // decoded size past what the header shows; count frames if they are wanted.
            ["png", "jpg", "jpeg", "bmp"].contains(&extension.to_ascii_lowercase().as_str())
        });
    picture
        && std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_IMAGE)
        && image::image_dimensions(path)
            .is_ok_and(|(width, height)| u64::from(width) * u64::from(height) <= MAX_PIXELS)
}

/// Code blocks in these languages get Warp's "insert into terminal" button.
fn is_shell(language: &str) -> bool {
    ["sh", "bash", "zsh", "shell", "console", "fish"].contains(&language)
}

pub enum MarkdownEvent {
    Open(Target),
    /// A shell code block's text, for the terminal next to the viewer.
    Insert(String),
    /// Show the file's text in the editor instead (`Raw`, Cmd+E).
    ShowRaw,
}

pub struct MarkdownPane {
    path: PathBuf,
    blocks: Vec<Block>,
    error: Option<String>,
    theme: &'static Theme,
    mono: String,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _watch: Option<Watch>,
}

impl EventEmitter<MarkdownEvent> for MarkdownPane {}

impl MarkdownPane {
    pub fn new(path: PathBuf, theme: &'static Theme, mono: String, cx: &mut Context<Self>) -> Self {
        let mut pane = Self {
            path,
            blocks: Vec::new(),
            error: None,
            theme,
            mono,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _watch: None,
        };
        pane.load();
        pane._watch = pane.watch(cx);
        pane
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into(),
        )
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn set_theme(&mut self, theme: &'static Theme, mono: String, cx: &mut Context<Self>) {
        self.theme = theme;
        self.mono = mono;
        cx.notify();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = path;
        self.load();
        self._watch = self.watch(cx);
        self.scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    fn load(&mut self) {
        match read_document(&self.path) {
            Ok(text) => {
                self.blocks = parse(&text);
                self.error = None;
            }
            Err(error) => {
                self.blocks.clear();
                self.error = Some(format!("Could not show {}: {error}", self.path.display()));
            }
        }
    }

    fn watch(&self, cx: &mut Context<Self>) -> Option<Watch> {
        watch_file(
            &self.path,
            |pane, cx| {
                pane.load();
                cx.notify();
            },
            cx,
        )
    }

    fn render_block(&self, block: &Block, ids: &mut usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        match block {
            Block::Heading(level, text) => {
                // Warp's heading scale.
                let scale = match level {
                    1 => 2.25,
                    2 => 1.8,
                    3 => 1.5,
                    4 => 1.0,
                    5 => 0.83,
                    _ => 0.67,
                };
                div()
                    .pt(px(if *level <= 2 { 12.0 } else { 6.0 }))
                    .text_size(px(TEXT_SIZE * scale))
                    .line_height(gpui::relative(1.25))
                    .when(*level <= 4, |this| this.font_weight(FontWeight::SEMIBOLD))
                    .when(*level == 6, |this| this.text_color(hsla(theme.text_muted)))
                    .child(self.render_inline(text, ids, cx))
                    .into_any_element()
            }
            Block::Paragraph(text) => self.render_inline(text, ids, cx),
            Block::Code { language, text } => self.render_code(language, text, ids, cx),
            Block::Quote(blocks) => div()
                .flex()
                .flex_col()
                .gap_3()
                .pl_3()
                .border_l_4()
                .border_color(hsla(theme.border))
                .text_color(hsla(theme.text_muted))
                .children(
                    blocks
                        .iter()
                        .map(|block| self.render_block(block, ids, cx))
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
            Block::List { start, items } => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            let marker = match (item.checked, start) {
                                (Some(true), _) => "☑".to_owned(),
                                (Some(false), _) => "☐".to_owned(),
                                (None, Some(start)) => format!("{}.", start + index as u64),
                                (None, None) => "•".to_owned(),
                            };
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_none()
                                        .min_w(px(18.0))
                                        .text_color(hsla(theme.text_muted))
                                        .child(marker),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .when(item.checked == Some(true), |this| {
                                            this.text_color(hsla(theme.text_muted))
                                        })
                                        .children(
                                            item.blocks
                                                .iter()
                                                .map(|block| self.render_block(block, ids, cx))
                                                .collect::<Vec<_>>(),
                                        ),
                                )
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
            Block::Rule => div()
                .my_2()
                .h(px(1.0))
                .bg(hsla(theme.border))
                .into_any_element(),
            Block::Table(rows) => div()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(hsla(theme.border))
                .overflow_hidden()
                .children(
                    rows.iter()
                        .enumerate()
                        .map(|(index, row)| {
                            div()
                                .flex()
                                .when(index > 0, |this| {
                                    this.border_t_1().border_color(hsla(theme.border))
                                })
                                .when(index == 0, |this| {
                                    this.bg(hsla(theme.tab_active))
                                        .font_weight(FontWeight::SEMIBOLD)
                                })
                                .children(
                                    row.iter()
                                        .enumerate()
                                        .map(|(column, cell)| {
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .px_3()
                                                .py_2()
                                                .when(column > 0, |this| {
                                                    this.border_l_1()
                                                        .border_color(hsla(theme.border))
                                                })
                                                .child(self.render_inline(cell, ids, cx))
                                        })
                                        .collect::<Vec<_>>(),
                                )
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
        }
    }

    fn render_code(
        &self,
        language: &str,
        text: &str,
        ids: &mut usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        *ids += 1;
        let id = *ids;
        let copied = text.to_owned();
        let inserted = text.to_owned();
        let button = |id: (&'static str, usize), label: &'static str| {
            theme::ghost_button(theme, id, label)
                .px_2()
                .py(px(2.0))
                .rounded_sm()
                .text_size(px(11.0))
                .font_family(theme::UI_FONT)
        };
        div()
            .relative()
            .min_w_0()
            .overflow_hidden()
            .rounded_md()
            .bg(hsla(theme.tab_active))
            .border_1()
            .border_color(hsla(theme.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .pt_1()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.0))
                            .text_color(hsla(theme.text_muted))
                            .child(language.to_owned()),
                    )
                    .when(is_shell(language), |this| {
                        this.child(button(("md-insert", id), "Insert into terminal").on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.emit(MarkdownEvent::Insert(inserted.clone()))
                            }),
                        ))
                    })
                    .child(button(("md-copy", id), "Copy").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()))
                    })),
            )
            .child(
                div()
                    .id(("md-code", id))
                    .min_w_0()
                    .overflow_x_scroll()
                    .px_3()
                    .pb_3()
                    .pt_1()
                    .font_family(self.mono.clone())
                    .text_size(px(13.0))
                    .whitespace_nowrap()
                    .child(text.to_owned()),
            )
            .into_any_element()
    }

    fn font(&self, style: Style) -> Font {
        let mut font = gpui::font(if style.code {
            self.mono.clone()
        } else {
            theme::UI_FONT.to_owned()
        });
        if style.bold {
            font.weight = FontWeight::BOLD;
        }
        if style.italic {
            font.style = FontStyle::Italic;
        }
        font
    }

    fn render_inline(&self, text: &Inline, ids: &mut usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        *ids += 1;
        let id = *ids;
        let runs = text
            .runs
            .iter()
            .map(|(range, style)| TextRun {
                len: range.len(),
                font: self.font(*style),
                color: hsla(if style.link { theme.accent } else { theme.text }),
                background_color: style.code.then(|| hsla(theme.tab_active)),
                underline: style.link.then(|| UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(hsla(theme.accent)),
                    ..Default::default()
                }),
                strikethrough: style.strike.then(|| StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                }),
            })
            .collect();
        let base = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let targets: Vec<Option<Target>> = text
            .links
            .iter()
            .map(|(_, link)| resolve(link, &base))
            .collect();
        let ranges = text.links.iter().map(|(range, _)| range.clone()).collect();
        let pane = cx.entity().downgrade();
        let images = text
            .images
            .iter()
            .map(|image| match resolve(&image.source, &base) {
                Some(Target::File(path)) if is_image(&path) => {
                    img(path).max_w_full().into_any_element()
                }
                // Only files next to the document: fetching a web image would tell its
                // server that this file was opened.
                _ => div()
                    .text_color(hsla(theme.text_muted))
                    .child(format!("[image: {}]", image.alt))
                    .into_any_element(),
            })
            .collect::<Vec<_>>();
        let styled = StyledText::new(text.text.clone()).with_runs(runs);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!text.text.is_empty(), |this| {
                this.child(InteractiveText::new(("md-text", id), styled).on_click(
                    ranges,
                    move |index, _, cx: &mut App| {
                        let Some(Some(target)) = targets.get(index) else {
                            return;
                        };
                        let target = match target {
                            Target::Web(url) => Target::Web(url.clone()),
                            Target::Markdown(path) => Target::Markdown(path.clone()),
                            Target::File(path) => Target::File(path.clone()),
                        };
                        pane.update(cx, |pane, cx| match target {
                            // A linked document replaces this one, as in a browser.
                            Target::Markdown(path) => pane.navigate(path, cx),
                            target => cx.emit(MarkdownEvent::Open(target)),
                        })
                        .ok();
                    },
                ))
            })
            .children(images)
            .into_any_element()
    }
}

impl Render for MarkdownPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let mut ids = 0;
        let blocks: Vec<AnyElement> = match &self.error {
            Some(error) => vec![
                div()
                    .text_color(hsla(theme.text_muted))
                    .child(error.clone())
                    .into_any_element(),
            ],
            None => self
                .blocks
                .iter()
                .map(|block| self.render_block(block, &mut ids, cx))
                .collect(),
        };
        let switch = view_switch(self.theme, false, {
            let pane = cx.entity().downgrade();
            move |_, cx| {
                pane.update(cx, |_, cx| cx.emit(MarkdownEvent::ShowRaw))
                    .ok();
            }
        });
        let content = div()
            .id("markdown")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(hsla(theme.terminal_background))
            .font_family(theme::UI_FONT)
            .text_size(px(TEXT_SIZE))
            .line_height(gpui::relative(1.55))
            .text_color(hsla(theme.text))
            .child(
                div().w_full().flex().justify_center().child(
                    div()
                        .w_full()
                        .max_w(px(MAX_WIDTH))
                        .px_6()
                        .py_5()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .children(blocks),
                ),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .bg(hsla(theme.terminal_background))
            .on_action(cx.listener(|_, _: &ToggleRaw, _, cx| cx.emit(MarkdownEvent::ShowRaw)))
            .child(switch)
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Inline {
        let mut inline = Inline::default();
        inline.push(text, Style::default());
        inline
    }

    #[test]
    fn blocks_follow_the_document() {
        let blocks = parse(
            "# Title\n\nSome **bold** and `code`.\n\n```sh\nls -la\n```\n\n> quoted\n\n---\n",
        );
        assert_eq!(blocks[0], Block::Heading(1, plain("Title")));
        let Block::Paragraph(text) = &blocks[1] else {
            panic!("{:?}", blocks[1]);
        };
        assert_eq!(text.text, "Some bold and code.");
        let bold = text.runs.iter().find(|(_, style)| style.bold).unwrap();
        assert_eq!(&text.text[bold.0.clone()], "bold");
        let code = text.runs.iter().find(|(_, style)| style.code).unwrap();
        assert_eq!(&text.text[code.0.clone()], "code");
        assert_eq!(
            blocks[2],
            Block::Code {
                language: "sh".into(),
                text: "ls -la".into()
            }
        );
        assert_eq!(
            blocks[3],
            Block::Quote(vec![Block::Paragraph(plain("quoted"))])
        );
        assert_eq!(blocks[4], Block::Rule);
    }

    #[test]
    fn lists_tasks_and_nesting() {
        let blocks = parse("- [x] done\n- [ ] todo\n  1. inner\n");
        let Block::List { start: None, items } = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[0].blocks, [Block::Paragraph(plain("done"))]);
        assert_eq!(items[1].checked, Some(false));
        assert!(matches!(
            items[1].blocks[1],
            Block::List { start: Some(1), .. }
        ));
    }

    #[test]
    fn tables_links_and_images() {
        let blocks =
            parse("| a | b |\n|---|---|\n| 1 | [x](y.md) |\n\n![alt text](pic.png) after\n");
        let Block::Table(rows) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1].text, "b");
        assert_eq!(rows[1][1].links, [(0..1, "y.md".to_owned())]);
        let Block::Paragraph(text) = &blocks[1] else {
            panic!("{blocks:?}");
        };
        assert_eq!(text.text.trim(), "after");
        assert_eq!(
            text.images,
            [Image {
                source: "pic.png".into(),
                alt: "alt text".into()
            }]
        );
    }

    #[test]
    fn deep_nesting_is_flattened_not_recursed() {
        let deep = format!("{}text\n", "> ".repeat(5000));
        let blocks = parse(&deep);
        let mut depth = 0;
        let mut level = &blocks;
        while let [Block::Quote(inner)] = level.as_slice() {
            depth += 1;
            level = inner;
        }
        assert_eq!(depth, MAX_DEPTH);
        assert_eq!(level.as_slice(), [Block::Paragraph(plain("text"))]);
        let list = format!("{}- item\n", "- ".repeat(5000));
        assert!(!parse(&list).is_empty());
    }

    #[test]
    fn documents_and_images_are_bounded() {
        let dir = std::env::temp_dir().join(format!("bareframe-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.md");
        std::fs::write(&big, vec![b'a'; MAX_FILE as usize + 1]).unwrap();
        assert!(read_document(&big).is_err());
        assert!(
            read_document(&dir).is_err(),
            "a directory is not a document"
        );
        // A PNG header claiming 100000 × 100000 pixels, in a file of a few bytes.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let bomb = dir.join("bomb.png");
        std::fs::write(&bomb, png).unwrap();
        assert!(!is_image(&bomb));
        assert!(!is_image(&dir.join("missing.png")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_is_left_out() {
        assert_eq!(
            parse("<div>\nhidden\n</div>\n\ntext\n"),
            [Block::Paragraph(plain("text"))]
        );
    }

    #[test]
    fn links_resolve_only_to_safe_targets() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            resolve("https://example.com", base),
            Some(Target::Web("https://example.com".into()))
        );
        assert_eq!(
            resolve("docs/usage.md#설정", base),
            Some(Target::Markdown(base.join("docs/usage.md")))
        );
        assert_eq!(
            resolve("Cargo.toml", base),
            Some(Target::File(base.join("Cargo.toml")))
        );
        assert_eq!(resolve("#anchor", base), None);
        assert_eq!(resolve("file:///etc/passwd", base), None);
        assert_eq!(resolve("javascript:alert(1)", base), None);
        assert_eq!(resolve("missing.md", base), None);
        assert_eq!(percent_decode("a%20b%zz"), "a b%zz");
    }
}
