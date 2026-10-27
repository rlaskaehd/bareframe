//! Reading Markdown into blocks of styled text, with pulldown-cmark.
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// Quotes and lists nested deeper than this are read into the level above, so a hostile
/// file can't exhaust the stack while parsing or drawing.
const MAX_DEPTH: usize = 32;

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

/// The source of every Mermaid code block in `blocks`, nested ones included.
pub(super) fn mermaid_sources<'a>(blocks: &'a [Block], out: &mut Vec<&'a str>) {
    for block in blocks {
        match block {
            Block::Code { language, text } if language == "mermaid" => out.push(text),
            Block::Quote(blocks) => mermaid_sources(blocks, out),
            Block::List { items, .. } => {
                for item in items {
                    mermaid_sources(&item.blocks, out);
                }
            }
            _ => {}
        }
    }
}

/// Every run of text in `blocks`, nested ones included.
pub(super) fn inlines<'a>(blocks: &'a [Block], out: &mut Vec<&'a Inline>) {
    for block in blocks {
        match block {
            Block::Heading(_, text) | Block::Paragraph(text) => out.push(text),
            Block::Quote(blocks) => inlines(blocks, out),
            Block::List { items, .. } => {
                for item in items {
                    inlines(&item.blocks, out);
                }
            }
            Block::Table(rows) => out.extend(rows.iter().flatten()),
            Block::Code { .. } | Block::Rule => {}
        }
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
    fn html_is_left_out() {
        assert_eq!(
            parse("<div>\nhidden\n</div>\n\ntext\n"),
            [Block::Paragraph(plain("text"))]
        );
    }
}
