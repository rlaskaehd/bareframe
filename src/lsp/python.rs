//! Python interpreters for pyright, chosen as in VS Code: the ones found for a project are
//! listed in the editor's footer, and the one picked is remembered per project (`store`).
//! Finding them only reads folders; nothing is run, least of all a project's own `.venv`.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Interpreter {
    /// As the list shows it: `.venv`, `conda: name`, `pyenv: 3.12.4` or the path.
    pub label: String,
    /// The environment's short name for the status line: `.venv`, `name`, `python3`.
    pub name: String,
    /// Read from the environment's files, never by running it.
    pub version: Option<String>,
    pub python: PathBuf,
}

impl Interpreter {
    /// `spark (3.14.7)`, as VS Code's status bar shows it.
    pub fn chip(&self) -> String {
        match &self.version {
            Some(version) if version != &self.name => format!("{} ({version})", self.name),
            _ => self.name.clone(),
        }
    }
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
    let mut add = |label: String, name: String, version: Option<String>, python: PathBuf| {
        if python.is_file() && !found.iter().any(|known| known.python == python) {
            found.push(Interpreter {
                label,
                name,
                version,
                python,
            });
        }
    };
    for name in PROJECT_VENVS {
        let venv = root.join(name);
        let version = venv_version(&venv);
        add(
            (*name).into(),
            (*name).into(),
            version,
            venv.join("bin/python"),
        );
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
        let version = conda_version(&prefix);
        add(
            format!("conda: {name}"),
            name,
            version,
            prefix.join("bin/python"),
        );
    }
    for dir in subfolders(&home.join(".pyenv/versions")) {
        let version = crate::explorer::file_name(&dir);
        let label = format!("pyenv: {version}");
        add(
            label,
            version.clone(),
            Some(version),
            dir.join("bin/python"),
        );
    }
    for dir in std::env::split_paths(path) {
        let python = dir.join("python3");
        // `python3` links to `python3.12` and the like.
        let version = std::fs::canonicalize(&python).ok().and_then(|real| {
            let name = real
                .file_name()?
                .to_str()?
                .strip_prefix("python")?
                .to_owned();
            (!name.is_empty() && name != "3").then_some(name)
        });
        add(
            python.display().to_string(),
            "python3".into(),
            version,
            python,
        );
    }
    found
}

/// A virtual environment's version, from the `pyvenv.cfg` it was made with.
fn venv_version(venv: &Path) -> Option<String> {
    let text = std::fs::read_to_string(venv.join("pyvenv.cfg")).ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        let version = matches!(key.trim(), "version" | "version_info").then(|| {
            value
                .trim()
                .split('.')
                .take(3)
                .collect::<Vec<_>>()
                .join(".")
        })?;
        Some(version)
    })
}

/// A conda environment's version, from its `conda-meta/python-3.12.4-<build>.json`.
fn conda_version(prefix: &Path) -> Option<String> {
    std::fs::read_dir(prefix.join("conda-meta"))
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let version = name.strip_prefix("python-")?.split('-').next()?;
            version
                .starts_with(|c: char| c.is_ascii_digit())
                .then(|| version.to_owned())
        })
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

/// The one used until the user picks: the first on PATH, else any outside the project.
/// pyright runs the interpreter it is given, and Python runs the `.pth` files of its
/// environment's packages as it starts, so an environment inside the project, which the
/// repository may have shipped, is only used once the user picks it in the footer.
pub fn default<'a>(found: &'a [Interpreter], root: &Path) -> Option<&'a Interpreter> {
    let outside: Vec<&Interpreter> = found
        .iter()
        .filter(|interpreter| !interpreter.python.starts_with(root))
        .collect();
    outside
        .iter()
        .find(|interpreter| interpreter.label.starts_with('/'))
        .or(outside.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn python(at: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "").unwrap();
    }

    #[test]
    fn project_venvs_conda_pyenv_and_path_are_found() {
        let dir = std::env::temp_dir().join(format!("bareframe-python-{}", std::process::id()));
        let (root, home, bin) = (dir.join("project"), dir.join("home"), dir.join("bin"));
        python(&root.join(".venv/bin/python"));
        std::fs::write(
            root.join(".venv/pyvenv.cfg"),
            "home = /x\nversion_info = 3.14.7.final.0\n",
        )
        .unwrap();
        python(&home.join("miniforge3/bin/python"));
        python(&home.join("miniforge3/conda-meta/python-3.12.4-h1_0_cpython.json"));
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
        let chips: Vec<String> = found.iter().map(Interpreter::chip).collect();
        assert_eq!(
            &chips[..4],
            [".venv (3.14.7)", "base (3.12.4)", "claude", "3.12.4"]
        );
        // The project's own environment is listed first but never chosen unasked.
        assert_eq!(default(&found, &root).unwrap().label, on_path);
        std::fs::remove_dir_all(&bin).unwrap();
        let found = discover(&root, &home, &bin.display().to_string());
        assert_eq!(default(&found, &root).unwrap().label, "conda: base");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
