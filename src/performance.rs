//! Pinned public performance packages. This module never runs package executables,
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
    time::{SystemTime, UNIX_EPOCH},
};

pub const REPOSITORY: &str = "PaladinsCat/PaladinsCat-mods";
pub const VERSION: &str = "0.1.0";
pub const RELEASE_URL: &str = "https://github.com/PaladinsCat/PaladinsCat-mods/releases/tag/v0.1.0";
const BASE: &str = "https://github.com/PaladinsCat/PaladinsCat-mods/releases/download/v0.1.0";
const MANIFEST: &str = "manifest.json";
const ARCHIVE: &str = "PaladinsCat-performance-v0.1.0-windows-x64.zip";
const SUMS: &str = "SHA256SUMS.txt";
const MANIFEST_SHA: &str = "7681f8572fa92ce49731ffa3d21ee62f37437aaeef07f95b85b54b68308b3f24";
const MANIFEST_BYTES: u64 = 1844;
const ARCHIVE_SHA: &str = "428a1703ab3a1faecc30fd21d914ad34f2de5f0d90a1637244d87bba83f260cc";
const ARCHIVE_BYTES: u64 = 720701;
const SUMS_SHA: &str = "1da28fc642ecf2f35dda771b63870dd5334a35c7fedb3e62e7ea3d26f6f67bac";
const SOURCE_REVISION: &str = "635a3469532a605b02af0a9ac9b9a7bf32a1fb91";
const LIMIT: u64 = 1024 * 1024;
const JSON_LIMIT: u64 = 16 * 1024;
pub const GAME_SHA: &str = "0c51d381dfe9ecbc1f571ab36603478626375704f4e4e40482524a1adafd6f75";
pub const DINPUT_SHA: &str = "b0f10001fccd05e9863d84588be9b9f8389656d95177287dd4c6cf3be4bf1dfe";
pub const USER32_SHA: &str = "f285937030bcb325cad39ee5be4818f6666241229adeadea4ff1ae469dfd7ef3";
pub const PAYLOADS: [(&str, &str, u64); 3] = [
    (
        "watchcat-native-input-compact-modifier.dll",
        "f45578bc07a204dd3b992a5faca7945ca1695b8a9386f4ac0072ec9b463a3f1d",
        134144,
    ),
    (
        "watchcat-world-scheduler.dll",
        "2b5893da71714f36548146d466f310490f859354803a0d0a71123bc53464e0e4",
        135168,
    ),
    (
        "watchcat-native-input-raw-read-original.dll",
        "2da0b9da101ddf82bbc7e2f3a61205b47e96bd9200864bb86854d4fb17de61e6",
        151552,
    ),
];
const ASSET_NAMES: [&str; 8] = [
    PAYLOADS[0].0,
    PAYLOADS[1].0,
    PAYLOADS[2].0,
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
fn validate(package: &Package) -> Result<()> {
    if package.schema != 1
        || package.version != VERSION
        || package.mod_id != "paladins-performance"
        || package.platform != "windows-x86_64"
        || package.source_revision != SOURCE_REVISION
        || package.game_sha256 != GAME_SHA
        || package.dinput8_sha256 != DINPUT_SHA
        || package.user32_sha256 != USER32_SHA
        || package.performance_verified
        || package.compatibility_verified
        || package.rendering != "uncapped; 1000 FPS target is not a limiter"
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
    for (name, sha, bytes) in PAYLOADS {
        if !package
            .assets
            .iter()
            .any(|a| a.name == name && a.sha256 == sha && a.bytes == bytes)
        {
            return Err("Performance DLL native pin mismatch".into());
        }
    }
    Ok(())
}
fn parse(bytes: &[u8]) -> Result<Package> {
    if bytes.len() as u64 != MANIFEST_BYTES || digest(bytes) != MANIFEST_SHA {
        return Err("The performance manifest is not the pinned published v0.1.0 release".into());
    }
    let package = serde_json::from_slice(bytes).map_err(error)?;
    validate(&package)?;
    Ok(package)
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
        if !ASSET_NAMES.contains(&name.as_str()) && name != MANIFEST && name != SUMS {
            return Err("Unexpected member in cached performance package".into());
        }
        seen.insert(name);
    }
    if seen.len() != ASSET_NAMES.len() + 2 {
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
    let package = parse(&bytes)?;
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

/// Exactly one input variant plus the unchanged world scheduler. Never executes.
pub fn dlls(cached: &Cached, rollback: bool) -> Result<Vec<PathBuf>> {
    verify(cached)?;
    Ok(vec![
        cached
            .directory
            .join(PAYLOADS[if rollback { 2 } else { 0 }].0),
        cached.directory.join(PAYLOADS[1].0),
    ])
}

/// Fail closed after game/Windows updates; retain original mod native pins.
pub fn preflight(cached: &Cached, game_root: &Path, system: &Path) -> Result<()> {
    if !game_root.is_absolute() || !system.is_absolute() {
        return Err("Performance game and Windows system paths must be absolute".into());
    }
    verify(cached)?;
    pinned(
        &game_root.join("Binaries/Win64/Paladins.exe"),
        GAME_SHA,
        None,
    )?;
    pinned(&system.join("dinput8.dll"), DINPUT_SHA, None)?;
    pinned(&system.join("user32.dll"), USER32_SHA, None)
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
    let destination = cache.join(MANIFEST_SHA);
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

/// Downloads only the explicitly pinned v0.1.0 public release, never a floating
/// latest URL. Archive, manifest and outer checksums have independent pins.
pub fn download(cache: &Path, progress: Progress<'_>) -> Result<Cached> {
    let root = performance_cache(cache)?;
    let downloads = cache_directory(&root, "downloads")?;
    progress("Downloading pinned performance package v0.1.0…".into());
    for (name, sha, bytes) in [
        (MANIFEST, MANIFEST_SHA, MANIFEST_BYTES),
        (ARCHIVE, ARCHIVE_SHA, ARCHIVE_BYTES),
        (SUMS, SUMS_SHA, 193),
    ] {
        let path = downloads.join(name);
        if path.exists() {
            plain(&path, false)?;
        }
        engine::fetch(&format!("{BASE}/{name}"), &path, sha, Some(bytes), progress)?;
        pinned(&path, sha, Some(bytes))?;
    }
    let manifest = read(&downloads.join(MANIFEST), JSON_LIMIT)?;
    let package = parse(&manifest)?;
    let expected = format!("{ARCHIVE_SHA}  {ARCHIVE}\n{MANIFEST_SHA}  {MANIFEST}\n");
    if read(&downloads.join(SUMS), JSON_LIMIT)? != expected.as_bytes() {
        return Err("Published performance archive checksum inventory mismatch".into());
    }
    let members = unpack(&downloads.join(ARCHIVE), &package, &manifest)?;
    let cached = stage(&root, package, members)?;
    progress("Performance package verified and prepared; no game files changed".into());
    Ok(cached)
}

/// Imports either the release manifest beside its pinned ZIP, or a fully
/// extracted release directory. No executable/helper is run during import.
pub fn import(manifest: &Path, cache: &Path) -> Result<Cached> {
    let bytes = read(manifest, JSON_LIMIT)?;
    let package = parse(&bytes)?;
    let source = manifest
        .parent()
        .ok_or("Performance manifest has no parent directory")?;
    let archive = source.join(ARCHIVE);
    let members = if archive.exists() {
        pinned(&archive, ARCHIVE_SHA, Some(ARCHIVE_BYTES))?;
        unpack(&archive, &package, &bytes)?
    } else {
        let current = Cached {
            package: package.clone(),
            directory: source.canonicalize().map_err(error)?,
        };
        verify(&current)?;
        let mut members = BTreeMap::new();
        for name in ASSET_NAMES.into_iter().chain([MANIFEST, SUMS]) {
            members.insert(name.into(), read(&source.join(name), LIMIT)?);
        }
        members
    };
    stage(&performance_cache(cache)?, package, members)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    type Entries = Vec<(String, Vec<u8>)>;

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
            version: VERSION.into(),
            mod_id: "paladins-performance".into(),
            platform: "windows-x86_64".into(),
            source_revision: SOURCE_REVISION.into(),
            game_sha256: GAME_SHA.into(),
            dinput8_sha256: DINPUT_SHA.into(),
            user32_sha256: USER32_SHA.into(),
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
        assert_eq!(result[PAYLOADS[1].0], PAYLOADS[1].0.as_bytes());
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
    fn native_pins_and_unverified_release_claims_cannot_be_relaxed() {
        let (mut package, _, _) = fixture();
        for (name, sha, bytes) in PAYLOADS {
            let asset = package.assets.iter_mut().find(|a| a.name == name).unwrap();
            asset.sha256 = sha.into();
            asset.bytes = bytes;
        }
        validate(&package).unwrap();
        for field in [0, 1, 2, 3, 4] {
            let mut changed = package.clone();
            match field {
                0 => changed.game_sha256 = "0".repeat(64),
                1 => changed.dinput8_sha256 = "0".repeat(64),
                2 => changed.user32_sha256 = "0".repeat(64),
                3 => changed.assets[0].sha256 = "0".repeat(64),
                _ => changed.compatibility_verified = true,
            }
            assert!(validate(&changed).is_err());
        }
    }
    #[test]
    fn unpinned_manifest_and_settings_never_prepare_or_enable_payloads() {
        let (package, manifest, _) = fixture();
        assert!(parse(&manifest).is_err());
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
        inventory(&root).unwrap();
        fs::write(root.join("unexpected.dll"), b"never allowed").unwrap();
        assert!(inventory(&root).unwrap_err().contains("Unexpected"));
        let root = temp();
        for name in ASSET_NAMES.into_iter().chain([MANIFEST, SUMS]) {
            fs::write(root.join(name), name).unwrap();
        }
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
        assert_eq!(current[0].file_name().unwrap(), PAYLOADS[0].0);
        assert_eq!(rollback[0].file_name().unwrap(), PAYLOADS[2].0);
        assert_eq!(current[1], rollback[1]);
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
