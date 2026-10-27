//! Warp's Markdown viewer: a `.md` file rendered read-only in a pane and read again when it
//! changes on disk. Inline HTML is left out; Mermaid blocks are drawn as diagrams.
use crate::{
    theme::{self, Theme, hsla},
    watch::{Watch, watch_file},
};
use files::{is_image, read_document, resolve};
use gpui::{
    App, Context, EventEmitter, FocusHandle, ListAlignment, ListState, Task, Window, div, list,
    prelude::*, px,
};
use parse::{Block, inlines, mermaid_sources, parse};
use std::path::{Path, PathBuf};

mod files;
mod parse;
mod render;
mod select;

pub use files::{Target, is_markdown};

gpui::actions!(markdown, [ToggleRaw, CopySelection]);

/// Keys of a Markdown file's pane, rendered or raw.
pub const KEY_CONTEXT: &str = "Markdown";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    vec![
        gpui::KeyBinding::new("cmd-e", ToggleRaw, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("cmd-c", CopySelection, Some(KEY_CONTEXT)),
    ]
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

pub(super) const TEXT_SIZE: f32 = 14.0;
/// Readable line length; wider panes center the text, as Warp does.
const MAX_WIDTH: f32 = 860.0;

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
    /// The blocks as a list that lays out only those in view: a long document stays quick
    /// to scroll and to redraw.
    list: ListState,
    /// Links and pictures resolved against the disk once per load, not on every redraw.
    links: std::collections::HashMap<String, Option<Target>>,
    pictures: std::collections::HashMap<String, Option<PathBuf>>,
    /// Mermaid diagrams by their source, drawn off the UI thread; `diagrams_dark` is the
    /// theme they were drawn in.
    diagrams: std::collections::HashMap<String, Diagram>,
    diagrams_dark: bool,
    /// Diagrams no longer shown; GPUI keeps them until the window lets them go.
    stale: Vec<std::sync::Arc<gpui::Image>>,
    _diagrams: Option<Task<()>>,
    /// The text of each piece drawn so far, for copying a selection (see `select`).
    pieces: std::cell::RefCell<std::collections::BTreeMap<usize, String>>,
    selection: Option<select::Selection>,
    _watch: Option<Watch>,
    _release: gpui::Subscription,
}

pub(super) enum Diagram {
    Pending,
    Ready(std::sync::Arc<gpui::Image>),
    Failed(String),
}

/// Drops the diagrams whose source `gone` picks, moving their pictures to `stale`.
fn forget(
    diagrams: &mut std::collections::HashMap<String, Diagram>,
    stale: &mut Vec<std::sync::Arc<gpui::Image>>,
    gone: impl Fn(&str) -> bool,
) {
    let forgotten = diagrams.extract_if(|source, _| gone(source));
    stale.extend(forgotten.filter_map(|(_, diagram)| match diagram {
        Diagram::Ready(image) => Some(image),
        _ => None,
    }));
}

/// Whether the theme is dark, judged by its background's brightness.
fn is_dark(theme: &Theme) -> bool {
    let rgb = theme.terminal_background;
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    r * 299 + g * 587 + b * 114 < 128_000
}

impl EventEmitter<MarkdownEvent> for MarkdownPane {}

