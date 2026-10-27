//! Syntax colors for the code editor: tree-sitter grammars (bundled by `arborium`) turn a
//! file into spans, flattened here into one color class per stretch of each line.
use arborium::{GrammarStore, Highlighter};
use bareframe_core::git::{Hunk, LineKind};
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

/// Larger files are left plain; parsing them on every edit would lag.
pub const MAX_HIGHLIGHT: usize = 2 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Tag,
    Builtin,
    /// Markdown `**bold**`, drawn bold.
    Strong,
    /// Markdown `*italics*`, drawn slanted.
    Emphasis,
}

/// A line's colored stretches: byte ranges into the line, in order, not overlapping.
pub type LineSyntax = Vec<(Range<usize>, Syntax)>;

/// The terminal palette's colors, so code looks like the rest of the theme.
pub fn color(theme: &crate::theme::Theme, syntax: Syntax) -> u32 {
    match syntax {
        Syntax::Keyword => theme.palette[5],
        Syntax::String => theme.palette[2],
        Syntax::Comment => theme.text_muted,
        Syntax::Number => theme.palette[3],
        Syntax::Function => theme.palette[4],
        Syntax::Type => theme.palette[6],
        Syntax::Tag | Syntax::Builtin => theme.palette[1],
        Syntax::Strong => theme.palette[3],
        Syntax::Emphasis => theme.palette[6],
    }
}

pub fn language_for(path: &std::path::Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    match name {
        "Dockerfile" => Some("dockerfile"),
        ".zshrc" | ".bashrc" | ".zshenv" | ".profile" => Some("bash"),
        _ => arborium::detect_language(name),
    }
}

/// Compiled grammars are shared by every editor and kept for the app's life.
fn store() -> Arc<GrammarStore> {
    static STORE: OnceLock<Arc<GrammarStore>> = OnceLock::new();
    STORE.get_or_init(|| Arc::new(GrammarStore::new())).clone()
}

fn class(capture: &str) -> Option<Syntax> {
    let head = capture.split('.').next().unwrap_or(capture);
    Some(match head {
        "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass" => {
            Syntax::Keyword
        }
        "string" | "character" | "escape" => Syntax::String,
        "comment" => Syntax::Comment,
        "number" | "float" | "boolean" | "constant" => Syntax::Number,
        "function" | "method" | "macro" => Syntax::Function,
        "type" | "constructor" | "namespace" | "module" => Syntax::Type,
        "tag" | "attribute" | "label" => Syntax::Tag,
        "variable" if capture.contains("builtin") => Syntax::Builtin,
        "text" if capture.contains("title") || capture.contains("strong") => Syntax::Keyword,
        "text" if capture.contains("literal") || capture.contains("uri") => Syntax::String,
        "markup" if capture.contains("heading") => Syntax::Keyword,
        "markup" if capture.contains("raw") || capture.contains("link") => Syntax::String,
        _ => return None,
    })
}

/// Bold, italics, inline code and links: the bundled Markdown grammar marks only blocks and
/// leaves these to an inline grammar it lacks, so the viewer's Markdown parser finds them.
fn markdown_inline(text: &str, classes: &mut [Option<Syntax>]) {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let mut spans: Vec<(Range<usize>, Syntax)> = Parser::new_ext(text, Options::all())
        .into_offset_iter()
        .filter_map(|(event, range)| {
            let syntax = match event {
                Event::Start(Tag::Link { .. }) => Syntax::Function,
                Event::Start(Tag::Emphasis) => Syntax::Emphasis,
                Event::Start(Tag::Strong) => Syntax::Strong,
                Event::Code(_) => Syntax::String,
                _ => return None,
            };
            Some((range, syntax))
        })
        .collect();
    // Wider first, so bold inside a link still shows bold.
    spans.sort_by_key(|(range, _)| std::cmp::Reverse(range.len()));
    for (range, syntax) in spans {
        classes[range.start.min(text.len())..range.end.min(text.len())].fill(Some(syntax));
    }
}

