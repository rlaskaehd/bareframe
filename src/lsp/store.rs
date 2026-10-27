//! Language server choices remembered beside the settings file, one `a<TAB>b` pair per
//! line: the servers enabled for each project, and the Python picked for each project.
use super::Key;
use bareframe_core::lsp;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

const ENABLED: &str = "language-servers";
const PYTHONS: &str = "python-interpreters";

/// `binary<TAB>root` per line.
pub(super) fn load_enabled() -> HashSet<Key> {
    load(ENABLED)
        .into_iter()
        .filter_map(|(binary, root)| {
            let server = lsp::SERVERS.iter().find(|server| server.binary == binary)?;
            Some((server.binary, PathBuf::from(root)))
        })
        .collect()
}

pub(super) fn save_enabled(enabled: &HashSet<Key>) {
    save(
        ENABLED,
        enabled
            .iter()
            .map(|(binary, root)| ((*binary).to_owned(), root.display().to_string())),
    );
}

/// `root<TAB>python` per line.
pub(super) fn load_pythons() -> HashMap<PathBuf, PathBuf> {
    load(PYTHONS)
        .into_iter()
        .map(|(root, python)| (root.into(), python.into()))
        .collect()
}

pub(super) fn save_pythons(chosen: &HashMap<PathBuf, PathBuf>) {
    save(
        PYTHONS,
        chosen
            .iter()
            .map(|(root, python)| (root.display().to_string(), python.display().to_string())),
    );
}

fn file(name: &str) -> Option<PathBuf> {
    Some(crate::files::config_file()?.with_file_name(name))
}

fn load(name: &str) -> Vec<(String, String)> {
    let Some(file) = file(name) else {
        return Vec::new();
    };
    let text = crate::files::read_optional(&file)
        .inspect_err(|error| eprintln!("bareframe: {}: {error}", file.display()))
        .ok()
        .flatten()
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(first, second)| (first.to_owned(), second.to_owned()))
        .collect()
}

/// A line break in a folder name would start a line of its own and add a project the user
/// never chose, so a pair holding a control character is kept for this run only.
fn save(name: &str, pairs: impl Iterator<Item = (String, String)>) {
    let Some(file) = file(name) else {
        return;
    };
    let mut lines: Vec<String> = pairs
        .filter(|(first, second)| !first.chars().chain(second.chars()).any(char::is_control))
        .map(|(first, second)| format!("{first}\t{second}\n"))
        .collect();
    lines.sort();
    if let Err(error) = crate::files::write_atomically(&file, &lines.concat()) {
        eprintln!("bareframe: {}: {error}", file.display());
    }
}
