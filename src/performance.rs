//! GitHub-discovered performance packages. This module never runs package executables,
//! installs game files, injects DLLs, or changes rendering/input settings.
//!
//! Native fingerprints originate from the MIT-licensed PaladinsCat-mods release
//! launcher; opaque DLL redistribution terms remain in BINARY-LICENSE.txt.
use crate::engine::{self, Progress, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub use crate::sources::{
    PERFORMANCE_RELEASE_URL as RELEASE_URL, PERFORMANCE_REPOSITORY as REPOSITORY,
};
const MANIFEST: &str = "manifest.json";
const SUMS: &str = "SHA256SUMS.txt";
const LIMIT: u64 = 1024 * 1024;
const JSON_LIMIT: u64 = 16 * 1024;
const RECEIPT: &str = "github-release.json";
const METADATA_LIMIT: u64 = 256 * 1024;
pub const PAYLOADS: [&str; 3] = [
    "watchcat-native-input-compact-modifier.dll",
    "watchcat-world-scheduler.dll",
    "watchcat-native-input-raw-read-original.dll",
];
const ASSET_NAMES: [&str; 8] = [
    PAYLOADS[0],
    PAYLOADS[1],
    PAYLOADS[2],
    "paladinscat-mods.exe",
    "README.md",
    "RELEASE_NOTES.md",
    "LICENSE",
    "BINARY-LICENSE.txt",
];
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub schema: u32,
    pub version: String,
    #[serde(rename = "mod")]
    pub mod_id: String,
    pub platform: String,
    pub source_revision: String,
    pub game_sha256: String,
    pub dinput8_sha256: String,
    pub user32_sha256: String,
    pub performance_verified: bool,
    pub compatibility_verified: bool,
    pub rendering: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cached {
    pub package: Package,
    pub directory: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub manifest: Asset,
    pub archive: Asset,
    pub checksums: Asset,
}
fn stable_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}
impl Release {
    pub fn page(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/tag/v{}",
            self.version
        )
    }
    fn validate(&self) -> Result<()> {
        if !stable_version(&self.version) {
            return Err("Invalid stable performance release version".into());
        }
        for (asset, name, limit) in [
            (&self.manifest, MANIFEST.to_string(), JSON_LIMIT),
            (
                &self.archive,
                format!("PaladinsCat-performance-v{}-windows-x64.zip", self.version),
                LIMIT,
            ),
            (&self.checksums, SUMS.to_string(), JSON_LIMIT),
        ] {
            if asset.name != name || !hash(&asset.sha256) || asset.bytes == 0 || asset.bytes > limit
            {
                return Err("Invalid performance release asset identity, digest or size".into());
            }
        }
        Ok(())
    }
    fn package(&self, bytes: &[u8]) -> Result<Package> {
        self.validate()?;
        if bytes.len() as u64 != self.manifest.bytes || digest(bytes) != self.manifest.sha256 {
            return Err("Performance manifest differs from GitHub's digest or size".into());
        }
        let package: Package = serde_json::from_slice(bytes).map_err(error)?;
        validate_contents(&package)?;
        if package.version != self.version
            || package.source_revision.len() != 40
            || !package
                .source_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err("Performance manifest release/source identity mismatch".into());
        }
        Ok(package)
    }
}
pub fn parse_release(bytes: &[u8]) -> Result<Release> {
    #[derive(Deserialize)]
    struct GithubRelease {
        tag_name: String,
        draft: bool,
        prerelease: bool,
        html_url: String,
        assets: Vec<GithubAsset>,
    }
    #[derive(Deserialize)]
    struct GithubAsset {
        name: String,
        browser_download_url: String,
        size: u64,
        digest: Option<String>,
        state: String,
    }
    if bytes.len() as u64 > METADATA_LIMIT {
        return Err("Performance release metadata exceeds its limit".into());
    }
    let github: GithubRelease = serde_json::from_slice(bytes).map_err(error)?;
    let version = github
        .tag_name
        .strip_prefix('v')
        .ok_or("Performance tag must start with v")?;
    if github.draft || github.prerelease || !stable_version(version) {
        return Err("Only published stable performance releases are supported".into());
    }
    let asset = |name: &str| -> Result<Asset> {
        let matches = github
            .assets
            .iter()
            .filter(|a| a.name == name)
            .collect::<Vec<_>>();
        let [value] = matches.as_slice() else {
            return Err("Missing or duplicate performance release asset".into());
        };
        if value.state != "uploaded"
            || value.browser_download_url != crate::sources::performance_asset_url(version, name)
        {
            return Err("Performance asset has an unexpected source or upload state".into());
        }
        Ok(Asset {
            name: name.into(),
            bytes: value.size,
            sha256: value
                .digest
                .as_deref()
                .and_then(|d| d.strip_prefix("sha256:"))
                .ok_or("GitHub did not supply a performance asset SHA-256")?
                .to_ascii_lowercase(),
        })
    };
    let release = Release {
        version: version.into(),
        manifest: asset(MANIFEST)?,
        archive: asset(&format!(
            "PaladinsCat-performance-v{version}-windows-x64.zip"
        ))?,
        checksums: asset(SUMS)?,
    };
    release.validate()?;
    if github.html_url != release.page() {
        return Err("Unexpected performance release repository".into());
    }
    Ok(release)
}
fn check_url(url: &str) -> Result<Release> {
    let response = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout(Duration::from_secs(30))
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .build()
        .get(url)
        .set(
            "User-Agent",
            concat!("PaladinsCat-Launcher/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(error)?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(METADATA_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    parse_release(&bytes)
}
pub fn check() -> Result<Release> {
    check_url(crate::sources::PERFORMANCE_RELEASE_API)
}
fn check_version(version: &str) -> Result<Release> {
    if !stable_version(version) {
        return Err("Invalid performance version".into());
    }
    let release = check_url(&format!(
        "https://api.github.com/repos/{REPOSITORY}/releases/tags/v{version}"
    ))?;
    if release.version != version {
        return Err("Performance release tag mismatch".into());
    }
    Ok(release)
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn unique() -> String {
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(format!(
                "Performance package reparse point refused: {}",
                path.display()
            ));
        }
    }
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(format!(
            "Performance package requires an ordinary {}: {}",
            if directory { "directory" } else { "file" },
            path.display()
        ));
    }
    Ok(())
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    plain(path, false)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(error)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > limit {
        return Err("Performance package file exceeds size limit".into());
    }
    Ok(bytes)
}
fn pinned(path: &Path, sha: &str, length: Option<u64>) -> Result<()> {
    plain(path, false)?;
    if length.is_some_and(|n| fs::metadata(path).map(|m| m.len() != n).unwrap_or(true)) {
        return Err(format!(
            "Performance package size mismatch: {}",
            path.display()
        ));
    }
    let mut file = File::open(path).map_err(error)?;
    let mut state = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        state.update(&buffer[..count]);
    }
    if format!("{:x}", state.finalize()) != sha {
        return Err(format!(
            "Pinned performance SHA-256 mismatch: {}",
            path.display()
        ));
    }
    Ok(())
}
fn validate_contents(package: &Package) -> Result<()> {
    if package.schema != 1
        || !stable_version(&package.version)
        || package.mod_id != "paladins-performance"
        || package.platform != "windows-x86_64"
        || !hash(&package.game_sha256)
        || !hash(&package.dinput8_sha256)
        || !hash(&package.user32_sha256)
        || package.assets.len() != ASSET_NAMES.len()
    {
        return Err(
            "Unsupported performance release identity, native pins or verification claims".into(),
        );
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for asset in &package.assets {
        if !ASSET_NAMES.contains(&asset.name.as_str())
            || !seen.insert(asset.name.as_str())
            || !hash(&asset.sha256)
            || asset.bytes == 0
            || asset.bytes > LIMIT
        {
            return Err("Unsupported, duplicate or oversized performance asset".into());
        }
        total = total
            .checked_add(asset.bytes)
            .ok_or("Performance asset size overflow")?;
    }
    if total > LIMIT {
        return Err("Performance package exceeds expanded size limit".into());
    }
    Ok(())
}
fn parse_cached(directory: &Path, bytes: &[u8]) -> Result<Package> {
    let release: Release =
        serde_json::from_slice(&read(&directory.join(RECEIPT), JSON_LIMIT)?).map_err(error)?;
    release.package(bytes)
}
fn sums(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).map_err(error)?;
    let mut result = BTreeMap::new();
    for line in text.lines() {
        let (sha, name) = line
            .split_once("  ")
            .ok_or("Invalid performance checksums")?;
        if !hash(sha)
            || !ASSET_NAMES.contains(&name) && name != MANIFEST
            || result.insert(name.into(), sha.into()).is_some()
        {
            return Err("Unexpected or repeated performance checksum entry".into());
        }
    }
    Ok(result)
}
fn check_sums(bytes: &[u8], package: &Package, manifest: &[u8]) -> Result<()> {
    let mut expected: BTreeMap<_, _> = package
        .assets
        .iter()
        .map(|a| (a.name.clone(), a.sha256.clone()))
        .collect();
    expected.insert(MANIFEST.into(), digest(manifest));
    if sums(bytes)? != expected {
        return Err("Performance bundle checksum inventory mismatch".into());
    }
    Ok(())
}
fn inventory(directory: &Path) -> Result<()> {
    plain(directory, true)?;
    let mut seen = HashSet::new();
    for entry in fs::read_dir(directory).map_err(error)? {
        let entry = entry.map_err(error)?;
        plain(&entry.path(), false)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Non-Unicode performance member")?;
        if !ASSET_NAMES.contains(&name.as_str())
            && name != MANIFEST
            && name != SUMS
            && name != RECEIPT
        {
            return Err("Unexpected member in cached performance package".into());
        }
        seen.insert(name);
    }
    if seen.len() != ASSET_NAMES.len() + 3 || !seen.contains(RECEIPT) {
        return Err("Incomplete cached performance package".into());
    }
    Ok(())
}

