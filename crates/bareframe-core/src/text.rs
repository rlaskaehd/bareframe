//! The code editor's text: lines, positions, edits with undo, and the cursor moves and
//! offsets an editor and macOS input methods need. Lines are kept as a vector of strings;
//! for files of tens of thousands of lines an edit is still a small copy.
use std::{ops::Range, time::Duration, time::Instant};

/// A place in the text: a line and a byte column inside it, always on a char boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// One change: `old` at `start` was replaced by `new`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edit {
    start: Position,
    old: String,
    new: String,
    at: Instant,
}

/// Typing and deleting within this long of the previous key undo together.
const GROUP: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug)]
pub struct Buffer {
    lines: Vec<String>,
    pub line_ending: LineEnding,
    history: Vec<Edit>,
    /// Edits in `history` before this index are applied; the rest are undone (redoable).
    applied: usize,
    /// `applied` when the text last matched the file on disk; `None` if no state does.
    saved: Option<usize>,
    /// Bumped on every change, for caches keyed by content.
    pub version: u64,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        let line_ending = if text.contains("\r\n") {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        };
        let lines = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect();
        Self {
            lines,
            line_ending,
            history: Vec::new(),
            applied: 0,
            saved: Some(0),
            version: 0,
        }
    }

    /// The text as it would be saved, with the file's own line endings.
    pub fn text(&self) -> String {
        self.lines.join(self.line_ending.as_str())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, line: usize) -> &str {
        self.lines.get(line).map_or("", String::as_str)
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    pub fn end(&self) -> Position {
        let last = self.lines.len() - 1;
        Position::new(last, self.lines[last].len())
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.applied)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.applied);
    }

    /// The nearest valid position: inside the text and on a char boundary.
    pub fn clamp(&self, position: Position) -> Position {
        let line = position.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let mut column = position.column.min(text.len());
        while !text.is_char_boundary(column) {
            column -= 1;
        }
        Position::new(line, column)
    }

    pub fn slice(&self, range: Range<Position>) -> String {
        let (start, end) = (self.clamp(range.start), self.clamp(range.end));
        if start.line == end.line {
            return self.lines[start.line][start.column..end.column.max(start.column)].to_owned();
        }
        let mut out = self.lines[start.line][start.column..].to_owned();
        for line in &self.lines[start.line + 1..end.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&self.lines[end.line][..end.column]);
        out
    }

    /// Replaces `range` with `text` and returns where the new text ends. `now` decides
    /// whether this joins the previous edit for undo, as typing a word does.
    pub fn replace(&mut self, range: Range<Position>, text: &str, now: Instant) -> Position {
        let (start, end) = (self.clamp(range.start), self.clamp(range.end));
        let (start, end) = (start.min(end), start.max(end));
        let old = self.slice(start..end);
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if old.is_empty() && text.is_empty() {
            return start;
        }
        let after = self.splice(start, end, &text);
        self.history.truncate(self.applied);
        if self.saved.is_some_and(|saved| saved > self.applied) {
            self.saved = None;
        }
        let edit = Edit {
            start,
            old,
            new: text,
            at: now,
        };
        match self.history.last_mut() {
            // An input method rewriting what it just inserted (한 → 한ㄱ → 한글) amends
            // that edit, so composing a syllable is one undo step.
            Some(last)
                if self.saved != Some(self.applied)
                    && last.start == edit.start
                    && last.new == edit.old
                    && edit.at.duration_since(last.at) <= GROUP =>
            {
                last.new = edit.new;
                last.at = edit.at;
            }
            Some(last) if self.saved != Some(self.applied) && joins(last, &edit) => {
                if edit.old.is_empty() {
                    last.new.push_str(&edit.new);
                } else {
                    // Backspacing: the deleted text grows towards the start.
                    last.start = edit.start;
                    last.old.insert_str(0, &edit.old);
                }
                last.at = edit.at;
            }
            _ => {
                self.history.push(edit);
                self.applied += 1;
            }
        }
        after
    }

    /// The text change itself, without history. Returns where `text` ends.
    fn splice(&mut self, start: Position, end: Position, text: &str) -> Position {
        let tail = self.lines[end.line][end.column..].to_owned();
        let head = self.lines[start.line][..start.column].to_owned();
        let mut pieces = text.split('\n');
        let first = pieces.next().unwrap_or("");
        let mut new_lines = vec![format!("{head}{first}")];
        new_lines.extend(pieces.map(str::to_owned));
        let last = new_lines.len() - 1;
        let after = Position::new(start.line + last, new_lines[last].len());
        new_lines[last].push_str(&tail);
        self.lines.splice(start.line..=end.line, new_lines);
        self.version += 1;
        after
    }

    /// Takes back the last edit; returns the text it restored, as a range to select.
    pub fn undo(&mut self) -> Option<Range<Position>> {
        let edit = self.history.get(self.applied.checked_sub(1)?)?.clone();
        let end = end_of(edit.start, &edit.new);
        let after = self.splice(edit.start, end, &edit.old);
        self.applied -= 1;
        Some(edit.start..after)
    }

    pub fn redo(&mut self) -> Option<Range<Position>> {
        let edit = self.history.get(self.applied)?.clone();
        let end = end_of(edit.start, &edit.old);
        let after = self.splice(edit.start, end, &edit.new);
        self.applied += 1;
        Some(after..after)
    }

    pub fn previous(&self, position: Position) -> Position {
        let position = self.clamp(position);
        if position.column == 0 {
            return match position.line {
                0 => position,
                line => Position::new(line - 1, self.lines[line - 1].len()),
            };
        }
        let text = &self.lines[position.line][..position.column];
        let column = text
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        Position::new(position.line, column)
    }

    pub fn next(&self, position: Position) -> Position {
        let position = self.clamp(position);
        let text = &self.lines[position.line];
        match text[position.column..].chars().next() {
            Some(ch) => Position::new(position.line, position.column + ch.len_utf8()),
            None if position.line + 1 < self.lines.len() => Position::new(position.line + 1, 0),
            None => position,
        }
    }

    /// Option+Left: back to the start of this or the previous word.
    pub fn word_start(&self, position: Position) -> Position {
        let position = self.clamp(position);
        if position.column == 0 {
            return self.previous(position);
        }
        let text = &self.lines[position.line][..position.column];
        let mut chars = text.char_indices().rev().peekable();
        while chars.next_if(|(_, ch)| ch.is_whitespace()).is_some() {}
        let word = chars.peek().is_some_and(|(_, ch)| is_word(*ch));
        let mut column = chars.peek().map_or(0, |(index, _)| *index);
        while let Some((index, _)) =
            chars.next_if(|(_, ch)| is_word(*ch) == word && !ch.is_whitespace())
        {
            column = index;
        }
        Position::new(position.line, column)
    }

    /// Option+Right: on to the end of this or the next word.
    pub fn word_end(&self, position: Position) -> Position {
        let position = self.clamp(position);
        let text = &self.lines[position.line];
        if position.column == text.len() {
            return self.next(position);
        }
        let mut chars = text[position.column..].char_indices().peekable();
        while chars.next_if(|(_, ch)| ch.is_whitespace()).is_some() {}
        let word = chars.peek().is_some_and(|(_, ch)| is_word(*ch));
        let mut column = text.len() - position.column;
        while let Some((index, ch)) = chars.peek().copied() {
            if is_word(ch) != word || ch.is_whitespace() {
                column = index;
                break;
            }
            chars.next();
        }
        Position::new(position.line, position.column + column)
    }

    /// The word around `position`, for a double click.
    pub fn word_at(&self, position: Position) -> Range<Position> {
        let position = self.clamp(position);
        let text = &self.lines[position.line];
        let Some(ch) = text[position.column..].chars().next() else {
            return position..position;
        };
        let kind = |c: char| (c.is_whitespace(), is_word(c));
        let here = kind(ch);
        let start = text[..position.column]
            .char_indices()
            .rev()
            .take_while(|(_, c)| kind(*c) == here)
            .last()
            .map_or(position.column, |(index, _)| index);
        let end = text[position.column..]
            .char_indices()
            .find(|(_, c)| kind(*c) != here)
            .map_or(text.len(), |(index, _)| position.column + index);
        Position::new(position.line, start)..Position::new(position.line, end)
    }

    /// The leading spaces or tabs of a line, which Enter carries to the next.
    pub fn indent(&self, line: usize) -> &str {
        let text = self.line(line);
        &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
    }

    /// Offsets as macOS input methods count them: UTF-16 units over the whole text, with
    /// one unit per line break.
    pub fn to_utf16(&self, position: Position) -> usize {
        let position = self.clamp(position);
        let before: usize = self.lines[..position.line]
            .iter()
            .map(|line| line.encode_utf16().count() + 1)
            .sum();
        before
            + self.lines[position.line][..position.column]
                .encode_utf16()
                .count()
    }

    pub fn from_utf16(&self, mut offset: usize) -> Position {
        for (index, line) in self.lines.iter().enumerate() {
            let length = line.encode_utf16().count();
            if offset <= length {
                let mut units = 0;
                for (column, ch) in line.char_indices() {
                    if units >= offset {
                        return Position::new(index, column);
                    }
                    units += ch.len_utf16();
                }
                return Position::new(index, line.len());
            }
            offset -= length + 1;
        }
        self.end()
    }
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Where `text` ends when inserted at `start`.
fn end_of(start: Position, text: &str) -> Position {
    match text.rsplit_once('\n') {
        Some((before, last)) => {
            Position::new(start.line + before.matches('\n').count() + 1, last.len())
        }
        None => Position::new(start.line, start.column + text.len()),
    }
}

