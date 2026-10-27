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

/// The command that installs `server` for the project at `root`, meant to run from the home
/// folder: tools like npm take settings from the directory they run in. rust-analyzer goes
/// into the toolchain the project pins, read from its toolchain file without running it.
pub fn install_command(server: &Server, root: &Path) -> String {
    match pinned_toolchain(root).filter(|_| server.binary == "rust-analyzer") {
        Some(channel) => format!("{} --toolchain {channel}", server.install),
        None => server.install.to_owned(),
    }
}

/// The channel in `rust-toolchain.toml` (or a plain `rust-toolchain`), if it is a plain name.
fn pinned_toolchain(root: &Path) -> Option<String> {
    let channel = if let Ok(text) = std::fs::read_to_string(root.join("rust-toolchain.toml")) {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "channel").then(|| value.trim().trim_matches('"').to_owned())
        })?
    } else {
        std::fs::read_to_string(root.join("rust-toolchain"))
            .ok()?
            .trim()
            .to_owned()
    };
    let plain = !channel.is_empty()
        && channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    plain.then_some(channel)
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
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(*byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?.as_bytes();
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        if encoded[index] == b'%' {
            let hex = std::str::from_utf8(encoded.get(index + 1..index + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            bytes.push(encoded[index]);
            index += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// A byte column in `line` as UTF-16 code units, LSP's default position encoding.
pub fn utf16_column(line: &str, byte: usize) -> u32 {
    line[..byte.min(line.len())]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum()
}

/// A UTF-16 column in `line` as a byte column on a char boundary (clamped to the line).
pub fn byte_column(line: &str, utf16: u32) -> usize {
    let mut units = 0;
    for (byte, c) in line.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += c.len_utf16() as u32;
    }
    line.len()
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
    fn rust_analyzer_installs_into_the_pinned_toolchain() {
        let dir = std::env::temp_dir().join(format!("bareframe-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rust = &SERVERS[0];
        assert_eq!(
            install_command(rust, &dir),
            "rustup component add rust-analyzer"
        );
        std::fs::write(
            dir.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.98.1\"\n",
        )
        .unwrap();
        assert_eq!(
            install_command(rust, &dir),
            "rustup component add rust-analyzer --toolchain 1.98.1"
        );
        // Anything that isn't a plain name stays out of the typed command.
        std::fs::write(
            dir.join("rust-toolchain.toml"),
            "channel = \"x; rm -rf ~\"\n",
        )
        .unwrap();
        assert_eq!(
            install_command(rust, &dir),
            "rustup component add rust-analyzer"
        );
        assert_eq!(install_command(&SERVERS[1], &dir), "npm install -g pyright");
        std::fs::remove_dir_all(&dir).unwrap();
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
    fn columns_convert_between_bytes_and_utf16() {
        let line = "a한😀b";
        // a=1 byte/1 unit, 한=3/1, 😀=4/2.
        assert_eq!(utf16_column(line, 4), 2);
        assert_eq!(utf16_column(line, 8), 4);
        assert_eq!(byte_column(line, 2), 4);
        assert_eq!(byte_column(line, 4), 8);
        assert_eq!(byte_column(line, 99), line.len());
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
