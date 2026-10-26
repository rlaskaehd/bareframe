//! Fuzzy matching for the command palette: the query's letters in order, anywhere in the
//! text, scored so word starts and runs of letters rank first.

/// The score and the byte offsets of the matched characters, or `None` if `text` lacks
/// one of the query's letters in order. Case and spaces in the query are ignored.
// ponytail: greedy left-to-right match, so "nt" in "Next tab" may pick a later "t" than the
// best one; switch to a dynamic-programming matcher if rankings look wrong.
pub fn fuzzy_match(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let mut wanted = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .peekable();
    let mut score = 0;
    let mut offsets = Vec::new();
    let mut previous: Option<char> = None;
    let mut last_match: Option<usize> = None;
    for (index, (offset, ch)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else {
            break;
        };
        if ch.to_lowercase().eq(std::iter::once(next)) {
            score += 1;
            if previous.is_none_or(|previous| !previous.is_alphanumeric()) {
                score += 8;
            }
            if last_match == Some(index.wrapping_sub(1)) {
                score += 4;
            }
            offsets.push(offset);
            last_match = Some(index);
            wanted.next();
        }
        previous = Some(ch);
    }
    if wanted.peek().is_some() {
        return None;
    }
    // Among equal matches, the shorter text is the closer one.
    Some((score * 100 - text.len() as i32, offsets))
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn letters_in_order_match_and_others_do_not() {
        assert_eq!(
            fuzzy_match("spr", "Split pane right").unwrap().1,
            [0, 1, 11]
        );
        assert!(fuzzy_match("rps", "Split pane right").is_none());
        assert!(fuzzy_match("", "anything").is_some());
        assert_eq!(fuzzy_match("NEW t", "New tab").unwrap().1, [0, 1, 2, 4]);
    }

    #[test]
    fn word_starts_and_runs_rank_first() {
        let score = |query, text| fuzzy_match(query, text).unwrap().0;
        assert!(score("nt", "New tab") > score("nt", "Paint"));
        assert!(score("tab", "New tab") > score("tab", "Activate next tab"));
    }

    #[test]
    fn hangul_matches_by_syllable() {
        assert_eq!(fuzzy_match("작업", "새 작업 공간").unwrap().1, [4, 7]);
    }
}