fn cache_directory(parent: &Path, name: &str) -> Result<PathBuf> {
    // Validate an existing component before any create/write follows it. The
    // caller-selected cache root may be canonicalized, but package-owned child
    // directories must never redirect extraction/downloads through junctions.
    plain(parent, true)?;
    let path = parent.join(name);
    if path.exists() {
        plain(&path, true)?;
    } else {
        fs::create_dir(&path).map_err(error)?;
    }
    Ok(path)
}

fn performance_cache(cache: &Path) -> Result<PathBuf> {
    if !cache.exists() {
        fs::create_dir_all(cache).map_err(error)?;
    }
    plain(cache, true)?;
    let cache = cache.canonicalize().map_err(error)?;
    cache_directory(&cache, "performance")
}

pub fn manifest_path(cached: &Cached) -> PathBuf {
    cached.directory.join(MANIFEST)
}

/// Revalidates persisted settings against pinned release bytes and all members.
pub fn verify(cached: &Cached) -> Result<()> {
    if !cached.directory.is_absolute() {
        return Err("Performance cache path must be absolute".into());
    }
    inventory(&cached.directory)?;
    let bytes = read(&manifest_path(cached), JSON_LIMIT)?;
    let package = parse_cached(&cached.directory, &bytes)?;
    if package != cached.package {
        return Err("Performance settings do not match cached release manifest".into());
    }
    for asset in &package.assets {
        pinned(
            &cached.directory.join(&asset.name),
            &asset.sha256,
            Some(asset.bytes),
        )?;
    }
    check_sums(
        &read(&cached.directory.join(SUMS), JSON_LIMIT)?,
        &package,
        &bytes,
    )
}

