//! Handing files to macOS: plain documents to their apps, anything else to Finder.
use std::path::Path;

/// Opens a file outside bareframe until it has its own viewers. Only plain documents are
/// opened, by their resolved path so a symlink can't disguise a script or app: text and
/// source in the default text editor (a `.py` default handler may run it), images and
/// PDFs in their viewer. Everything else is revealed in Finder, so a click never runs code.
pub(super) fn open_externally(path: &Path) {
    let target = std::fs::canonicalize(path).ok();
    let mut open = std::process::Command::new("/usr/bin/open");
    match target.as_deref().map(document_kind) {
        Some(Some(Document::Text)) => open.arg("-t"),
        Some(Some(Document::Media)) => &mut open,
        _ => return reveal_in_finder(path),
    };
    let target = target.expect("a document kind implies a resolved path");
    if let Err(error) = open.arg(&target).spawn() {
        eprintln!("bareframe: opening {}: {error}", target.display());
    }
}

#[derive(Debug, PartialEq)]
enum Document {
    Text,
    Media,
}

/// A regular, non-executable file with a known document extension.
fn document_kind(path: &Path) -> Option<Document> {
    use std::os::unix::fs::PermissionsExt;
    const TEXT: [&str; 31] = [
        "md", "markdown", "txt", "log", "csv", "tsv", "json", "jsonl", "toml", "yaml", "yml",
        "xml", "ini", "conf", "rs", "c", "h", "cc", "cpp", "hpp", "m", "swift", "zig", "go", "py",
        "js", "ts", "tsx", "css", "lock", "patch",
    ];
    const MEDIA: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "pdf"];
    let plain = std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 == 0);
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    if !plain {
        None
    } else if TEXT.contains(&extension.as_str()) {
        Some(Document::Text)
    } else if MEDIA.contains(&extension.as_str()) {
        Some(Document::Media)
    } else {
        None
    }
}

pub(super) fn reveal_in_finder(path: &Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        eprintln!("bareframe: revealing {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, document_kind};

    #[test]
    fn only_plain_documents_are_opened() {
        let dir = std::env::temp_dir().join(format!("bareframe-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("notes.md");
        std::fs::write(&note, "").unwrap();
        let script = dir.join("run.py");
        std::fs::write(&script, "").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let command = dir.join("deploy.command");
        std::fs::write(&command, "").unwrap();
        // A link named like a document resolves to what it points at.
        let disguised = dir.join("readme.md");
        let _ = std::fs::remove_file(&disguised);
        std::os::unix::fs::symlink(&command, &disguised).unwrap();
        assert_eq!(document_kind(&note), Some(Document::Text));
        assert_eq!(document_kind(&script), None, "executable bit");
        assert_eq!(document_kind(&command), None, "not on the list");
        assert_eq!(
            document_kind(&std::fs::canonicalize(&disguised).unwrap()),
            None
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
