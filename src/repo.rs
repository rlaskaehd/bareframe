//! Running git for the diff button and the Code review panel. Everything here blocks, so
//! callers run it on the background executor.
//!
//! A refresh runs on its own, without the user asking, so a repository's config must not
//! be able to start programs through it: every read switches off fsmonitor hooks,
//! external diff tools and textconv filters. Commits run the repository's hooks as usual,
//! since the user asked for them.
use bareframe_core::git::{self, Change, Entry, FileDiff, Status};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// What `git diff` compares against before the first commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// A file's diff past these is folded away, as Warp does, rather than laid out.
const MAX_DIFF: usize = 4 << 20;
const MAX_LINE: usize = 5000;
/// Untracked files are diffed one git call each; past this many they are only listed.
const MAX_UNTRACKED: usize = 200;
const MAX_COUNTED: u64 = 1 << 20;

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=off"])
        .arg("--no-pager")
        .stdin(Stdio::null());
    command
}

/// Runs git and returns its output; exit codes in `ok` count as success.
fn run_with(root: &Path, args: &[&str], ok: &[i32]) -> Result<String, String> {
    let output = git(root)
        .args(args)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if output.status.code().is_some_and(|code| ok.contains(&code)) {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(error
            .trim()
            .lines()
            .last()
            .unwrap_or("git failed")
            .to_owned())
    }
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    run_with(root, args, &[0])
}