/// Select the package's input variant and scheduler after metadata verification.
pub fn dlls(cached: &Cached, rollback: bool) -> Result<Vec<PathBuf>> {
    verify(cached)?;
    let selected = vec![
        cached
            .directory
            .join(PAYLOADS[if rollback { 2 } else { 0 }]),
        cached.directory.join(PAYLOADS[1]),
    ];
    Ok(selected)
}

/// Fail closed after game/Windows updates; retain original mod native pins.
pub fn preflight(cached: &Cached, game_root: &Path, system: &Path) -> Result<()> {
    if !game_root.is_absolute() || !system.is_absolute() {
        return Err("Performance game and Windows system paths must be absolute".into());
    }
    verify(cached)?;
    pinned(
        &game_root.join("Binaries/Win64/Paladins.exe"),
        &cached.package.game_sha256,
        None,
    )?;
    pinned(
        &system.join("dinput8.dll"),
        &cached.package.dinput8_sha256,
        None,
    )?;
    pinned(
        &system.join("user32.dll"),
        &cached.package.user32_sha256,
        None,
    )
}

fn unpack(archive: &Path, package: &Package, manifest: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let raw = read(archive, LIMIT)?;
    // zip's name index can collapse repeated central-directory names. Check
    // the actual classic ZIP inventory before that index is built. This
    // release has no ZIP64, disks, comment, preamble or trailing bytes.
    let end = raw
        .len()
        .checked_sub(22)
        .ok_or("Truncated performance ZIP")?;
    let trailer = &raw[end..];
    let word = |at| u16::from_le_bytes(trailer[at..at + 2].try_into().unwrap());
    let dword = |at| u32::from_le_bytes(trailer[at..at + 4].try_into().unwrap());
    if trailer[..4] != [0x50, 0x4b, 0x05, 0x06]
        || word(4) != 0
        || word(6) != 0
        || word(20) != 0
        || word(8) as usize != ASSET_NAMES.len() + 2
        || word(10) != word(8)
        || u64::from(dword(12)) + u64::from(dword(16)) != end as u64
        || raw.get(..4) != Some(&[0x50, 0x4b, 0x03, 0x04])
    {
        return Err("Performance ZIP member count or classic archive layout mismatch".into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(raw)).map_err(error)?;
    if zip.len() != ASSET_NAMES.len() + 2 {
        return Err("Performance ZIP member count mismatch".into());
    }
    let mut members = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(error)?;
        let name = entry.name().to_string();
        // Exact ASCII basenames prevent traversal, drive/UNC/ADS paths, case
        // collisions and separator tricks before creating any destination.
        if entry.is_dir()
            || entry.enclosed_name().is_none()
            || name.contains(['/', '\\', ':'])
            || !ASSET_NAMES.contains(&name.as_str()) && name != MANIFEST && name != SUMS
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err(format!(
                "Unsafe or unexpected performance ZIP member: {name}"
            ));
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err("Duplicate performance ZIP member".into());
        }
        let limit = package
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.bytes)
            .unwrap_or(JSON_LIMIT);
        if entry.size() > limit || entry.compressed_size() > LIMIT {
            return Err("Performance ZIP member exceeds declared size".into());
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("Performance ZIP size overflow")?;
        if expanded > LIMIT {
            return Err("Performance ZIP expanded size limit".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() as u64 != entry.size() || bytes.len() as u64 > limit {
            return Err("Performance ZIP member size mismatch".into());
        }
        if let Some(asset) = package.assets.iter().find(|a| a.name == name) {
            if bytes.len() as u64 != asset.bytes || digest(&bytes) != asset.sha256 {
                return Err(format!("Performance ZIP member hash mismatch: {name}"));
            }
        } else if name == MANIFEST && bytes != manifest {
            return Err("Embedded performance manifest mismatch".into());
        }
        members.insert(name, bytes);
    }
    let checksums = members
        .get(SUMS)
        .ok_or("Missing performance ZIP checksums")?;
    check_sums(checksums, package, manifest)?;
    Ok(members)
}
fn stage(cache: &Path, package: Package, members: BTreeMap<String, Vec<u8>>) -> Result<Cached> {
    fs::create_dir_all(cache).map_err(error)?;
    plain(cache, true)?;
    let cache = cache.canonicalize().map_err(error)?;
    let manifest_sha = digest(
        members
            .get(MANIFEST)
            .ok_or("Missing performance manifest")?,
    );
    let destination = cache.join(manifest_sha);
    if destination.exists() {
        let current = Cached {
            package: package.clone(),
            directory: destination.clone(),
        };
        if verify(&current).is_ok() {
            return Ok(current);
        }
        // Preserve corrupt/unknown files instead of deleting or overwriting them.
        fs::rename(&destination, cache.join(format!("quarantine-{}", unique()))).map_err(error)?;
    }
    let staging = cache.join(format!("staging-{}", unique()));
    fs::create_dir(&staging).map_err(error)?;
    for (name, bytes) in members {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging.join(name))
            .map_err(error)?;
        output.write_all(&bytes).map_err(error)?;
        output.sync_all().map_err(error)?;
    }
    verify(&Cached {
        package: package.clone(),
        directory: staging.clone(),
    })?;
    fs::rename(staging, &destination).map_err(error)?;
    Ok(Cached {
        package,
        directory: destination,
    })
}

