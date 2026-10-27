//! Language servers the editor can use, as Warp offers them: one server per language,
//! picked by the file's extension, and the plumbing that doesn't need a process: message
//! framing, `file://` URIs and LSP's UTF-16 columns.
use std::{
    io::{self, BufRead},
    path::{Path, PathBuf},
};

pub struct Server {
    /// The executable, looked up on the login shell's PATH.
    pub binary: &'static str,
    pub args: &'static [&'static str],
    /// Arguments that make an installed server exit 0. `None`: being on PATH is enough.
    /// rustup leaves a `rust-analyzer` proxy that fails until the component is added.
    pub probe: Option<&'static [&'static str]>,
    /// Run in a terminal by the editor's Install button.
    pub install: &'static str,
    /// File extensions and their LSP language ids.
    pub languages: &'static [(&'static str, &'static str)],
}

pub const SERVERS: &[Server] = &[
    Server {
        binary: "rust-analyzer",
        args: &[],
        probe: Some(&["--version"]),
        install: "rustup component add rust-analyzer",
        languages: &[("rs", "rust")],
    },
    Server {
        binary: "pyright-langserver",
        args: &["--stdio"],
        probe: None,
        install: "npm install -g pyright",
        languages: &[("py", "python"), ("pyi", "python")],
    },
    Server {
        binary: "typescript-language-server",
        args: &["--stdio"],
        probe: Some(&["--version"]),
        install: "npm install -g typescript-language-server typescript",
        languages: &[
            ("ts", "typescript"),
            ("mts", "typescript"),
            ("cts", "typescript"),
            ("tsx", "typescriptreact"),
            ("js", "javascript"),
            ("mjs", "javascript"),
            ("cjs", "javascript"),
            ("jsx", "javascriptreact"),
        ],
    },
    Server {
        binary: "gopls",
        args: &[],
        probe: Some(&["version"]),
        install: "go install golang.org/x/tools/gopls@latest",
        languages: &[("go", "go")],
    },
    Server {
        binary: "clangd",
        args: &[],
        probe: Some(&["--version"]),
        install: "xcode-select --install",
        languages: &[
            ("c", "c"),
            ("h", "cpp"),
            ("cc", "cpp"),
            ("cpp", "cpp"),
            ("cxx", "cpp"),
            ("hh", "cpp"),
            ("hpp", "cpp"),
            ("hxx", "cpp"),
        ],
    },
];

/// The server for `path` and the file's language id.
pub fn server_for(path: &Path) -> Option<(&'static Server, &'static str)> {
    let extension = path.extension()?.to_str()?;
    SERVERS.iter().find_map(|server| {
        server
            .languages
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(extension))
            .map(|(_, language)| (server, *language))
    })
}

/// The project a file belongs to, as Warp picks it: the enclosing git repository, else the
/// file's own directory.
pub fn project_root(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(file);
    dir.ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(dir)
        .to_path_buf()
}

/// One message in LSP's base protocol: a `Content-Length` header, a blank line, the body.
pub fn frame(body: &str) -> Vec<u8> {
    let mut message = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    message.extend_from_slice(body.as_bytes());
    message
}

/// Reads the next message body; `None` once the stream ends.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut length = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| io::Error::other("message without Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| io::Error::other("message is not UTF-8"))
}

/// The `file://` URI of an absolute path, with everything but unreserved characters and
/// `/` percent-encoded.
pub fn path_to_uri(path: &Path) -> String {
    format!(
        "file://{}",
        crate::state::escape(path.as_os_str().as_encoded_bytes())
    )
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let bytes = crate::state::unescape(uri.strip_prefix("file://")?)?;
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_follow_the_extension() {
        let (server, language) = server_for(Path::new("/a/main.rs")).unwrap();
        assert_eq!((server.binary, language), ("rust-analyzer", "rust"));
        assert_eq!(server_for(Path::new("x.TSX")).unwrap().1, "typescriptreact");
        assert_eq!(server_for(Path::new("x.h")).unwrap().0.binary, "clangd");
        assert!(server_for(Path::new("notes.md")).is_none());
        assert!(server_for(Path::new("Makefile")).is_none());
    }

    #[test]
    fn messages_are_framed_and_read_back() {
        let mut stream = frame(r#"{"id":1}"#);
        stream.extend(b"content-length: 2\r\nContent-Type: x\r\n\r\n{}");
        let mut reader = io::Cursor::new(stream);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), r#"{"id":1}"#);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), "{}");
        assert!(read_message(&mut reader).unwrap().is_none());
        assert!(read_message(&mut io::Cursor::new(b"X: 1\r\n\r\n".to_vec())).is_err());
    }

    #[test]
    fn uris_round_trip() {
        let path = Path::new("/Users/me/프로젝트 a/main#1.rs");
        let uri = path_to_uri(path);
        assert_eq!(
            uri,
            "file:///Users/me/%ED%94%84%EB%A1%9C%EC%A0%9D%ED%8A%B8%20a/main%231.rs"
        );
        assert_eq!(uri_to_path(&uri).unwrap(), path);
        assert!(uri_to_path("https://x").is_none());
    }

    #[test]
    fn the_project_is_the_git_repository() {
        let dir = std::env::temp_dir().join(format!("bareframe-lsp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        std::fs::create_dir_all(dir.join("repo/src")).unwrap();
        assert_eq!(
            project_root(&dir.join("repo/src/main.rs")),
            dir.join("repo")
        );
        assert_eq!(project_root(&dir.join("loose.rs")), dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
