//! Putting a release in place of the running app: downloaded with the system's curl (what
//! this app writes gets no quarantine flag, so the new version opens without Gatekeeper's
//! prompt), checked against the maintainer's signature, unpacked and swapped in, then
//! opened once this process has quit.
use super::release::{self, Release};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const BUNDLE_ID: &str = "dev.bareframe.app";

/// The app bundle this process runs from, when it is a release build installed as an app;
/// a development build never replaces itself.
pub fn running_bundle() -> Option<PathBuf> {
    if cfg!(debug_assertions) || crate::app::is_validation_instance() {
        return None;
    }
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Downloads `release`, checks it and puts it where `bundle` is. Blocks; run it off the UI
/// thread.
pub fn install(release: &Release, bundle: &Path) -> Result<(), String> {
    let work = std::env::temp_dir().join(format!("bareframe-update-{}", release.version));
    // Left over from an attempt that failed; it is ours to clear.
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    let result = install_from(release, bundle, &work);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn install_from(release: &Release, bundle: &Path, work: &Path) -> Result<(), String> {
    let archive = work.join("bareframe.zip");
    let signature = work.join("bareframe.zip.sig");
    download(&release.archive, &archive)?;
    download(&release.signature, &signature)?;
    let data = std::fs::read(&archive).map_err(|error| error.to_string())?;
    let signature = std::fs::read_to_string(&signature).map_err(|error| error.to_string())?;
    if !release::verify(&data, &signature, release::PUBLIC_KEY) {
        return Err("the download is not signed by bareframe's release key".into());
    }
    let unpacked = work.join("unpacked");
    run(
        "/usr/bin/ditto",
        &[
            "-x".as_ref(),
            "-k".as_ref(),
            archive.as_os_str(),
            unpacked.as_os_str(),
        ],
    )?;
    let new = unpacked.join("bareframe.app");
    if info(&new, "CFBundleIdentifier")? != BUNDLE_ID
        || info(&new, "CFBundleShortVersionString")? != release.version
    {
        return Err("the download is not the expected bareframe version".into());
    }
    // Copied beside the old app, then two renames swap them, so no failure leaves no app.
    let staged = bundle.with_file_name(".bareframe-update.app");
    let old = bundle.with_file_name(".bareframe-old.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    run("/usr/bin/ditto", &[new.as_os_str(), staged.as_os_str()])?;
    std::fs::rename(bundle, &old)
        .map_err(|error| format!("could not move {}: {error}", bundle.display()))?;
    if let Err(error) = std::fs::rename(&staged, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(format!("could not put the new version in place: {error}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Opens `bundle` once this process has exited, from a shell that outlives it.
pub fn relaunch_after_exit(bundle: &Path) -> Result<(), String> {
    Command::new("/bin/sh")
        .args([
            "-c",
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#,
            "sh",
            &std::process::id().to_string(),
        ])
        .arg(bundle)
        .spawn()
        .map(drop)
        .map_err(|error| format!("could not schedule the restart: {error}"))
}

/// Fetches `url` (HTTPS only) to `to`.
pub fn download(url: &str, to: &Path) -> Result<(), String> {
    run(
        "/usr/bin/curl",
        &[
            "-fsSL".as_ref(),
            "--proto".as_ref(),
            "=https".as_ref(),
            "--max-time".as_ref(),
            "600".as_ref(),
            "-o".as_ref(),
            to.as_os_str(),
            url.as_ref(),
        ],
    )
}

/// The same, into memory: the release API's small answer.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--proto", "=https", "--max-time", "30"])
        .args(["-H", "Accept: application/vnd.github+json", url])
        .output()
        .map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output.stdout)
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A key from the bundle's Info.plist.
fn info(bundle: &Path, key: &str) -> Result<String, String> {
    let plist = bundle.join("Contents/Info.plist");
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", &format!("Print :{key}")])
        .arg(&plist)
        .output()
        .map_err(|error| format!("PlistBuddy: {error}"))?;
    if !output.status.success() {
        return Err(format!("{} has no {key}", plist.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