/// Discover the current stable release, then download its immutable asset URLs
/// using GitHub-provided digests and sizes. No package version is compiled in.
pub fn download(cache: &Path, progress: Progress<'_>) -> Result<Cached> {
    progress("Checking performance releases…".into());
    let release = check()?;
    let _lease = crate::cache::lock(cache)?;
    let mut attempt = crate::cache::Attempt::begin(cache, &["performance"])?;
    let root = performance_cache(cache)?;
    let existing = root.join(&release.manifest.sha256);
    if existing.exists() {
        plain(&existing, true)?;
        if let Ok(package) =
            read(&existing.join(MANIFEST), JSON_LIMIT).and_then(|bytes| release.package(&bytes))
        {
            let cached = Cached {
                package,
                directory: existing,
            };
            if verify(&cached).is_ok() {
                attempt.commit();
                crate::cache::prepared_cleanup(crate::cache::retain_family(
                    cache,
                    "performance",
                    &cached.directory,
                ));
                progress("Performance package verified and prepared; no game files changed".into());
                return Ok(cached);
            }
        }
    }
    let downloads = cache_directory(&root, "downloads")?;
    progress(format!(
        "Downloading performance package v{}…",
        release.version
    ));
    for asset in [&release.manifest, &release.archive, &release.checksums] {
        let path = downloads.join(&asset.name);
        if path.exists() {
            plain(&path, false)?;
        }
        engine::fetch(
            &crate::sources::performance_asset_url(&release.version, &asset.name),
            &path,
            &asset.sha256,
            Some(asset.bytes),
            progress,
        )?;
        pinned(&path, &asset.sha256, Some(asset.bytes))?;
    }
    let manifest = read(&downloads.join(MANIFEST), JSON_LIMIT)?;
    let package = release.package(&manifest)?;
    let expected = format!(
        "{}  {}\n{}  {MANIFEST}\n",
        release.archive.sha256, release.archive.name, release.manifest.sha256
    );
    if read(&downloads.join(SUMS), JSON_LIMIT)? != expected.as_bytes() {
        return Err("Published performance archive checksum inventory mismatch".into());
    }
    let mut members = unpack(&downloads.join(&release.archive.name), &package, &manifest)?;
    members.insert(RECEIPT.into(), serde_json::to_vec(&release).map_err(error)?);
    let cached = stage(&root, package, members)?;
    attempt.commit();
    crate::cache::prepared_cleanup(crate::cache::retain_family(
        cache,
        "performance",
        &cached.directory,
    ));
    progress("Performance package verified and prepared; no game files changed".into());
    Ok(cached)
}

