//! bareframe's own files: the settings file, the saved layout and the zsh startup files,
//! and reading and writing them safely.
use crate::app::is_validation_instance;
use bareframe_core::{
    settings::{Settings, config_path, state_path},
    state::{self, SavedState},
};
use std::path::{Path, PathBuf};

/// bareframe's zsh startup directory and the vendored Ghostty integration it sources: in
/// the app bundle, else in the source tree.
pub fn zsh_integration() -> Option<(PathBuf, PathBuf)> {
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        let resources = exe.parent()?.join("../Resources/shell-integration");
        Some((resources.join("bareframe"), resources.join("zsh")))
    });
    let source = (
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/shell-integration/bareframe"
        )),
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/gpui-libghostty/crates/gpui-ghostty/vendor/ghostty/src/shell-integration/zsh"
        )),
    );
    bundled
        .into_iter()
        .chain([source])
        .find(|(own, ghostty)| own.join(".zshenv").is_file() && ghostty.join(".zshenv").is_file())
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A file beside the settings; the validation instance keeps its own under `target/` so
/// results are reproducible.
fn app_file(name: &str, path: fn(Option<&Path>, &Path) -> PathBuf) -> Option<PathBuf> {
    if is_validation_instance() {
        return Some(
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target/validation")).join(name),
        );
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    Some(path(xdg.as_deref().map(Path::new), &home_dir()?))
}

pub fn config_file() -> Option<PathBuf> {
    app_file("config", config_path)
}

/// Where the window layout is saved.
pub fn state_file() -> Option<PathBuf> {
    app_file("state", state_path)
}

/// The file's text, or `None` if it does not exist.
pub fn read_optional(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes through a temporary file, so a crash mid-write leaves the previous file intact.
/// A symbolic link (a settings file kept in a dotfiles repository) is followed, so the
/// rename replaces the file it points to rather than the link.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let temporary = path.with_extension("tmp");
    path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// The file's text and the settings it holds; a missing file gives the defaults.
pub fn load_settings() -> (Settings, String) {
    let Some(path) = config_file() else {
        return (Settings::default(), String::new());
    };
    let text = read_optional(&path)
        .unwrap_or_else(|error| {
            eprintln!("bareframe: {}: {error}", path.display());
            None
        })
        .unwrap_or_default();
    let (settings, warnings) = Settings::parse(&text);
    for warning in warnings {
        eprintln!("bareframe: {}: {warning}", path.display());
    }
    (settings, text)
}

/// Rewrites one key in the settings file, keeping the user's comments and other lines.
/// Returns the new text so the file watcher can tell this write from an outside edit.
pub fn save_setting(key: &str, value: &str, default: &str) -> Result<String, String> {
    edit_config(|text| bareframe_core::settings::write_value(text, key, value, default))
}

/// Reads the settings file (empty if missing), rewrites it with `change` and writes it back
/// through a temporary file. Returns the new text.
pub fn edit_config(change: impl FnOnce(&str) -> String) -> Result<String, String> {
    let path = config_file().ok_or("no home directory")?;
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let text = change(&read_optional(&path).map_err(failed)?.unwrap_or_default());
    write_atomically(&path, &text).map_err(failed)?;
    Ok(text)
}

/// A missing file is a first launch; a broken one is reported and left for inspection.
pub fn load_state() -> Option<SavedState> {
    let path = state_file()?;
    let text = read_optional(&path)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", path.display()))
        .ok()??;
    state::decode(&text)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", path.display()))
        .ok()
}
