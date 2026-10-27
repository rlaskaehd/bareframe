//! Python interpreters for pyright, chosen as in VS Code: the ones found for a project are
//! listed in the editor's footer, and the one picked is remembered per project. Finding
//! them only reads folders; nothing is run, least of all a project's own `.venv`.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Interpreter {
    /// As the footer shows it: `.venv`, `conda: name`, `pyenv: 3.12.4` or the path.
    pub label: String,
    pub python: PathBuf,
}

/// Folders inside a project that usually hold its virtual environment.
const PROJECT_VENVS: &[&str] = &[".venv", "venv", "env"];
/// Where conda, mamba and micromamba keep their base install, under the home folder.
const CONDA_BASES: &[&str] = &[
    "miniforge3",
    "mambaforge",
    "miniconda3",
    "anaconda3",
    "micromamba",
    ".local/share/mamba",
];

/// Every interpreter worth offering for the project at `root`, the project's own first.
pub fn discover(root: &Path, home: &Path, path: &str) -> Vec<Interpreter> {
    let mut found: Vec<Interpreter> = Vec::new();
    let mut add = |label: String, python: PathBuf| {
        if python.is_file() && !found.iter().any(|known| known.python == python) {
            found.push(Interpreter { label, python });
        }
    };
    for name in PROJECT_VENVS {
        add((*name).to_owned(), root.join(name).join("bin/python"));
    }
    // conda lists every environment it made in one file; the usual folders cover the rest.
    let listed = std::fs::read_to_string(home.join(".conda/environments.txt")).unwrap_or_default();
    let mut conda: Vec<PathBuf> = listed.lines().map(PathBuf::from).collect();
    for base in CONDA_BASES {
        let base = home.join(base);
        conda.push(base.clone());
        conda.extend(subfolders(&base.join("envs")));
    }
    for prefix in conda {
        let name = if prefix
            .parent()
            .is_some_and(|parent| parent.ends_with("envs"))
        {
            crate::explorer::file_name(&prefix)
        } else {
            "base".to_owned()
        };
        add(format!("conda: {name}"), prefix.join("bin/python"));
    }
    for version in subfolders(&home.join(".pyenv/versions")) {
        let label = format!("pyenv: {}", crate::explorer::file_name(&version));
        add(label, version.join("bin/python"));
    }
    for dir in std::env::split_paths(path) {
        let python = dir.join("python3");
        add(python.display().to_string(), python);
    }
    found
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The one used until the user picks: the project's own environment, else the first on PATH.
/// pyright runs the interpreter it is given, so a project's own is taken unasked only when it
/// is a real virtual environment: its `python` links to an interpreter outside the project.
/// A `python` file a repository ships itself is offered in the list but never chosen for
/// the user.
pub fn default<'a>(found: &'a [Interpreter], root: &Path) -> Option<&'a Interpreter> {
    // Both sides resolved: on macOS a temporary or linked folder may be reached two ways.
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let outside = |interpreter: &&Interpreter| {
        std::fs::canonicalize(&interpreter.python).is_ok_and(|real| !real.starts_with(&real_root))
    };
    found
        .iter()
        .find(|interpreter| interpreter.python.starts_with(root) && outside(interpreter))
        .or_else(|| {
            found
                .iter()
                .find(|interpreter| interpreter.label.starts_with('/'))
        })
        .or_else(|| {
            found
                .iter()
                .find(|interpreter| !interpreter.python.starts_with(root))
        })
}

fn chosen_file() -> Option<PathBuf> {
    Some(crate::files::config_file()?.with_file_name("python-interpreters"))
}

/// `root<TAB>python` per line, beside the settings file.
pub fn load_chosen() -> HashMap<PathBuf, PathBuf> {
    let text = chosen_file()
        .and_then(|file| crate::files::read_optional(&file).ok().flatten())
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (root, python) = line.split_once('\t')?;
            Some((PathBuf::from(root), PathBuf::from(python)))
        })
        .collect()
}

pub fn save_chosen(chosen: &HashMap<PathBuf, PathBuf>) {
    let Some(file) = chosen_file() else {
        return;
    };
    let plain = |path: &PathBuf| !path.to_string_lossy().chars().any(char::is_control);
    let mut lines: Vec<String> = chosen
        .iter()
        .filter(|(root, python)| plain(root) && plain(python))
        .map(|(root, python)| format!("{}\t{}\n", root.display(), python.display()))
        .collect();
    lines.sort();
    if let Err(error) = crate::files::write_atomically(&file, &lines.concat()) {
        eprintln!("bareframe: {}: {error}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn python(at: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "").unwrap();
    }

    /// A virtual environment's `python`: a link to an interpreter elsewhere.
    fn venv_python(at: &Path, real: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(real, at).unwrap();
    }

    #[test]
    fn project_venvs_conda_pyenv_and_path_are_found() {
        let dir = std::env::temp_dir().join(format!("bareframe-python-{}", std::process::id()));
        let (root, home, bin) = (dir.join("project"), dir.join("home"), dir.join("bin"));
        python(&home.join("miniforge3/bin/python"));
        venv_python(
            &root.join(".venv/bin/python"),
            &home.join("miniforge3/bin/python"),
        );
        python(&home.join("miniforge3/envs/claude/bin/python"));
        python(&home.join(".pyenv/versions/3.12.4/bin/python"));
        python(&bin.join("python3"));
        let found = discover(&root, &home, &bin.display().to_string());
        let labels: Vec<&str> = found.iter().map(|found| found.label.as_str()).collect();
        let on_path = bin.join("python3").display().to_string();
        assert_eq!(
            labels,
            [
                ".venv",
                "conda: base",
                "conda: claude",
                "pyenv: 3.12.4",
                on_path.as_str()
            ]
        );
        assert_eq!(default(&found, &root).unwrap().label, ".venv");
        // A `python` the repository ships itself is listed but not chosen unasked.
        std::fs::remove_dir_all(root.join(".venv")).unwrap();
        python(&root.join("venv/bin/python"));
        let found = discover(&root, &home, &bin.display().to_string());
        assert_eq!(found[0].label, "venv");
        assert_eq!(default(&found, &root).unwrap().label, on_path);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
