//! What GitHub says the latest release is, whether it is newer than this build, and whether
//! a downloaded archive carries the maintainer's signature.
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;

/// The repository whose GitHub Releases the app updates from.
pub const REPOSITORY: &str = "rlaskaehd/bareframe";

/// The maintainer's Ed25519 public key; `bareframe-release keygen` printed it and keeps the
/// private half outside the repository.
pub const PUBLIC_KEY: &str = "540dc1e9e8dab751761bca8ae601ccac1541d08c1c42dc4c8573db5449e5267d";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub archive: String,
    pub signature: String,
}

/// The archive name a release carries for this Mac.
pub fn archive_name(version: &str) -> String {
    format!("bareframe-{version}-macos-arm64.zip")
}

/// The latest release from the API's answer, with its archive and signature to download.
pub fn parse_latest(reply: &Value) -> Option<Release> {
    let tag = reply["tag_name"].as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag).to_owned();
    let url = |name: &str| {
        reply["assets"]
            .as_array()?
            .iter()
            .find(|asset| asset["name"] == name)?["browser_download_url"]
            .as_str()
            .map(str::to_owned)
    };
    let archive = archive_name(&version);
    Some(Release {
        archive: url(&archive)?,
        signature: url(&format!("{archive}.sig"))?,
        version,
    })
}

/// `1.2.3` and `1.2.3-alpha.1` as comparable keys: a pre-release sorts before its release.
fn key(version: &str) -> Option<([u64; 3], bool)> {
    let (base, pre) = match version.split_once('-') {
        Some((base, _)) => (base, true),
        None => (version, false),
    };
    let mut parts = base.split('.').map(|part| part.parse::<u64>().ok());
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    Some((numbers, !pre))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((key(candidate), key(current)), (Some(a), Some(b)) if a > b)
}

/// Whether `signature` (hex, as `bareframe-release sign` writes it) signs `data` with `key`.
pub fn verify(data: &[u8], signature: &str, key: &str) -> bool {
    let (Some(signature), Some(key)) = (unhex::<64>(signature.trim()), unhex::<32>(key)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    key.verify_strict(data, &Signature::from_bytes(&signature))
        .is_ok()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_latest_release_names_its_archive_and_signature() {
        let reply = json!({"tag_name": "v0.2.0", "assets": [
            {"name": "bareframe-0.2.0-macos-arm64.zip", "browser_download_url": "https://x/a.zip"},
            {"name": "bareframe-0.2.0-macos-arm64.zip.sig", "browser_download_url": "https://x/a.sig"},
        ]});
        let release = parse_latest(&reply).unwrap();
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.archive, "https://x/a.zip");
        assert_eq!(release.signature, "https://x/a.sig");
        assert!(parse_latest(&json!({"tag_name": "v0.2.0", "assets": []})).is_none());
    }

    #[test]
    fn versions_compare_by_number_and_pre_release() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("0.2.0", "0.2.0-alpha.1"));
        assert!(!is_newer("0.2.0-alpha.1", "0.2.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }

    #[test]
    fn only_the_maintainers_signature_passes() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = hex(key.verifying_key().as_bytes());
        let signature = hex(&key.sign(b"archive").to_bytes());
        assert!(verify(b"archive", &signature, &public));
        assert!(!verify(b"tampered", &signature, &public));
        let other = hex(SigningKey::from_bytes(&[8; 32]).verifying_key().as_bytes());
        assert!(!verify(b"archive", &signature, &other));
        assert!(!verify(b"archive", "zz", &public));
    }
}
