//! Reading git's output for the Code review panel: `status --porcelain=v2 -z`,
//! `diff --numstat -z` and unified diffs, and cutting a single hunk back out as a patch.
//! Running git is the app's job; everything here is plain text in, data out.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed {
        from: String,
    },
    TypeChanged,
    /// Both sides changed it in a merge; it can't be committed until resolved.
    Conflicted,
    Untracked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the repository root, `/`-separated, exactly as git prints it.
    pub path: String,
    pub change: Change,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    /// `None` before the first commit.
    pub head: Option<String>,
    pub entries: Vec<Entry>,
}

impl Status {
    pub fn conflicted(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.change == Change::Conflicted)
    }
}

/// `git status --porcelain=v2 -z --branch --untracked-files=all`.
pub fn parse_status(out: &str) -> Status {
    let mut status = Status::default();
    let mut fields = out.split('\0');
    while let Some(record) = fields.next() {
        if let Some(header) = record.strip_prefix("# ") {
            match header.split_once(' ') {
                Some(("branch.head", name)) if name != "(detached)" => {
                    status.branch = Some(name.to_owned())
                }
                Some(("branch.oid", oid)) if oid != "(initial)" => {
                    status.head = Some(oid.to_owned())
                }
                _ => {}
            }
            continue;
        }
        let kind = record.split(' ').next().unwrap_or("");
        // Ordinary entries have 8 fields before the path, renames 9, unmerged 10.
        let (path, change) = match kind {
            "1" => {
                let Some(path) = nth_rest(record, 8) else {
                    continue;
                };
                let xy = record.get(2..4).unwrap_or("..");
                (path, change_of(xy))
            }
            "2" => {
                let Some(path) = nth_rest(record, 9) else {
                    continue;
                };
                let from = fields.next().unwrap_or("").to_owned();
                (path, Change::Renamed { from })
            }
            "u" => {
                let Some(path) = nth_rest(record, 10) else {
                    continue;
                };
                (path, Change::Conflicted)
            }
            "?" => match record.get(2..) {
                Some(path) => (path, Change::Untracked),
                None => continue,
            },
            _ => continue,
        };
        status.entries.push(Entry {
            path: path.to_owned(),
            change,
        });
    }
    status
}

/// What the change looks like next to HEAD, whether staged or not.
fn change_of(xy: &str) -> Change {
    let mut codes = xy.chars();
    let (x, y) = (codes.next().unwrap_or('.'), codes.next().unwrap_or('.'));
    match (x, y) {
        ('A', _) => Change::Added,
        ('D', _) | (_, 'D') => Change::Deleted,
        ('T', _) | (_, 'T') => Change::TypeChanged,
        _ => Change::Modified,
    }
}

/// The rest of `record` after `count` space-separated fields: a path may hold spaces.
fn nth_rest(record: &str, count: usize) -> Option<&str> {
    let mut rest = record;
    for _ in 0..count {
        rest = rest.split_once(' ')?.1;
    }
    Some(rest)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub path: String,
    /// `None` for a binary file.
    pub lines: Option<(u64, u64)>,
}

