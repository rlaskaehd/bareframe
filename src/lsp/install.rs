//! Finding what runs on this machine: each server's globally installed binary, the login
//! shell's PATH it is found on, the Python interpreters for a project, and looking again
//! while a server is being installed.
use super::{
    Key, LanguageServers, LspEvent, key_for,
    python::{self, Interpreter},
};
use bareframe_core::lsp::Server;
use gpui::Context;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// The interpreters for the project at `root` (see `python::discover`).
pub(super) fn find_interpreters(root: &Path) -> Vec<Interpreter> {
    match (crate::files::home_dir(), login_path()) {
        (Some(home), Some(path)) => python::discover(root, &home, path),
        _ => Vec::new(),
    }
}

pub(super) enum Installed {
    Checking,
    Found(PathBuf),
    Missing,
}

impl LanguageServers {
    /// The footer's Install runs the server's install command in a terminal; meanwhile the
    /// server is looked for every few seconds, for up to ten minutes.
    pub fn watch_install(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some((_, server, _)) = key_for(path) else {
            return;
        };
        self._recheck = Some(cx.spawn(async move |servers, cx| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(600) {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let found = servers.update(cx, |servers, cx| {
                    let installed = servers.installed.get(server.binary);
                    let found = matches!(installed, Some(Installed::Found(_)));
                    if !found && !matches!(installed, Some(Installed::Checking)) {
                        servers.installed.remove(server.binary);
                        servers.check_installed(server, cx);
                    }
                    found
                });
                if !matches!(found, Ok(false)) {
                    break;
                }
            }
        }));
    }

    /// Looks for the server on the login shell's PATH once, off the UI thread.
    pub(super) fn check_installed(&mut self, server: &'static Server, cx: &mut Context<Self>) {
        if self.installed.contains_key(server.binary) {
            return;
        }
        self.installed.insert(server.binary, Installed::Checking);
        cx.spawn(async move |servers, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { find_server(server) })
                .await;
            servers
                .update(cx, |servers, cx| {
                    servers.installed.insert(
                        server.binary,
                        found.map_or(Installed::Missing, Installed::Found),
                    );
                    let keys: Vec<Key> = servers
                        .documents
                        .values()
                        .map(|document| document.key.clone())
                        .filter(|key| key.0 == server.binary && servers.enabled.contains(key))
                        .collect();
                    for key in keys {
                        servers.start(server, key, cx);
                    }
                    cx.emit(LspEvent::Updated);
                })
                .ok();
        })
        .detach();
    }
}

/// The globally installed server: found on the login shell's PATH and tried from the home
/// folder. rustup's proxy in `~/.cargo/bin` is swapped for the binary of the default
/// toolchain, which `rustup which` names: run as the proxy inside a project, the server
/// would follow that project's `rust-toolchain.toml`, which may lack it or, in an untrusted
/// repository, name a program of its own.
fn find_server(server: &Server) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = login_path()?;
    let home = crate::files::home_dir()?;
    let run = |program: &Path, args: &[&str]| {
        Command::new(program)
            .args(args)
            .current_dir(&home)
            .env("PATH", path)
            // Set when bareframe itself was started through cargo; it would pin the
            // toolchain to bareframe's own.
            .env_remove("RUSTUP_TOOLCHAIN")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
    };
    let mut binary = std::env::split_paths(path)
        .map(|dir| dir.join(server.binary))
        .find(|file| {
            file.metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })?;
    let rustup = binary.with_file_name("rustup");
    if rustup.is_file() {
        let output = run(&rustup, &["which", server.binary])?;
        binary = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    }
    match server.probe {
        Some(probe) => run(&binary, probe).map(|_| binary),
        None => Some(binary),
    }
}

/// PATH as the user's login shell sets it, so servers in `~/.cargo/bin` or npm's global
/// folder are found though the app was started from the Dock. Asked once; a shell that
/// doesn't answer within five seconds leaves the app's own PATH.
pub(super) fn login_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        const MARK: &str = "__bareframe_path__";
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut command = Command::new(shell);
        // From the home folder, not wherever bareframe was started (a project, with
        // `--cwd`): rc files that activate per-folder tools (mise, direnv, nvm) would
        // otherwise act on that project's files before its server is enabled.
        if let Some(home) = crate::files::home_dir() {
            command.current_dir(home);
        }
        let shell_path = command
            .args([
                "-l",
                "-i",
                "-c",
                &format!("printf '\\n{MARK}%s\\n' \"$PATH\""),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .and_then(|mut child| {
                let started = Instant::now();
                while matches!(child.try_wait(), Ok(None)) {
                    if started.elapsed() > Duration::from_secs(5) {
                        let _ = child.kill();
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let output = child.wait_with_output().ok()?;
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find_map(|line| line.strip_prefix(MARK).map(str::to_owned))
            });
        shell_path.or_else(|| std::env::var("PATH").ok())
    })
    .as_deref()
}