/// Colors for every line of `text`, or `None` if the language has no grammar here.
pub fn highlight(language: &str, text: &str) -> Option<Vec<LineSyntax>> {
    let mut highlighter = Highlighter::with_store(store());
    let mut spans = highlighter.highlight_spans(language, text).ok()?;
    // Wider spans first, so a nested one (a string inside a macro call) paints over them.
    spans.sort_by_key(|span| (std::cmp::Reverse(span.end - span.start), span.pattern_index));
    let mut classes: Vec<Option<Syntax>> = vec![None; text.len()];
    for span in &spans {
        let Some(syntax) = class(&span.capture) else {
            continue;
        };
        classes[span.start as usize..(span.end as usize).min(text.len())].fill(Some(syntax));
    }
    if language == "markdown" {
        markdown_inline(text, &mut classes);
    }
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let mut runs: LineSyntax = Vec::new();
        for (index, class) in classes[offset..offset + line.len()].iter().enumerate() {
            let Some(class) = class else {
                continue;
            };
            match runs.last_mut() {
                Some((range, last)) if *last == *class && range.end == index => range.end += 1,
                _ => runs.push((index..index + 1, *class)),
            }
        }
        lines.push(runs);
        offset += line.len() + 1;
    }
    Some(lines)
}

/// Colors for a diff hunk's lines. The old side (context and removed lines) and the new
/// side (context and added lines) are each parsed as one text, so every line is colored
/// in the code it belongs to; context lines take the new side's colors.
pub fn highlight_hunk(language: &str, hunk: &Hunk) -> Vec<LineSyntax> {
    let mut colors = vec![Vec::new(); hunk.lines.len()];
    let sides: [fn(LineKind) -> bool; 2] = [
        |kind| matches!(kind, LineKind::Context | LineKind::Removed),
        |kind| matches!(kind, LineKind::Context | LineKind::Added),
    ];
    for on_side in sides {
        let picked: Vec<usize> = (0..hunk.lines.len())
            .filter(|&index| on_side(hunk.lines[index].kind))
            .collect();
        let text = picked
            .iter()
            .map(|&index| hunk.lines[index].text.trim_end_matches('\r'))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(lines) = highlight(language, &text) {
            for (line, index) in lines.into_iter().zip(picked) {
                colors[index] = line;
            }
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_gets_keywords_strings_and_comments() {
        let text = "// note\nfn main() {\n    let s = \"hi\";\n}\n";
        let lines = highlight("rust", text).expect("rust grammar");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], [(0..7, Syntax::Comment)]);
        assert!(
            lines[1].contains(&(0..2, Syntax::Keyword)),
            "{:?}",
            lines[1]
        );
        assert!(
            lines[1]
                .iter()
                .any(|(range, syntax)| *syntax == Syntax::Function && *range == (3..7))
        );
        assert!(
            lines[2]
                .iter()
                .any(|(range, syntax)| *syntax == Syntax::String && range.start == 12)
        );
    }

    #[test]
    fn hunks_are_colored_on_their_own_side() {
        let line = |kind, text: &str| bareframe_core::git::Line {
            kind,
            old: None,
            new: None,
            text: text.into(),
        };
        let hunk = Hunk {
            header: "@@ -1,2 +1,2 @@".into(),
            lines: vec![
                line(LineKind::Context, "fn main() {"),
                line(LineKind::Removed, "    let a = \"old\";"),
                line(LineKind::Added, "    // new\r"),
                line(LineKind::NoNewline, "\\ No newline at end of file"),
            ],
        };
        let colors = highlight_hunk("rust", &hunk);
        assert_eq!(colors.len(), 4);
        assert!(
            colors[0].contains(&(0..2, Syntax::Keyword)),
            "{:?}",
            colors[0]
        );
        assert!(
            colors[1]
                .iter()
                .any(|(_, syntax)| *syntax == Syntax::String)
        );
        assert_eq!(colors[2], [(4..10, Syntax::Comment)]);
        assert!(colors[3].is_empty());
    }

    #[test]
    fn markdown_marks_bold_italics_code_and_links() {
        let lines = highlight(
            "markdown",
            "# Title\n\nSome **HI**, *it*, `x` and [a](#b).\n",
        )
        .expect("markdown grammar");
        assert!(
            lines[0]
                .iter()
                .any(|(_, syntax)| *syntax == Syntax::Keyword)
        );
        let line = &lines[2];
        assert!(line.contains(&(5..11, Syntax::Strong)), "{line:?}");
        assert!(line.contains(&(13..17, Syntax::Emphasis)), "{line:?}");
        assert!(line.contains(&(19..22, Syntax::String)), "{line:?}");
        assert!(line.contains(&(27..34, Syntax::Function)), "{line:?}");
    }

    #[test]
    fn languages_come_from_file_names() {
        assert_eq!(language_for("src/main.rs".as_ref()), Some("rust"));
        assert_eq!(language_for("Dockerfile".as_ref()), Some("dockerfile"));
        assert_eq!(language_for("notes.unknown".as_ref()), None);
        assert!(highlight("no-such-language", "x").is_none());
    }
}