/// Restore owned verified cache generations offline. New imports are checked
/// against their GitHub tag metadata before any package files are accepted.
pub fn import(manifest: &Path, cache: &Path) -> Result<Cached> {
    let bytes = read(manifest, JSON_LIMIT)?;
    let source = manifest
        .parent()
        .ok_or("Performance manifest has no parent directory")?;
    let directory = source.canonicalize().map_err(error)?;
    if cache
        .canonicalize()
        .ok()
        .is_some_and(|root| directory == root.join("performance").join(digest(&bytes)))
    {
        let cached = Cached {
            package: parse_cached(&directory, &bytes)?,
            directory,
        };
        verify(&cached)?;
        return Ok(cached);
    }
    let untrusted: Package = serde_json::from_slice(&bytes).map_err(error)?;
    validate_contents(&untrusted)?;
    let release = check_version(&untrusted.version)?;
    let package = release.package(&bytes)?;
    let _lease = crate::cache::lock(cache)?;
    let mut attempt = crate::cache::Attempt::begin(cache, &["performance"])?;
    let archive = source.join(&release.archive.name);
    let mut members = if archive.exists() {
        pinned(
            &archive,
            &release.archive.sha256,
            Some(release.archive.bytes),
        )?;
        unpack(&archive, &package, &bytes)?
    } else {
        let mut members = BTreeMap::new();
        for name in ASSET_NAMES.into_iter().chain([MANIFEST, SUMS]) {
            members.insert(name.into(), read(&source.join(name), LIMIT)?);
        }
        members
    };
    members.insert(RECEIPT.into(), serde_json::to_vec(&release).map_err(error)?);
    let cached = stage(&performance_cache(cache)?, package, members)?;
    attempt.commit();
    crate::cache::prepared_cleanup(crate::cache::retain_family(
        cache,
        "performance",
        &cached.directory,
    ));
    Ok(cached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    type Entries = Vec<(String, Vec<u8>)>;

    #[test]
    fn loader_contains_no_compiled_release_version_or_checksum_pins() {
        for source in [include_str!("performance.rs"), include_str!("sources.rs")] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            for literal in production.split('"').skip(1).step_by(2) {
                assert!(
                    !stable_version(literal) && !hash(literal),
                    "Compiled release data: {literal}"
                );
                assert!(!literal.contains("/releases/download/v0."));
            }
        }
    }

    fn metadata_fixture(version: &str) -> serde_json::Value {
        let asset = |name: String| {
            serde_json::json!({
                "name":name, "state":"uploaded", "size":128,
                "digest":format!("sha256:{}", "a".repeat(64)),
                "browser_download_url":crate::sources::performance_asset_url(version, &name)
            })
        };
        serde_json::json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,
            "html_url":format!("https://github.com/{REPOSITORY}/releases/tag/v{version}"),
            "assets":[asset(MANIFEST.into()),asset(format!("PaladinsCat-performance-v{version}-windows-x64.zip")),asset(SUMS.into())]})
    }
    #[test]
    fn future_release_is_discovered_without_compiled_package_values() {
        for version in ["0.1.1", "0.1.2", "71.23.109"] {
            let release =
                parse_release(&serde_json::to_vec(&metadata_fixture(version)).unwrap()).unwrap();
            assert_eq!(release.version, version);
            assert_eq!(release.manifest.sha256, "a".repeat(64));
        }
    }
    #[test]
    fn metadata_refuses_missing_digests_wrong_sources_drafts_and_duplicates() {
        for case in 0..7 {
            let mut value = metadata_fixture("71.23.109");
            match case {
                0 => value["draft"] = true.into(),
                1 => value["prerelease"] = true.into(),
                2 => value["assets"][0]["digest"] = serde_json::Value::Null,
                3 => {
                    value["assets"][0]["browser_download_url"] =
                        "https://example.com/mod.dll".into()
                }
                4 => {
                    value["html_url"] =
                        "https://github.com/other/repo/releases/tag/v71.23.109".into()
                }
                5 => value["assets"][0]["size"] = (JSON_LIMIT + 1).into(),
                _ => {
                    let duplicate = value["assets"][0].clone();
                    value["assets"].as_array_mut().unwrap().push(duplicate);
                }
            }
            assert!(
                parse_release(&serde_json::to_vec(&value).unwrap()).is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn manifest_and_compatibility_come_from_metadata_and_restore_offline() {
        let (package, manifest, entries) = fixture();
        let mut release =
            parse_release(&serde_json::to_vec(&metadata_fixture(&package.version)).unwrap())
                .unwrap();
        release.manifest.sha256 = digest(&manifest);
        release.manifest.bytes = manifest.len() as u64;
        assert_eq!(release.package(&manifest).unwrap(), package);
        let mut changed = manifest.clone();
        changed.push(b' ');
        assert!(release.package(&changed).is_err());
        let cache = temp();
        let mut members = unpack(&archive(&entries, false), &package, &manifest).unwrap();
        members.insert(RECEIPT.into(), serde_json::to_vec(&release).unwrap());
        let cached = stage(
            &performance_cache(&cache).unwrap(),
            package.clone(),
            members,
        )
        .unwrap();
        assert_eq!(
            import(&manifest_path(&cached), &cache).unwrap().directory,
            cached.directory
        );
        let game = temp();
        let system = temp();
        fs::create_dir_all(game.join("Binaries/Win64")).unwrap();
        fs::write(game.join("Binaries/Win64/Paladins.exe"), b"game fixture").unwrap();
        fs::write(system.join("dinput8.dll"), b"dinput fixture").unwrap();
        fs::write(system.join("user32.dll"), b"user32 fixture").unwrap();
        preflight(&cached, &game, &system).unwrap();
        fs::write(system.join("user32.dll"), b"changed Windows fixture").unwrap();
        assert!(preflight(&cached, &game, &system).is_err());
    }

    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(format!("paladinscat-performance-test-{}", unique()));
        fs::create_dir(&root).unwrap();
        root
    }
    fn fixture() -> (Package, Vec<u8>, Entries) {
        let assets: Vec<_> = ASSET_NAMES
            .iter()
            .map(|name| Asset {
                name: (*name).into(),
                sha256: digest(name.as_bytes()),
                bytes: name.len() as u64,
            })
            .collect();
        let package = Package {
            schema: 1,
            version: "99.4.3".into(),
            mod_id: "paladins-performance".into(),
            platform: "windows-x86_64".into(),
            source_revision: "a".repeat(40),
            game_sha256: digest(b"game fixture"),
            dinput8_sha256: digest(b"dinput fixture"),
            user32_sha256: digest(b"user32 fixture"),
            performance_verified: false,
            compatibility_verified: false,
            rendering: "uncapped; 1000 FPS target is not a limiter".into(),
            assets,
        };
        let manifest = serde_json::to_vec(&package).unwrap();
        let mut entries: Vec<_> = ASSET_NAMES
            .iter()
            .map(|name| ((*name).into(), name.as_bytes().to_vec()))
            .collect();
        let mut checksums: String = package
            .assets
            .iter()
            .map(|a| format!("{}  {}\n", a.sha256, a.name))
            .collect();
        checksums.push_str(&format!("{}  {MANIFEST}\n", digest(&manifest)));
        entries.push((MANIFEST.into(), manifest.clone()));
        entries.push((SUMS.into(), checksums.into_bytes()));
        (package, manifest, entries)
    }
    fn archive(entries: &[(String, Vec<u8>)], symlink: bool) -> PathBuf {
        let root = temp();
        let path = root.join("fixture.zip");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (index, (name, bytes)) in entries.iter().enumerate() {
            if symlink && index == 0 {
                zip.add_symlink(name, "elsewhere", options).unwrap();
            } else {
                zip.start_file(name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
        }
        fs::write(&path, zip.finish().unwrap().into_inner()).unwrap();
        path
    }
    #[test]
    fn bounded_archive_validates_all_members_and_checksums() {
        let (package, manifest, entries) = fixture();
        let result = unpack(&archive(&entries, false), &package, &manifest).unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result[PAYLOADS[1]], PAYLOADS[1].as_bytes());
    }
    #[test]
    fn zip_rejects_traversal_unexpected_case_and_symlink_members() {
        let (package, manifest, entries) = fixture();
        for name in [
            "../mod.dll",
            "C:/mod.dll",
            "mod.dll:payload",
            "README.MD",
            "unexpected.dll",
            "folder/README.md",
            "folder\\README.md",
        ] {
            let mut altered = entries.clone();
            altered[0].0 = name.into();
            let error = unpack(&archive(&altered, false), &package, &manifest).unwrap_err();
            assert!(error.contains("Unsafe or unexpected"), "{name}: {error}");
        }
        assert!(
            unpack(&archive(&entries, true), &package, &manifest)
                .unwrap_err()
                .contains("Unsafe or unexpected")
        );
    }
    #[test]
    fn zip_rejects_duplicate_missing_oversized_and_corrupt_assets() {
        let (package, manifest, entries) = fixture();
        let duplicate = archive(&entries, false);
        let bytes = fs::read(&duplicate).unwrap();
        let end = bytes.len() - 22;
        let mut trailer = bytes[end..].to_vec();
        let offset = u32::from_le_bytes(trailer[16..20].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(trailer[12..16].try_into().unwrap());
        let name = u16::from_le_bytes(bytes[offset + 28..offset + 30].try_into().unwrap()) as usize;
        let extra =
            u16::from_le_bytes(bytes[offset + 30..offset + 32].try_into().unwrap()) as usize;
        let comment =
            u16::from_le_bytes(bytes[offset + 32..offset + 34].try_into().unwrap()) as usize;
        let first = &bytes[offset..offset + 46 + name + extra + comment];
        let mut altered = bytes[..end].to_vec();
        altered.extend(first);
        trailer[8..10].copy_from_slice(&11u16.to_le_bytes());
        trailer[10..12].copy_from_slice(&11u16.to_le_bytes());
        trailer[12..16].copy_from_slice(&(size + first.len() as u32).to_le_bytes());
        altered.extend(trailer);
        fs::write(&duplicate, altered).unwrap();
        assert!(
            unpack(&duplicate, &package, &manifest)
                .unwrap_err()
                .contains("count")
        );
        assert!(
            unpack(&archive(&entries[..9], false), &package, &manifest)
                .unwrap_err()
                .contains("count")
        );
        let mut large = entries.clone();
        large[0].1.push(0);
        assert!(
            unpack(&archive(&large, false), &package, &manifest)
                .unwrap_err()
                .contains("size")
        );
        let mut corrupt = entries;
        corrupt[0].1[0] ^= 1;
        assert!(
            unpack(&archive(&corrupt, false), &package, &manifest)
                .unwrap_err()
                .contains("hash")
        );
    }
    #[test]
    fn zip_rejects_embedded_manifest_and_checksum_mismatch() {
        let (package, manifest, entries) = fixture();
        let mut changed = entries.clone();
        changed[8].1[0] ^= 1;
        assert!(
            unpack(&archive(&changed, false), &package, &manifest)
                .unwrap_err()
                .contains("manifest mismatch")
        );
        let mut changed = entries;
        changed[9].1 = b"garbage".to_vec();
        assert!(unpack(&archive(&changed, false), &package, &manifest).is_err());
    }
    #[test]
    fn manifest_compatibility_fingerprints_and_asset_inventory_are_validated() {
        let (package, _, _) = fixture();
        validate_contents(&package).unwrap();
        for field in [0, 1, 2, 3, 4] {
            let mut changed = package.clone();
            match field {
                0 => changed.game_sha256 = "bad".into(),
                1 => changed.dinput8_sha256 = "bad".into(),
                2 => changed.user32_sha256 = "bad".into(),
                3 => changed.assets[0].sha256 = "bad".into(),
                _ => changed.assets[0].name = "unexpected.dll".into(),
            }
            assert!(validate_contents(&changed).is_err());
        }
    }
    #[test]
    fn unpinned_manifest_and_settings_never_prepare_or_enable_payloads() {
        let (mut package, _, _) = fixture();
        package.version = "unreleased".into();
        let manifest = serde_json::to_vec(&package).unwrap();
        let root = temp();
        fs::write(root.join(MANIFEST), manifest).unwrap();
        assert!(import(&root.join(MANIFEST), &root.join("cache")).is_err());
        let cached = Cached {
            package,
            directory: root,
        };
        assert!(verify(&cached).is_err());
        assert!(dlls(&cached, false).is_err());
        assert!(dlls(&cached, true).is_err());
        assert!(preflight(&cached, Path::new("relative"), Path::new("relative")).is_err());
    }

    #[test]
    fn cache_inventory_rejects_extra_executables_and_directories() {
        let root = temp();
        for name in ASSET_NAMES.into_iter().chain([MANIFEST, SUMS]) {
            fs::write(root.join(name), name).unwrap();
        }
        fs::write(root.join(RECEIPT), b"fixture").unwrap();
        inventory(&root).unwrap();
        fs::write(root.join("unexpected.dll"), b"never allowed").unwrap();
        assert!(inventory(&root).unwrap_err().contains("Unexpected"));
        let root = temp();
        for name in ASSET_NAMES.into_iter().chain([MANIFEST, SUMS]) {
            fs::write(root.join(name), name).unwrap();
        }
        fs::write(root.join(RECEIPT), b"fixture").unwrap();
        fs::create_dir(root.join("subdirectory")).unwrap();
        assert!(inventory(&root).is_err());
    }

    #[test]
    #[ignore = "Explicit public-release download check; does not execute payloads"]
    fn published_release_download_import_and_variant_selection() {
        let root = temp();
        let cached = download(&root.join("download-cache"), &|_| {}).unwrap();
        verify(&cached).unwrap();
        let current = dlls(&cached, false).unwrap();
        let rollback = dlls(&cached, true).unwrap();
        assert_eq!(current.len(), 2);
        assert_eq!(rollback.len(), 2);
        assert_eq!(current[0].file_name().unwrap(), PAYLOADS[0]);
        assert_eq!(rollback[0].file_name().unwrap(), PAYLOADS[2]);
        assert!(
            current[1].file_name().unwrap() == PAYLOADS[1]
                && rollback[1].file_name().unwrap() == PAYLOADS[1]
        );
        let imported = import(&manifest_path(&cached), &root.join("import-cache")).unwrap();
        verify(&imported).unwrap();
        assert_eq!(imported.package, cached.package);
        assert_ne!(imported.directory, cached.directory);
        let same = import(&manifest_path(&cached), &root.join("download-cache")).unwrap();
        assert_eq!(same.directory, cached.directory);
        verify(&same).unwrap();
        let mut changed = imported;
        changed.package.game_sha256 = "0".repeat(64);
        assert!(verify(&changed).is_err());
    }
}
