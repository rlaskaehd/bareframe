use std::ops::Range;

/// Only the current preedit and not-yet-sent commits are stored here, never a
/// copy of the shell's input line.
#[derive(Default, Debug)]
pub struct Composition {
    text: String,
    selection: Range<usize>,
    /// Committed text held until the key event that produced it finishes.
    committed: String,
    /// Byte offset in `committed` where the latest commit starts.
    last_commit: usize,
}

impl Composition {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn len_utf16(&self) -> usize {
        self.text.encode_utf16().count()
    }
    pub fn active(&self) -> bool {
        !self.text.is_empty()
    }
    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.selection = 0..0;
    }
    pub fn mark(&mut self, text: &str, selection: Option<Range<usize>>) {
        self.text = text.into();
        let length = self.len_utf16();
        self.selection = selection
            .filter(|r| r.start <= r.end && r.end <= length)
            .unwrap_or(length..length);
    }
    /// Replacement and selection offsets are UTF-16, not byte indexes.
    pub fn replace(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> Result<(), &'static str> {
        let range = range.unwrap_or(0..self.len_utf16());
        if range.start > range.end {
            return Err("reversed IME replacement range");
        }
        let start = utf16_to_byte(&self.text, range.start).ok_or("invalid IME range start")?;
        let end = utf16_to_byte(&self.text, range.end).ok_or("invalid IME range end")?;
        let length = text.encode_utf16().count();
        let selected = selected.unwrap_or(length..length);
        if selected.start > selected.end || selected.end > length {
            return Err("invalid IME selection");
        }
        if utf16_to_byte(text, selected.start).is_none()
            || utf16_to_byte(text, selected.end).is_none()
        {
            return Err("IME selection splits a Unicode scalar");
        }
        self.text.replace_range(start..end, text);
        self.selection = range.start + selected.start..range.start + selected.end;
        Ok(())
    }
    /// Ends the composition with `text` replacing `range` and holds the result
    /// for delivery. The IME may pass the triggering key on after committing.
    pub fn commit(&mut self, range: Option<Range<usize>>, text: &str) -> Result<(), &'static str> {
        self.replace(range, text, None)?;
        self.last_commit = self.committed.len();
        self.committed.push_str(&self.text);
        self.clear();
        Ok(())
    }
    pub fn take_committed(&mut self) -> Option<String> {
        self.last_commit = 0;
        (!self.committed.is_empty()).then(|| std::mem::take(&mut self.committed))
    }
    /// The Korean IME commits a lone jamo and then passes Backspace on. That
    /// Backspace deletes the unsent commit, so neither reaches the terminal.
    pub fn backspace_discards_commit(&mut self) -> bool {
        if self.committed.is_empty() {
            return false;
        }
        self.committed.truncate(self.last_commit);
        self.last_commit = 0;
        true
    }
    pub fn slice(&self, range: Range<usize>) -> Option<String> {
        let start = utf16_to_byte(&self.text, range.start)?;
        let end = utf16_to_byte(&self.text, range.end)?;
        self.text.get(start..end).map(Into::into)
    }
}

fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
    }
    (units == offset).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ime_ranges_use_utf16_and_never_split_a_surrogate_pair() {
        let mut ime = Composition::default();
        ime.mark("한🙂글", Some(1..3));
        assert_eq!(ime.len_utf16(), 4);
        assert_eq!(ime.slice(1..3).as_deref(), Some("🙂"));
        assert_eq!(ime.slice(1..2), None);
        assert_eq!(ime.selection(), 1..3);
    }
    #[test]
    fn repeated_composition_replaces_preedit_and_cancel_discards_it() {
        let mut ime = Composition::default();
        for text in ["ㅎ", "하", "한"] {
            ime.mark(text, None);
        }
        assert_eq!(ime.text(), "한");
        ime.clear();
        assert!(!ime.active());
        assert_eq!(ime.selection(), 0..0);
    }
    #[test]
    fn backspace_after_a_lone_jamo_commit_sends_nothing() {
        let mut ime = Composition::default();
        ime.mark("ㅎ", None);
        ime.commit(None, "ㅎ").unwrap();
        assert!(!ime.active());
        assert!(ime.backspace_discards_commit());
        assert_eq!(ime.take_committed(), None);
        // Without a pending commit Backspace still reaches the terminal.
        assert!(!ime.backspace_discards_commit());
    }
    #[test]
    fn commits_are_delivered_in_order_and_backspace_drops_only_the_latest() {
        let mut ime = Composition::default();
        ime.mark("한", None);
        ime.commit(None, "한").unwrap();
        ime.mark("ㄱ", None);
        ime.commit(None, "ㄱ").unwrap();
        assert!(ime.backspace_discards_commit());
        assert_eq!(ime.take_committed().as_deref(), Some("한"));
        ime.commit(None, "글 ").unwrap();
        assert_eq!(ime.take_committed().as_deref(), Some("글 "));
    }
    #[test]
    fn partial_preedit_replacement_preserves_surrounding_composition() {
        let mut ime = Composition::default();
        ime.mark("한🙂글", None);
        ime.replace(Some(1..3), "국", Some(1..1)).unwrap();
        assert_eq!(ime.text(), "한국글");
        assert_eq!(ime.selection(), 2..2);
        assert!(ime.replace(Some(2..8), "", None).is_err());
        assert_eq!(ime.text(), "한국글");
    }
}
