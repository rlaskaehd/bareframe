//! The files a document involves: the document itself, read within bounds, where its links
//! lead, and which pictures are safe to show.
use std::path::{Path, PathBuf};

/// Larger files are not rendered; reading and laying them out would stall the window.
const MAX_FILE: u64 = 8 << 20;
const MAX_IMAGE: u64 = 20 << 20;
/// Pixels a picture may decode to; a small file can claim a huge canvas.
const MAX_PIXELS: u64 = 50_000_000;

/// Where a link points, once resolved against the file it is in.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Web(String),
    /// Another Markdown file, opened in the viewer.
    Markdown(PathBuf),
    /// Any other local file, opened as the explorer would.
    File(PathBuf),
    /// A heading in this document (`#slug`), scrolled to.
    Anchor(String),
}

/// `None` for links bareframe won't follow: schemes other than web pages and mail, which
/// could start other programs.
pub fn resolve(link: &str, base: &Path) -> Option<Target> {
    let lower = link.to_ascii_lowercase();
    if ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(Target::Web(link.to_owned()));
    }
    if let Some(anchor) = link.strip_prefix('#') {
        return Some(Target::Anchor(percent_decode(anchor)));
    }
    if link.contains("://") || lower.starts_with("javascript:") {
        return None;
    }
    let path = link.split('#').next().unwrap_or(link);
    let path = base.join(percent_decode(path));
    if !path.exists() {
        return None;
    }
    Some(if is_markdown(&path) {
        Target::Markdown(path)
    } else {
        Target::File(path)
    })
}

/// A heading's anchor as GitHub makes it: lowercased, spaces to `-`, other punctuation
/// dropped; letters of any script stay. Repeats get `-1`, `-2`… from the caller.
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "mdown", "mkd"].contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// `docs/a%20b.md` → `docs/a b.md`; anything malformed stays as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Reads a regular file of at most `MAX_FILE` bytes; a pipe or device under a `.md` name
/// would otherwise block or never end. The cap holds even if the file grows meanwhile.
pub(super) fn read_document(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err("not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("larger than {} MB", MAX_FILE >> 20));
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
}

/// A picture file of a sensible size: a link could name a device, a huge file, or a small
/// file that decodes to a huge canvas (only the header is read to check).
pub(super) fn is_image(path: &Path) -> bool {
    let picture = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            // ponytail: animated GIF and WebP are left out, since frames multiply the
            // decoded size past what the header shows; count frames if they are wanted.
            ["png", "jpg", "jpeg", "bmp"].contains(&extension.to_ascii_lowercase().as_str())
        });
    picture
        && std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_IMAGE)
        && image::image_dimensions(path)
            .is_ok_and(|(width, height)| u64::from(width) * u64::from(height) <= MAX_PIXELS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_and_images_are_bounded() {
        let dir = std::env::temp_dir().join(format!("bareframe-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.md");
        std::fs::write(&big, vec![b'a'; MAX_FILE as usize + 1]).unwrap();
        assert!(read_document(&big).is_err());
        assert!(
            read_document(&dir).is_err(),
            "a directory is not a document"
        );
        // A PNG header claiming 100000 × 100000 pixels, in a file of a few bytes.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let bomb = dir.join("bomb.png");
        std::fs::write(&bomb, png).unwrap();
        assert!(!is_image(&bomb));
        assert!(!is_image(&dir.join("missing.png")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_resolve_only_to_safe_targets() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            resolve("https://example.com", base),
            Some(Target::Web("https://example.com".into()))
        );
        assert_eq!(
            resolve("docs/usage.md#설정", base),
            Some(Target::Markdown(base.join("docs/usage.md")))
        );
        assert_eq!(
            resolve("Cargo.toml", base),
            Some(Target::File(base.join("Cargo.toml")))
        );
        assert_eq!(
            resolve("#%EC%84%A4%EC%A0%95", base),
            Some(Target::Anchor("설정".into()))
        );
        assert_eq!(resolve("file:///etc/passwd", base), None);
        assert_eq!(resolve("javascript:alert(1)", base), None);
        assert_eq!(resolve("missing.md", base), None);
        assert_eq!(percent_decode("a%20b%zz"), "a b%zz");
        assert_eq!(slug("1. 전체 다이어그램"), "1-전체-다이어그램");
        assert_eq!(slug("What's `new` (v2)?"), "whats-new-v2");
    }
}
