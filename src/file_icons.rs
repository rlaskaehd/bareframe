//! Icons for files by type, as Warp's explorer shows them: the language's logo in its own
//! color. The logos are Simple Icons (CC0); any other file gets the plain file icon.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileIcon {
    pub path: &'static str,
    /// The logo's color; `None` draws it in the surrounding text color.
    pub color: Option<u32>,
}

impl FileIcon {
    pub const FILE: Self = Self::plain("icons/file.svg");
    pub const FOLDER: Self = Self::plain("icons/folder.svg");
    pub const SETTINGS: Self = Self::plain("icons/gear.svg");
    pub const BROWSER: Self = Self::plain("icons/globe.svg");

    const fn plain(path: &'static str) -> Self {
        Self { path, color: None }
    }
}

/// Extensions (lowercase), the logo and its color.
const BY_EXTENSION: &[(&[&str], &str, Option<u32>)] = &[
    (&["rs"], "icons/lang/rust.svg", Some(0xdea584)),
    (&["py", "pyi"], "icons/lang/python.svg", Some(0x4b8bbe)),
    (&["md", "markdown"], "icons/lang/markdown.svg", None),
    (
        &["js", "mjs", "cjs"],
        "icons/lang/javascript.svg",
        Some(0xe8d44d),
    ),
    (
        &["ts", "mts", "cts"],
        "icons/lang/typescript.svg",
        Some(0x3178c6),
    ),
    (&["jsx", "tsx"], "icons/lang/react.svg", Some(0x61dafb)),
    (&["go"], "icons/lang/go.svg", Some(0x00add8)),
    (&["json", "jsonc"], "icons/lang/json.svg", None),
    (&["toml"], "icons/lang/toml.svg", Some(0xc0763a)),
    (&["yaml", "yml"], "icons/lang/yaml.svg", Some(0xcb171e)),
    (&["html", "htm"], "icons/lang/html5.svg", Some(0xe34f26)),
    (&["css"], "icons/lang/css3.svg", Some(0x2965f1)),
    (&["c", "h"], "icons/lang/c.svg", Some(0x7d95b0)),
    (
        &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
        "icons/lang/cplusplus.svg",
        Some(0x5c8dbc),
    ),
    (&["rb"], "icons/lang/ruby.svg", Some(0xcc342d)),
    (&["swift"], "icons/lang/swift.svg", Some(0xf05138)),
    (&["lua"], "icons/lang/lua.svg", Some(0x6b7bd6)),
    (&["sh", "bash", "zsh"], "icons/lang/gnubash.svg", None),
];

/// Whole file names that have a logo of their own.
const BY_NAME: &[(&[&str], &str, Option<u32>)] = &[
    (
        &[
            "Dockerfile",
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yaml",
        ],
        "icons/lang/docker.svg",
        Some(0x2496ed),
    ),
    (
        &[".gitignore", ".gitattributes", ".gitmodules"],
        "icons/lang/git.svg",
        Some(0xf05032),
    ),
    (
        &[".zshrc", ".bashrc", ".zshenv", ".profile"],
        "icons/lang/gnubash.svg",
        None,
    ),
];

pub fn for_path(path: &Path) -> FileIcon {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    BY_NAME
        .iter()
        .find(|(names, ..)| names.contains(&name))
        .or_else(|| {
            let extension = extension.as_deref()?;
            BY_EXTENSION
                .iter()
                .find(|(extensions, ..)| extensions.contains(&extension))
        })
        .map_or(FileIcon::FILE, |(_, path, color)| FileIcon {
            path,
            color: *color,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_get_their_language_logo() {
        assert_eq!(
            for_path(Path::new("src/main.rs")).path,
            "icons/lang/rust.svg"
        );
        assert_eq!(for_path(Path::new("A.PY")).path, "icons/lang/python.svg");
        assert_eq!(
            for_path(Path::new("x/Dockerfile")).path,
            "icons/lang/docker.svg"
        );
        assert_eq!(for_path(Path::new(".gitignore")).path, "icons/lang/git.svg");
        assert_eq!(for_path(Path::new("LICENSE")), FileIcon::FILE);
        assert_eq!(for_path(Path::new("notes.txt")), FileIcon::FILE);
    }

    #[test]
    fn every_icon_in_the_tables_is_bundled() {
        use gpui::AssetSource;
        let bundled = |path: &str| matches!(crate::assets::Assets.load(path), Ok(Some(_)));
        for (_, path, _) in BY_EXTENSION.iter().chain(BY_NAME) {
            assert!(bundled(path), "{path}");
        }
        for icon in [FileIcon::FILE, FileIcon::FOLDER, FileIcon::SETTINGS] {
            assert!(bundled(icon.path), "{}", icon.path);
        }
    }
}