impl MarkdownPane {
    pub fn new(
        path: PathBuf,
        theme: &'static Theme,
        mono: String,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut pane = Self {
            path,
            blocks: Vec::new(),
            error: None,
            theme,
            mono,
            focus: cx.focus_handle(),
            list: ListState::new(0, ListAlignment::Top, px(600.0)),
            links: Default::default(),
            pictures: Default::default(),
            diagrams: Default::default(),
            diagrams_dark: is_dark(theme),
            stale: Vec::new(),
            _diagrams: None,
            pieces: Default::default(),
            selection: None,
            _watch: None,
            _release: cx.on_release_in(window, |pane: &mut Self, window, cx| {
                forget(&mut pane.diagrams, &mut pane.stale, |_| true);
                pane.drop_stale(window, cx);
            }),
        };
        pane.load();
        pane.draw_diagrams(cx);
        pane._watch = pane.watch(cx);
        pane
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> String {
        crate::explorer::file_name(&self.path)
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn set_theme(&mut self, theme: &'static Theme, mono: String, cx: &mut Context<Self>) {
        self.theme = theme;
        self.mono = mono;
        // Diagrams carry their colors, so a switch between dark and light redraws them.
        if is_dark(theme) != self.diagrams_dark {
            self.diagrams_dark = is_dark(theme);
            forget(&mut self.diagrams, &mut self.stale, |_| true);
            self.draw_diagrams(cx);
        }
        cx.notify();
    }

    /// Draws the document's Mermaid diagrams not drawn yet, in the background. The list is
    /// laid out again when they arrive, since a picture is taller than its source.
    fn draw_diagrams(&mut self, cx: &mut Context<Self>) {
        let mut sources = Vec::new();
        mermaid_sources(&self.blocks, &mut sources);
        // Diagrams whose source left the document (edited, or another file) are let go.
        forget(&mut self.diagrams, &mut self.stale, |source| {
            !sources.contains(&source)
        });
        let wanted: Vec<String> = sources
            .into_iter()
            .filter(|source| !self.diagrams.contains_key(*source))
            .map(str::to_owned)
            .collect();
        if wanted.is_empty() {
            return;
        }
        for source in &wanted {
            self.diagrams.insert(source.clone(), Diagram::Pending);
        }
        let dark = self.diagrams_dark;
        self._diagrams = Some(cx.spawn(async move |pane, cx| {
            let drawn = cx
                .background_executor()
                .spawn(async move {
                    wanted
                        .into_iter()
                        .map(|source| {
                            let diagram = match crate::diagram::render(&source, dark) {
                                Ok(svg) => {
                                    Diagram::Ready(std::sync::Arc::new(gpui::Image::from_bytes(
                                        gpui::ImageFormat::Svg,
                                        svg.into_bytes(),
                                    )))
                                }
                                Err(error) => Diagram::Failed(error),
                            };
                            (source, diagram)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            pane.update(cx, |pane, cx| {
                if pane.diagrams_dark != dark {
                    return;
                }
                pane.diagrams.extend(drawn);
                pane.relayout();
                cx.notify();
            })
            .ok();
        }));
    }

    /// Hands the pictures of forgotten diagrams back: GPUI's image cache and the window's
    /// texture atlas keep them otherwise.
    fn drop_stale(&mut self, window: &mut Window, cx: &mut gpui::App) {
        for image in self.stale.drain(..) {
            if let Some(data) = image.clone().get_render_image(window, cx) {
                let _ = window.drop_image(data);
            }
            image.remove_asset(cx);
        }
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = path;
        self.load();
        self.draw_diagrams(cx);
        self._watch = self.watch(cx);
        self.list.scroll_to(gpui::ListOffset::default());
        cx.notify();
    }

    /// An in-page link: scrolls to the heading whose anchor it names.
    pub(super) fn go_to_anchor(&mut self, anchor: &str) {
        let anchor = anchor.to_lowercase();
        let mut seen: std::collections::HashMap<String, usize> = Default::default();
        let found = self.blocks.iter().position(|block| {
            let Block::Heading(_, inline) = block else {
                return false;
            };
            let base = files::slug(&inline.text);
            let repeats = seen.entry(base.clone()).or_default();
            let slug = match *repeats {
                0 => base,
                n => format!("{base}-{n}"),
            };
            *repeats += 1;
            slug == anchor
        });
        if let Some(index) = found {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: gpui::px(0.0),
            });
        }
    }

    fn load(&mut self) {
        // Piece numbers follow the blocks, which are about to change.
        self.pieces.borrow_mut().clear();
        self.selection = None;
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
        let base = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut texts = Vec::new();
        inlines(&self.blocks, &mut texts);
        let (mut links, mut pictures) = (
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        for text in texts {
            for (_, link) in &text.links {
                links
                    .entry(link.clone())
                    .or_insert_with(|| resolve(link, &base));
            }
            for image in &text.images {
                pictures.entry(image.source.clone()).or_insert_with(|| {
                    match resolve(&image.source, &base) {
                        Some(Target::File(path)) if is_image(&path) => Some(path),
                        _ => None,
                    }
                });
            }
        }
        self.links = links;
        self.pictures = pictures;
        self.relayout();
    }

    /// Measures every block again, keeping the reader's place.
    fn relayout(&mut self) {
        let top = self.list.logical_scroll_top();
        self.list.reset(self.blocks.len());
        if top.item_ix < self.blocks.len() {
            self.list.scroll_to(top);
        }
    }

    fn watch(&self, cx: &mut Context<Self>) -> Option<Watch> {
        watch_file(
            &self.path,
            |pane, cx| {
                pane.load();
                pane.draw_diagrams(cx);
                cx.notify();
            },
            cx,
        )
    }
}

impl Render for MarkdownPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drop_stale(window, cx);
        let theme = self.theme;
        let switch = view_switch(self.theme, false, {
            let pane = cx.entity().downgrade();
            move |_, cx| {
                pane.update(cx, |_, cx| cx.emit(MarkdownEvent::ShowRaw))
                    .ok();
            }
        });
        let content = match &self.error {
            Some(error) => div()
                .p_6()
                .text_color(hsla(theme.text_muted))
                .child(error.clone())
                .into_any_element(),
            None => list(
                self.list.clone(),
                cx.processor(|pane, index: usize, _, cx| {
                    let Some(block) = pane.blocks.get(index) else {
                        return div().into_any_element();
                    };
                    // Ids only need to be unique within a block, which lays out on its own.
                    let mut ids = index * 10_000;
                    let element = pane.render_block(block, &mut ids, cx);
                    let last = index + 1 == pane.blocks.len();
                    // Centered at a readable width, as one column, the gap below each block.
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(MAX_WIDTH))
                                .px_6()
                                .when(index == 0, |this| this.pt_5())
                                .pb(px(if last { 20.0 } else { 12.0 }))
                                .child(element),
                        )
                        .into_any_element()
                }),
            )
            .flex_1()
            .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .bg(hsla(theme.terminal_background))
            .font_family(theme::UI_FONT)
            .text_size(px(TEXT_SIZE))
            .line_height(gpui::relative(1.55))
            .text_color(hsla(theme.text))
            .on_action(cx.listener(|_, _: &ToggleRaw, _, cx| cx.emit(MarkdownEvent::ShowRaw)))
            .on_action(cx.listener(|pane, _: &CopySelection, _, cx| pane.copy_selection(cx)))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|pane, _, _, _| pane.end_selection()),
            )
            .child(switch)
            .child(content)
    }
}