/// `git diff --numstat -z`: `added\tremoved\tpath\0`, or for a rename
/// `added\tremoved\t\0from\0to\0`.
pub fn parse_numstat(out: &str) -> Vec<Stat> {
    let mut stats = Vec::new();
    let mut fields = out.split('\0');
    while let Some(record) = fields.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            fields.next();
            fields.next().unwrap_or("").to_owned()
        } else {
            path.to_owned()
        };
        let lines = added.parse().ok().zip(removed.parse().ok());
        stats.push(Stat { path, lines });
    }
    stats
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// `\ No newline at end of file`, kept so a hunk's patch still applies.
    NoNewline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// Line numbers in the old and new file; the side a line isn't on has none.
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ …` line.
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// `diff --git`, `index`, `---`, `+++` and mode lines, needed to apply a hunk.
    pub header: Vec<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn counts(&self) -> (u64, u64) {
        let mut counts = (0, 0);
        for line in self.hunks.iter().flat_map(|hunk| &hunk.lines) {
            match line.kind {
                LineKind::Added => counts.0 += 1,
                LineKind::Removed => counts.1 += 1,
                _ => {}
            }
        }
        counts
    }

    /// One hunk as a patch of its own, for `git apply -R` to take back just that change.
    pub fn hunk_patch(&self, index: usize) -> Option<String> {
        let hunk = self.hunks.get(index)?;
        let mut patch = self.header.join("\n");
        patch.push('\n');
        patch.push_str(&hunk.header);
        patch.push('\n');
        for line in &hunk.lines {
            match line.kind {
                LineKind::Context => patch.push(' '),
                LineKind::Added => patch.push('+'),
                LineKind::Removed => patch.push('-'),
                LineKind::NoNewline => {}
            }
            patch.push_str(&line.text);
            patch.push('\n');
        }
        Some(patch)
    }
}

/// A unified diff (`git diff`, one or more files) split into files and hunks.
pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old, mut new) = (0, 0);
    for line in text.lines() {
        if line.starts_with("diff --git ") || files.is_empty() {
            if line.starts_with("diff --git ") {
                files.push(FileDiff::default());
            } else {
                continue;
            }
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(header) = line.strip_prefix("@@ ") {
            let (from, to) = hunk_starts(header);
            (old, new) = (from, to);
            file.hunks.push(Hunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = file.hunks.last_mut() else {
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                file.binary = true;
            }
            file.header.push(line.to_owned());
            continue;
        };
        let (kind, text) = match line.split_at_checked(1) {
            Some((" ", text)) | Some(("", text)) => (LineKind::Context, text),
            Some(("+", text)) => (LineKind::Added, text),
            Some(("-", text)) => (LineKind::Removed, text),
            Some(("\\", _)) => (LineKind::NoNewline, line),
            _ => (LineKind::Context, line),
        };
        let (old_number, new_number) = match kind {
            LineKind::Context => {
                old += 1;
                new += 1;
                (Some(old - 1), Some(new - 1))
            }
            LineKind::Added => {
                new += 1;
                (None, Some(new - 1))
            }
            LineKind::Removed => {
                old += 1;
                (Some(old - 1), None)
            }
            LineKind::NoNewline => (None, None),
        };
        hunk.lines.push(Line {
            kind,
            old: old_number,
            new: new_number,
            text: text.to_owned(),
        });
    }
    files
}

/// `-12,3 +14,5 @@ fn x` → `(12, 14)`. A missing count means one line.
fn hunk_starts(header: &str) -> (u32, u32) {
    let start = |part: Option<&str>, sign: char| {
        part.and_then(|part| part.strip_prefix(sign))
            .and_then(|part| part.split(',').next())
            .and_then(|number| number.parse().ok())
            .unwrap_or(0)
    };
    let mut parts = header.split(' ');
    (start(parts.next(), '-'), start(parts.next(), '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_branch_entries_and_paths_with_spaces() {
        let out = [
            "# branch.oid 1234abcd",
            "# branch.head main",
            "1 .M N... 100644 100644 100644 aaa bbb src/a file.rs",
            "1 A. N... 000000 100644 100644 000 ccc new.rs",
            "1 .D N... 100644 100644 000000 ddd ddd gone.rs",
            "2 R. N... 100644 100644 100644 eee eee R100 to.rs",
            "from.rs",
            "u UU N... 100644 100644 100644 100644 f1 f2 f3 both.rs",
            "? 새 파일.txt",
            "",
        ]
        .join("\0");
        let status = parse_status(&out);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.head.as_deref(), Some("1234abcd"));
        let changes: Vec<(&str, &Change)> = status
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), &entry.change))
            .collect();
        assert_eq!(
            changes,
            [
                ("src/a file.rs", &Change::Modified),
                ("new.rs", &Change::Added),
                ("gone.rs", &Change::Deleted),
                (
                    "to.rs",
                    &Change::Renamed {
                        from: "from.rs".into()
                    }
                ),
                ("both.rs", &Change::Conflicted),
                ("새 파일.txt", &Change::Untracked),
            ]
        );
        assert!(status.conflicted());
        let fresh = parse_status("# branch.oid (initial)\0# branch.head (detached)\0");
        assert_eq!((fresh.head, fresh.branch), (None, None));
    }

    #[test]
    fn numstat_reads_counts_binaries_and_renames() {
        let out = "3\t1\tsrc/a.rs\0-\t-\timg.png\x0010\t2\t\0old.rs\0new.rs\0";
        assert_eq!(
            parse_numstat(out),
            [
                Stat {
                    path: "src/a.rs".into(),
                    lines: Some((3, 1))
                },
                Stat {
                    path: "img.png".into(),
                    lines: None
                },
                Stat {
                    path: "new.rs".into(),
                    lines: Some((10, 2))
                },
            ]
        );
    }

    const DIFF: &str = "diff --git a/a.txt b/a.txt
index 111..222 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
@@ -10,2 +10,3 @@ tail
 ten
+ten and a half
 eleven
\\ No newline at end of file
diff --git a/b.bin b/b.bin
index 333..444 100644
Binary files a/b.bin and b/b.bin differ
";

    #[test]
    fn diff_splits_files_hunks_and_numbers_lines() {
        let files = parse_diff(DIFF);
        assert_eq!(files.len(), 2);
        let file = &files[0];
        assert_eq!(file.header.len(), 4);
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(file.counts(), (2, 1));
        let first = &file.hunks[0].lines;
        assert_eq!(
            (first[1].kind, first[1].old, first[1].new),
            (LineKind::Removed, Some(2), None)
        );
        assert_eq!(
            (first[2].kind, first[2].old, first[2].new),
            (LineKind::Added, None, Some(2))
        );
        assert_eq!((first[3].old, first[3].new), (Some(3), Some(3)));
        let second = &file.hunks[1].lines;
        assert_eq!((second[1].new, second[2].old), (Some(11), Some(11)));
        assert_eq!(second[3].kind, LineKind::NoNewline);
        assert!(files[1].binary && files[1].hunks.is_empty());
    }

    #[test]
    fn a_hunk_becomes_a_patch_of_its_own() {
        let files = parse_diff(DIFF);
        let patch = files[0].hunk_patch(1).unwrap();
        assert_eq!(
            patch,
            "diff --git a/a.txt b/a.txt\nindex 111..222 100644\n--- a/a.txt\n+++ b/a.txt\n\
             @@ -10,2 +10,3 @@ tail\n ten\n+ten and a half\n eleven\n\\ No newline at end of file\n"
        );
        assert!(files[0].hunk_patch(2).is_none());
    }
}
