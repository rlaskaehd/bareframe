//! Syntax colors for the code editor: tree-sitter grammars (bundled by `arborium`) turn a
//! file into spans, flattened here into one color class per stretch of each line.
use arborium::{GrammarStore, Highlighter};
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
}

/// A line's colored stretches: byte ranges into the line, in order, not overlapping.
pub type LineSyntax = Vec<(Range<usize>, Syntax)>;

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
        let range = span.start as usize..(span.end as usize).min(text.len());
        for slot in &mut classes[range] {
            *slot = Some(syntax);
        }
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
    fn languages_come_from_file_names() {
        assert_eq!(language_for("src/main.rs".as_ref()), Some("rust"));
        assert_eq!(language_for("Dockerfile".as_ref()), Some("dockerfile"));
        assert_eq!(language_for("notes.unknown".as_ref()), None);
        assert!(highlight("no-such-language", "x").is_none());
    }
}
