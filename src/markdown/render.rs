//! Drawing the blocks: headings, paragraphs with links and pictures, code with its buttons,
//! diagrams, quotes, lists and tables.
use super::{
    Diagram, MarkdownEvent, MarkdownPane, TEXT_SIZE, Target,
    parse::{Block, Inline, Style},
};
use crate::{
    theme::{self, hsla},
    widgets,
};
use gpui::{
    AnyElement, App, Context, Font, FontStyle, FontWeight, InteractiveText, StrikethroughStyle,
    StyledText, TextRun, UnderlineStyle, div, img, prelude::*, px,
};

/// Code blocks in these languages get Warp's "insert into terminal" button.
fn is_shell(language: &str) -> bool {
    ["sh", "bash", "zsh", "shell", "console", "fish"].contains(&language)
}

impl MarkdownPane {
    pub(super) fn render_block(
        &self,
        block: &Block,
        ids: &mut usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        if language == "mermaid" {
            match self.diagrams.get(text) {
                Some(Diagram::Ready(image)) => {
                    return div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(img(image.clone()).max_w_full())
                        .into_any_element();
                }
                Some(Diagram::Pending) | None => {
                    return div()
                        .text_color(hsla(theme.text_muted))
                        .child("Drawing diagram…")
                        .into_any_element();
                }
                // Not drawable: the source as a code block, with why below.
                Some(Diagram::Failed(error)) => {
                    let error = format!("Mermaid: {error}");
                    return div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(self.render_source(language, text, ids, cx))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(hsla(theme.text_muted))
                                .child(error),
                        )
                        .into_any_element();
                }
            }
        }
        self.render_source(language, text, ids, cx)
    }

    fn render_source(
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
            widgets::ghost_button(theme, id, label)
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
                    .child(self.selectable(
                        id,
                        text.to_owned(),
                        vec![TextRun {
                            len: text.len(),
                            font: gpui::font(self.mono.clone()),
                            color: hsla(theme.text),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        hsla(theme.accent).opacity(0.3),
                        IntoElement::into_any_element,
                        cx,
                    )),
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
        let targets: Vec<Option<Target>> = text
            .links
            .iter()
            .map(|(_, link)| self.links.get(link).cloned().flatten())
            .collect();
        let ranges = text.links.iter().map(|(range, _)| range.clone()).collect();
        let pane = cx.entity().downgrade();
        let images = text
            .images
            .iter()
            .map(
                |image| match self.pictures.get(&image.source).cloned().flatten() {
                    Some(path) => img(path).max_w_full().into_any_element(),
                    // Only files next to the document: fetching a web image would tell its
                    // server that this file was opened.
                    _ => div()
                        .text_color(hsla(theme.text_muted))
                        .child(format!("[image: {}]", image.alt))
                        .into_any_element(),
                },
            )
            .collect::<Vec<_>>();
        let shade = hsla(theme.accent).opacity(0.3);
        let links = move |styled: StyledText| {
            InteractiveText::new(("md-text", id), styled)
                .on_click(ranges, move |index, _, cx: &mut App| {
                    let Some(Some(target)) = targets.get(index).cloned() else {
                        return;
                    };
                    pane.update(cx, |pane, cx| match target {
                        // A linked document replaces this one, as in a browser.
                        Target::Markdown(path) => pane.navigate(path, cx),
                        Target::Anchor(anchor) => pane.go_to_anchor(&anchor),
                        target => cx.emit(MarkdownEvent::Open(target)),
                    })
                    .ok();
                })
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!text.text.is_empty(), |this| {
                this.child(self.selectable(id, text.text.clone(), runs, shade, links, cx))
            })
            .children(images)
            .into_any_element()
    }
}