/// Typing continues a word, and backspacing continues a deletion, until a pause, a line
/// break or a jump elsewhere.
fn joins(last: &Edit, edit: &Edit) -> bool {
    if edit.at.duration_since(last.at) > GROUP || edit.new.contains('\n') || edit.old.contains('\n')
    {
        return false;
    }
    let typing = last.old.is_empty() && edit.old.is_empty();
    let deleting = last.new.is_empty() && edit.new.is_empty();
    (typing && end_of(last.start, &last.new) == edit.start && !edit.new.starts_with(' '))
        || (deleting && end_of(edit.start, &edit.old) == last.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, column: usize) -> Position {
        Position::new(line, column)
    }

    #[test]
    fn line_endings_round_trip() {
        assert_eq!(Buffer::new("a\nb\n").text(), "a\nb\n");
        let crlf = Buffer::new("a\r\nb\r\n");
        assert_eq!(crlf.line_ending, LineEnding::CrLf);
        assert_eq!((crlf.line_count(), crlf.line(0)), (3, "a"));
        assert_eq!(crlf.text(), "a\r\nb\r\n");
        assert_eq!(Buffer::new("").line_count(), 1);
    }

    #[test]
    fn replace_across_lines_and_undo_redo() {
        let mut buffer = Buffer::new("hello\nworld\n");
        let now = Instant::now();
        let end = buffer.replace(p(0, 2)..p(1, 3), "y\nnew\nwo", now);
        assert_eq!(buffer.text(), "hey\nnew\nwold\n");
        assert_eq!(end, p(2, 2));
        assert!(buffer.is_dirty());
        assert_eq!(buffer.undo(), Some(p(0, 2)..p(1, 3)));
        assert_eq!(buffer.text(), "hello\nworld\n");
        assert!(
            !buffer.is_dirty(),
            "undoing to the saved text is clean again"
        );
        buffer.redo();
        assert_eq!(buffer.text(), "hey\nnew\nwold\n");
        assert_eq!(buffer.slice(p(0, 1)..p(2, 1)), "ey\nnew\nw");
    }

    #[test]
    fn typing_groups_until_a_pause_space_or_newline() {
        let mut buffer = Buffer::new("");
        let t = Instant::now();
        let mut at = p(0, 0);
        for (index, ch) in "fn main".chars().enumerate() {
            at = buffer.replace(
                at..at,
                &ch.to_string(),
                t + Duration::from_millis(index as u64),
            );
        }
        // "fn" and " main" are two undo steps.
        buffer.undo();
        assert_eq!(buffer.text(), "fn");
        buffer.undo();
        assert_eq!(buffer.text(), "");
        buffer.redo();
        buffer.redo();
        let later = t + Duration::from_secs(5);
        buffer.replace(at..at, "!", later);
        buffer.undo();
        assert_eq!(buffer.text(), "fn main");
    }

    #[test]
    fn composing_hangul_is_one_undo_step() {
        let mut buffer = Buffer::new("x");
        let t = Instant::now();
        let start = p(0, 1);
        let mut end = buffer.replace(start..start, "ㅎ", t);
        for step in ["하", "한", "한ㄱ", "한그", "한글"] {
            end = buffer.replace(start..end, step, t);
        }
        assert_eq!(buffer.text(), "x한글");
        buffer.undo();
        assert_eq!(buffer.text(), "x");
    }

    #[test]
    fn backspaces_group_and_a_new_edit_drops_redo() {
        let mut buffer = Buffer::new("abcdef");
        let t = Instant::now();
        for column in (3..6).rev() {
            buffer.replace(p(0, column)..p(0, column + 1), "", t);
        }
        assert_eq!(buffer.text(), "abc");
        buffer.undo();
        assert_eq!(buffer.text(), "abcdef");
        buffer.replace(p(0, 0)..p(0, 0), "x", t + GROUP * 2);
        assert_eq!(buffer.redo(), None);
    }

    #[test]
    fn moves_respect_chars_lines_and_words() {
        let buffer = Buffer::new("한글 text_1, more\nnext");
        assert_eq!(
            buffer.next(p(0, 0)),
            p(0, 3),
            "a Hangul syllable is one step"
        );
        // "한글 text_1, more" is 19 bytes: two 3-byte syllables, then ASCII.
        assert_eq!(buffer.previous(p(1, 0)), p(0, 19));
        assert_eq!(buffer.next(p(0, 19)), p(1, 0));
        assert_eq!(buffer.word_end(p(0, 0)), p(0, 6));
        assert_eq!(buffer.word_end(p(0, 6)), p(0, 13));
        assert_eq!(
            buffer.word_end(p(0, 13)),
            p(0, 14),
            "punctuation is its own run"
        );
        assert_eq!(buffer.word_start(p(0, 19)), p(0, 15));
        assert_eq!(buffer.word_start(p(0, 15)), p(0, 13));
        assert_eq!(buffer.word_start(p(0, 13)), p(0, 7));
        assert_eq!(buffer.word_at(p(0, 9)), p(0, 7)..p(0, 13));
        assert_eq!(
            buffer.clamp(p(0, 1)),
            p(0, 0),
            "inside a syllable snaps back"
        );
        assert_eq!(buffer.clamp(p(9, 9)), p(1, 4));
    }

    #[test]
    fn utf16_offsets_count_line_breaks() {
        let buffer = Buffer::new("a한\n😀b");
        assert_eq!(buffer.to_utf16(p(0, 4)), 2);
        assert_eq!(buffer.to_utf16(p(1, 0)), 3);
        assert_eq!(buffer.to_utf16(p(1, 4)), 5, "an emoji is two units");
        for position in [p(0, 0), p(0, 1), p(0, 4), p(1, 0), p(1, 4), p(1, 5)] {
            assert_eq!(buffer.from_utf16(buffer.to_utf16(position)), position);
        }
        assert_eq!(buffer.from_utf16(99), p(1, 5));
    }

    #[test]
    fn indent_is_the_leading_whitespace() {
        let buffer = Buffer::new("    let x;\n\tif y {\nz");
        assert_eq!(
            (buffer.indent(0), buffer.indent(1), buffer.indent(2)),
            ("    ", "\t", "")
        );
    }
}