/// The repository holding `dir`, if any.
pub fn root_of(dir: &Path) -> Option<PathBuf> {
    let out = run(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    Some(PathBuf::from(out.trim_end_matches('\n')))
}

fn status(root: &Path) -> Result<Status, String> {
    let out = run(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    Ok(git::parse_status(&out))
}

/// The diff button's numbers: changed files and lines against HEAD, untracked included.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub status: Status,
    pub added: u64,
    pub removed: u64,
}

pub fn summary(root: &Path) -> Result<Summary, String> {
    let status = status(root)?;
    let head = status.head.as_deref().unwrap_or(EMPTY_TREE);
    let stats = run(
        root,
        &[
            "diff",
            "--numstat",
            "-z",
            "-M",
            "--no-ext-diff",
            "--no-textconv",
            head,
        ],
    )?;
    let (mut added, mut removed) = (0, 0);
    for stat in git::parse_numstat(&stats) {
        if let Some((plus, minus)) = stat.lines {
            added += plus;
            removed += minus;
        }
    }
    for entry in &status.entries {
        if entry.change == Change::Untracked {
            added += count_lines(&root.join(&entry.path));
        }
    }
    Ok(Summary {
        status,
        added,
        removed,
    })
}

/// Lines in a new text file; large or binary files count as none, as git's numstat does.
fn count_lines(path: &Path) -> u64 {
    if std::fs::metadata(path).map_or(true, |metadata| metadata.len() > MAX_COUNTED) {
        return 0;
    }
    match std::fs::read(path) {
        Ok(bytes) if !bytes.contains(&0) => {
            let lines = bytes.iter().filter(|byte| **byte == b'\n').count();
            (lines + usize::from(bytes.last().is_some_and(|byte| *byte != b'\n'))) as u64
        }
        _ => 0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    /// The working tree against HEAD, staged or not.
    Uncommitted,
    /// The working tree against where it branched off this branch.
    Branch(String),
}

#[derive(Clone, Debug)]
pub struct FileReview {
    pub path: String,
    pub change: Change,
    /// `None` when there is nothing to show: binary, too large, or not diffed.
    pub diff: Option<FileDiff>,
    pub note: Option<&'static str>,
    pub counts: (u64, u64),
}

pub struct Review {
    pub status: Status,
    pub files: Vec<FileReview>,
}

pub fn review(root: &Path, base: &Base) -> Result<Review, String> {
    let status = status(root)?;
    let head = status.head.clone().unwrap_or_else(|| EMPTY_TREE.to_owned());
    let rev = match base {
        Base::Uncommitted => head,
        Base::Branch(branch) => run(root, &["merge-base", "HEAD", branch])?
            .trim()
            .to_owned(),
    };
    let flags = ["-M", "--no-ext-diff", "--no-textconv", "--no-color"];
    let mut numstat = vec!["diff", "--numstat", "-z"];
    numstat.extend(flags);
    numstat.push(&rev);
    let stats = git::parse_numstat(&run(root, &numstat)?);
    let mut patch = vec!["diff"];
    patch.extend(flags);
    patch.push(&rev);
    let diffs = git::parse_diff(&run(root, &patch)?);
    let mut files = Vec::new();
    // Both listings come in git's path order, so the n-th patch is the n-th path.
    for (index, stat) in stats.iter().enumerate() {
        let diff = diffs
            .get(index)
            .cloned()
            .filter(|_| stats.len() == diffs.len());
        let change = status
            .entries
            .iter()
            .find(|entry| entry.path == stat.path)
            .map(|entry| entry.change.clone())
            .unwrap_or_else(|| change_from(diff.as_ref()));
        files.push(file_review(stat.path.clone(), change, diff));
    }
    for entry in status
        .entries
        .iter()
        .filter(|entry| entry.change == Change::Untracked)
    {
        let diff = if files.len() < stats.len() + MAX_UNTRACKED {
            run_with(
                root,
                &[
                    "diff",
                    "--no-index",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--",
                    "/dev/null",
                    &entry.path,
                ],
                &[0, 1],
            )
            .ok()
            .and_then(|out| git::parse_diff(&out).into_iter().next())
        } else {
            None
        };
        files.push(file_review(entry.path.clone(), Change::Untracked, diff));
    }
    Ok(Review { status, files })
}

/// A file changed in commits since the branch point isn't in `status`; its patch says how.
fn change_from(diff: Option<&FileDiff>) -> Change {
    let has = |prefix: &str| {
        diff.is_some_and(|diff| diff.header.iter().any(|line| line.starts_with(prefix)))
    };
    if has("new file mode") {
        Change::Added
    } else if has("deleted file mode") {
        Change::Deleted
    } else {
        Change::Modified
    }
}

fn file_review(path: String, change: Change, diff: Option<FileDiff>) -> FileReview {
    let counts = diff.as_ref().map_or((0, 0), FileDiff::counts);
    let (diff, note) = match diff {
        Some(diff) if diff.binary => (None, Some("Binary file")),
        Some(diff) => {
            let lines = diff.hunks.iter().flat_map(|hunk| &hunk.lines);
            let size: usize = lines.clone().map(|line| line.text.len() + 1).sum();
            if size > MAX_DIFF || lines.clone().any(|line| line.text.len() > MAX_LINE) {
                (None, Some("Diff too large to show"))
            } else if diff.hunks.is_empty() {
                (None, Some("No content changes"))
            } else {
                (Some(diff), None)
            }
        }
        None => (None, Some("Not shown")),
    };
    FileReview {
        path,
        change,
        diff,
        note,
        counts,
    }
}

pub fn branches(root: &Path) -> Vec<String> {
    run(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map(|out| out.lines().map(str::to_owned).collect())
    .unwrap_or_default()
}

/// Moves a file to the Trash, where it can be put back, rather than deleting it.
fn trash(path: &Path) -> Result<(), String> {
    use trash::{
        TrashContext,
        macos::{DeleteMethod, TrashContextExtMacos},
    };
    let mut context = TrashContext::default();
    // Finder would need the user's permission to be scripted; NSFileManager doesn't.
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
        .delete(path)
        .map_err(|error| format!("moving {} to the Trash: {error}", path.display()))
}

/// Takes a file back to HEAD, staged and unstaged alike. A file new to the repository goes
/// to the Trash.
pub fn revert_file(root: &Path, entry: &Entry) -> Result<(), String> {
    let restore = |path: &str| {
        run(
            root,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                path,
            ],
        )
    };
    let forget = |path: &str| run(root, &["rm", "--cached", "--quiet", "--", path]);
    match &entry.change {
        Change::Conflicted => Err("Resolve the conflict first.".into()),
        Change::Untracked => trash(&root.join(&entry.path)),
        Change::Added => {
            forget(&entry.path)?;
            trash(&root.join(&entry.path))
        }
        Change::Renamed { from } => {
            restore(from)?;
            forget(&entry.path)?;
            trash(&root.join(&entry.path))
        }
        Change::Modified | Change::Deleted | Change::TypeChanged => restore(&entry.path).map(drop),
    }
}

/// Takes one hunk back out of the working tree.
pub fn revert_hunk(root: &Path, patch: &str) -> Result<(), String> {
    let mut child = git(root)
        .args(["apply", "-R", "--whitespace=nowarn", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("git: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patch.as_bytes())
            .map_err(|error| format!("git apply: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("git apply: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(error
            .trim()
            .lines()
            .last()
            .unwrap_or("git apply failed")
            .to_owned())
    }
}

/// Why a commit can't be made now, if anything stands in the way.
pub fn commit_blocker(root: &Path, status: &Status) -> Option<&'static str> {
    if status.conflicted() {
        return Some("Resolve the merge conflicts before committing.");
    }
    let dir = run(root, &["rev-parse", "--absolute-git-dir"]).ok()?;
    let dir = Path::new(dir.trim_end_matches('\n'));
    [
        (
            "MERGE_HEAD",
            "A merge is in progress; finish it in the terminal.",
        ),
        (
            "rebase-merge",
            "A rebase is in progress; finish it in the terminal.",
        ),
        (
            "rebase-apply",
            "A rebase is in progress; finish it in the terminal.",
        ),
        (
            "CHERRY_PICK_HEAD",
            "A cherry-pick is in progress; finish it in the terminal.",
        ),
        (
            "REVERT_HEAD",
            "A revert is in progress; finish it in the terminal.",
        ),
    ]
    .into_iter()
    .find(|(name, _)| dir.join(name).exists())
    .map(|(_, reason)| reason)
}

/// Stages `paths` (every change if empty) and commits them. Returns git's one-line summary.
pub fn commit(root: &Path, message: &str, paths: &[String]) -> Result<String, String> {
    let mut add = vec!["add", "--all", "--"];
    add.extend(paths.iter().map(String::as_str));
    if paths.is_empty() {
        add.push(".");
    }
    run(root, &add)?;
    let mut commit = vec!["commit", "--quiet", "-m", message];
    if !paths.is_empty() {
        // Only these paths, even if other changes were staged before.
        commit.push("--");
        commit.extend(paths.iter().map(String::as_str));
    }
    run(root, &commit)?;
    run(root, &["log", "-1", "--format=%h %s"]).map(|out| out.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway repository under the system temp directory, removed on drop.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("bareframe-repo-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let repo = Self(root);
            repo.git(&["init", "--quiet", "--initial-branch=main"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "Test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        fn git(&self, args: &[&str]) {
            run(&self.0, args).unwrap();
        }

        fn write(&self, path: &str, text: &str) {
            std::fs::write(self.0.join(path), text).unwrap();
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn summary_review_hunk_revert_and_commit() {
        let repo = Repo::new("review");
        repo.write(
            "a.txt",
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
        );
        repo.git(&["add", "a.txt"]);
        repo.git(&["commit", "--quiet", "-m", "first"]);
        repo.write(
            "a.txt",
            "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
        );
        repo.write("new.txt", "x\ny\n");

        let first = summary(&repo.0).unwrap();
        assert_eq!(first.status.branch.as_deref(), Some("main"));
        assert_eq!((first.added, first.removed), (4, 2));

        let review = review(&repo.0, &Base::Uncommitted).unwrap();
        let paths: Vec<&str> = review.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "new.txt"]);
        let diff = review.files[0].diff.as_ref().unwrap();
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(review.files[1].counts, (2, 0));

        // Take back only the first hunk: line one returns, line ten stays changed.
        revert_hunk(&repo.0, &diff.hunk_patch(0).unwrap()).unwrap();
        let text = std::fs::read_to_string(repo.0.join("a.txt")).unwrap();
        assert!(text.starts_with("one\n") && text.ends_with("TEN\n"));

        assert_eq!(commit_blocker(&repo.0, &review.status), None);
        let committed = commit(&repo.0, "only a", &["a.txt".into()]).unwrap();
        assert!(committed.ends_with("only a"));
        let after = summary(&repo.0).unwrap();
        assert_eq!(
            after.status.entries.len(),
            1,
            "new.txt is still uncommitted"
        );
    }

    #[test]
    fn reverting_a_modified_file_restores_head() {
        let repo = Repo::new("revert");
        repo.write("a.txt", "kept\n");
        repo.git(&["add", "a.txt"]);
        repo.git(&["commit", "--quiet", "-m", "first"]);
        repo.write("a.txt", "changed\n");
        repo.git(&["add", "a.txt"]);
        let entry = Entry {
            path: "a.txt".into(),
            change: Change::Modified,
        };
        revert_file(&repo.0, &entry).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "kept\n"
        );
        assert!(summary(&repo.0).unwrap().status.entries.is_empty());
    }

    #[test]
    fn a_repository_cannot_run_programs_on_a_refresh() {
        let repo = Repo::new("hooks");
        let marker = repo.0.join("ran");
        let script = format!("touch '{}'", marker.display());
        repo.git(&["config", "core.fsmonitor", &script]);
        repo.git(&["config", "diff.external", &script]);
        repo.write("a.txt", "a\n");
        repo.git(&["add", "a.txt"]);
        repo.write("a.txt", "b\n");
        summary(&repo.0).unwrap();
        review(&repo.0, &Base::Uncommitted).unwrap();
        assert!(!marker.exists(), "a repository setting ran a program");
    }
}
