//! The maintainer's release tool.
//!
//! `bareframe-release keygen` makes the Ed25519 key updates are signed with: the private
//! half goes to `~/.config/bareframe-release/update-key`, readable only by you and never
//! into the repository; the public half is printed, for `update::release::PUBLIC_KEY`.
//! `bareframe-release sign FILE` writes `FILE.sig`, which the app checks before updating.
//! `bareframe-release notices OUT` gathers the license texts of every crate built in, and of
//! the bundled icons, into one file for the app bundle.
use ed25519_dalek::{Signer, SigningKey};
use std::{
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
};

fn key_file() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/bareframe-release/update-key"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn keygen() -> Result<(), String> {
    let file = key_file()?;
    if file.exists() {
        return Err(format!(
            "{} already exists; a new key would stop installed apps from updating",
            file.display()
        ));
    }
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut seed))
        .map_err(|error| format!("reading /dev/urandom: {error}"))?;
    let key = SigningKey::from_bytes(&seed);
    let dir = file.parent().ok_or("no folder for the key")?;
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
        .and_then(|mut out| std::io::Write::write_all(&mut out, hex(&seed).as_bytes()))
        .map_err(|error| format!("{}: {error}", file.display()))?;
    println!(
        "private key: {} (back it up; keep it out of git)",
        file.display()
    );
    println!("public key:  {}", hex(key.verifying_key().as_bytes()));
    Ok(())
}

fn sign(path: &str) -> Result<(), String> {
    let file = key_file()?;
    let text = std::fs::read_to_string(&file).map_err(|error| {
        format!(
            "{}: {error} (run `bareframe-release keygen` first)",
            file.display()
        )
    })?;
    let text = text.trim();
    let mut seed = [0u8; 32];
    if text.len() != 64 {
        return Err(format!("{} is not a key", file.display()));
    }
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("{} is not a key", file.display()))?;
    }
    let data = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    let signature = SigningKey::from_bytes(&seed).sign(&data);
    let out = format!("{path}.sig");
    std::fs::write(&out, hex(&signature.to_bytes())).map_err(|error| format!("{out}: {error}"))?;
    println!("{out}");
    Ok(())
}

/// License files a crate ships, by the names crates use for them.
fn license_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_uppercase();
            [
                "LICENSE",
                "LICENCE",
                "COPYING",
                "NOTICE",
                "COPYRIGHT",
                "UNLICENSE",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
                && path.is_file()
        })
        .collect();
    files.sort();
    files
}

fn notices(out: &str) -> Result<(), String> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .args(["--filter-platform", "aarch64-apple-darwin"])
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    // What the app links: its normal dependencies, followed down; build and dev
    // dependencies never ship.
    let nodes: std::collections::HashMap<&str, &serde_json::Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata has no dependency graph")?
        .iter()
        .filter_map(|node| Some((node["id"].as_str()?, node)))
        .collect();
    let root = metadata["resolve"]["root"]
        .as_str()
        .ok_or("no root package")?;
    let mut shipped = std::collections::HashSet::from([root]);
    let mut queue = vec![root];
    while let Some(id) = queue.pop() {
        for dep in nodes
            .get(id)
            .and_then(|node| node["deps"].as_array())
            .into_iter()
            .flatten()
        {
            let normal = dep["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()));
            if let Some(pkg) = dep["pkg"].as_str().filter(|_| normal)
                && shipped.insert(pkg)
            {
                queue.push(pkg);
            }
        }
    }
    let mut packages: Vec<&serde_json::Value> = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?
        .iter()
        .filter(|package| {
            package["id"]
                .as_str()
                .is_some_and(|id| shipped.contains(id))
                && !package["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("bareframe"))
        })
        .collect();
    packages.sort_by_key(|package| (package["name"].to_string(), package["version"].to_string()));
    let mut text = String::from(
        "bareframe is free software under the GNU Affero General Public License v3 or later\n\
         (LICENSE beside this file). It is built with the software below, under the licenses\n\
         that follow each name; where a package has no license file of its own, the standard\n\
         text of its license appears with other packages here.\n",
    );
    let section = |text: &mut String, title: &str, license: &str, files: &[PathBuf]| {
        text.push_str(&format!("\n{}\n{title} — {license}\n", "=".repeat(78)));
        for file in files {
            if let Ok(body) = std::fs::read_to_string(file) {
                text.push_str(&format!(
                    "\n--- {} ---\n\n{}\n",
                    file.file_name().unwrap_or_default().to_string_lossy(),
                    body.trim_end()
                ));
            }
        }
    };
    for package in packages {
        let dir = Path::new(package["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .unwrap_or(Path::new("."));
        let mut files = license_files(dir);
        // Crates in a larger repository often keep the license at its top.
        if files.is_empty() {
            files = dir
                .ancestors()
                .skip(1)
                .take(3)
                .map(license_files)
                .find(|files| !files.is_empty())
                .unwrap_or_default();
        }
        let title = format!(
            "{} {}",
            package["name"].as_str().unwrap_or("?"),
            package["version"].as_str().unwrap_or("?")
        );
        section(
            &mut text,
            &title,
            package["license"]
                .as_str()
                .unwrap_or("see its license file"),
            &files,
        );
        // Not every published crate carries its license file; its authors are the
        // copyright holders, under the standard text of the license named above.
        if files.is_empty()
            && let Some(authors) = package["authors"].as_array().filter(|a| !a.is_empty())
        {
            let authors: Vec<&str> = authors.iter().filter_map(|a| a.as_str()).collect();
            text.push_str(&format!("\nCopyright: {}\n", authors.join(", ")));
        }
    }
    section(
        &mut text,
        "Lucide icons",
        "ISC",
        &[PathBuf::from("assets/icons/LICENSE-lucide")],
    );
    section(
        &mut text,
        "Simple Icons (language logos)",
        "CC0-1.0",
        &[PathBuf::from("assets/icons/lang/README.md")],
    );
    section(
        &mut text,
        "Ghostty shell integration",
        "GPL-3.0",
        &[PathBuf::from("resources/shell-integration/README")],
    );
    std::fs::write(out, text).map_err(|error| format!("{out}: {error}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["keygen"] => keygen(),
        ["sign", path] => sign(path),
        ["notices", out] => notices(out),
        _ => Err("usage: bareframe-release keygen | sign FILE | notices OUT".into()),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bareframe-release: {error}");
            ExitCode::FAILURE
        }
    }
}
