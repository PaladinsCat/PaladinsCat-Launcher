//! Independent Tempest IME packages. Never injects or launches the game.
use crate::{
    engine::{self, Download, Fingerprint, Progress, Result},
    upk_patcher,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
mod pe;

pub const MOD_ID: &str = "PaladinsCat-IME";
pub const DLL: &str = "PaladinsCatImeFix.dll";
pub const TARGET: &str = ".tempest/v2/mods/PaladinsCat-IME";
const STATE: &str = ".watchcat-localization";
const LIMIT: u64 = 16 * 1024 * 1024;
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub schema_version: u32,
    pub package_id: String,
    pub version: String,
    pub game_build: String,
    pub platform: String,
    pub locale: String,
    pub native_abi: [u64; 4],
    pub game_fingerprint: Fingerprint,
    pub dll_sha256: String,
    pub dll_bytes: u64,
    #[serde(default)]
    pub source_commit: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub download: Option<Download>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub package_id: String,
    pub version: String,
    pub game_build: String,
    pub manifest_url: String,
    pub manifest_sha256: String,
    #[serde(default)]
    pub notes: String,
}
#[derive(Clone, Debug)]
pub struct Cached {
    pub package: Package,
    pub directory: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    previous: Option<Package>,
    next: Option<Package>,
    archive: String,
    staging: Option<String>,
}
#[derive(Clone, Copy)]
pub enum Action {
    Apply,
    Remove,
    Recover,
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
fn identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
fn digest(s: &str) -> Result<()> {
    if s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Invalid IME SHA-256".into())
    }
}
fn version(s: &str) -> bool {
    s.split('.').count() == 3
        && s.split('.')
            .all(|s| !s.is_empty() && s.len() <= 8 && s.bytes().all(|c| c.is_ascii_digit()))
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("IME metadata exceeds size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(error)
}
fn save<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    upk_patcher::save_json_atomic(path, v)
}
pub(crate) fn validate_release(r: &Release) -> Result<()> {
    if !identity(&r.package_id) || !version(&r.version) || r.game_build.is_empty() {
        return Err("Invalid IME release identity".into());
    }
    engine::trusted_url(&r.manifest_url)?;
    digest(&r.manifest_sha256)
}
fn validate(p: &Package) -> Result<()> {
    if p.schema_version != 1
        || !identity(&p.package_id)
        || !version(&p.version)
        || p.game_build.is_empty()
        || p.platform != "Win64"
        || p.locale != "KOR"
        || p.native_abi != [4, 9, 7, 1]
        || p.game_fingerprint.path != "Binaries/Win64/Paladins.exe"
        || p.dll_bytes == 0
        || p.dll_bytes > LIMIT
    {
        return Err("Unsupported IME package/profile".into());
    }
    digest(&p.dll_sha256)?;
    digest(&p.game_fingerprint.sha256)?;
    if let Some(d) = &p.download {
        engine::trusted_url(&d.url)?;
        digest(&d.sha256)?;
        if d.encoding != "raw" || d.bytes != p.dll_bytes || d.sha256 != p.dll_sha256 {
            return Err("IME download descriptor differs from payload".into());
        }
    }
    Ok(())
}
fn safe(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.contains([':', '\\']) || relative.is_empty() {
        return Err("Unsafe IME path".into());
    }
    let mut current = root.to_path_buf();
    for c in Path::new(relative).components() {
        let Component::Normal(c) = c else {
            return Err("Unsafe IME path".into());
        };
        current.push(c);
        if let Ok(m) = fs::symlink_metadata(&current) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return Err("IME path contains a reparse point".into());
                }
            }
            if m.file_type().is_symlink()
                || !current.canonicalize().map_err(error)?.starts_with(root)
            {
                return Err("IME path escapes installation".into());
            }
        }
    }
    Ok(current)
}
fn pe(bytes: &[u8]) -> Result<()> {
    pe::verify(bytes)
}
fn payload(path: &Path, p: &Package) -> Result<()> {
    validate(p)?;
    let m = fs::symlink_metadata(path).map_err(error)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() != p.dll_bytes {
        return Err("IME payload size/type differs".into());
    }
    upk_patcher::require_hash(path, &p.dll_sha256, "IME DLL")?;
    pe(&fs::read(path).map_err(error)?)
}
fn verify_cached(c: &Cached) -> Result<()> {
    let stored: Package = read(&c.directory.join("ime-package.json"))?;
    if stored != c.package {
        return Err("IME cached manifest differs from selection".into());
    }
    payload(&c.directory.join(DLL), &c.package)
}
pub fn download(r: &Release, cache: &Path, progress: Progress<'_>) -> Result<Cached> {
    validate_release(r)?;
    let dir = cache.join("ime").join(&r.manifest_sha256);
    fs::create_dir_all(&dir).map_err(error)?;
    let manifest = dir.join("ime-package.json");
    engine::fetch(
        &r.manifest_url,
        &manifest,
        &r.manifest_sha256,
        None,
        progress,
    )?;
    let p: Package = read(&manifest)?;
    validate(&p)?;
    if p.package_id != r.package_id || p.version != r.version || p.game_build != r.game_build {
        return Err("IME catalog/manifest identity differs".into());
    }
    if p.draft
        || p.source_commit.len() != 40
        || !p.source_commit.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Published IME package requires a reviewed source commit".into());
    }
    let d = p
        .download
        .as_ref()
        .ok_or("Published IME package has no payload URL")?;
    engine::fetch(&d.url, &dir.join(DLL), &d.sha256, Some(d.bytes), progress)?;
    let c = Cached {
        package: p,
        directory: dir,
    };
    verify_cached(&c)?;
    Ok(c)
}
pub fn import(path: &Path, cache: &Path) -> Result<Cached> {
    let p: Package = read(path)?;
    validate(&p)?;
    let source = path.parent().ok_or("IME package has no parent")?.join(DLL);
    payload(&source, &p)?;
    let hash = format!("{:x}", Sha256::digest(fs::read(path).map_err(error)?));
    let dir = cache.join("ime").join(hash);
    fs::create_dir_all(&dir).map_err(error)?;
    let destination = dir.join(DLL);
    if destination.exists() {
        payload(&destination, &p)?;
    } else {
        fs::copy(source, &destination).map_err(error)?;
    }
    save(&dir.join("ime-package.json"), &p)?;
    let c = Cached {
        package: p,
        directory: dir,
    };
    verify_cached(&c)?;
    Ok(c)
}
pub fn prepare(dll: &Path, output: &Path, release_version: &str) -> Result<Package> {
    if !version(release_version) || output.exists() {
        return Err("Use a new release directory and numeric major.minor.patch version".into());
    }
    if fs::metadata(dll).map_err(error)?.len() > LIMIT {
        return Err("IME payload exceeds size limit".into());
    }
    let bytes = fs::read(dll).map_err(error)?;
    pe(&bytes)?;
    let p = Package {
        schema_version: 1,
        package_id: format!("paladinscat-ime-{}", release_version.replace('.', "-")),
        version: release_version.into(),
        game_build: "8.1".into(),
        platform: "Win64".into(),
        locale: "KOR".into(),
        native_abi: [4, 9, 7, 1],
        game_fingerprint: Fingerprint {
            path: "Binaries/Win64/Paladins.exe".into(),
            sha256: "0c51d381dfe9ecbc1f571ab36603478626375704f4e4e40482524a1adafd6f75".into(),
        },
        dll_sha256: format!("{:x}", Sha256::digest(&bytes)),
        dll_bytes: bytes.len() as u64,
        source_commit: String::new(),
        draft: true,
        notes: "Korean IME: Esc, store actions and composition compatibility.".into(),
        download: None,
    };
    validate(&p)?;
    fs::create_dir_all(output).map_err(error)?;
    fs::copy(dll, output.join(DLL)).map_err(error)?;
    save(&output.join("ime-package.json"), &p)?;
    let c = Cached {
        package: p.clone(),
        directory: output.into(),
    };
    verify_cached(&c)?;
    Ok(p)
}
fn manifest(p: &Package) -> String {
    format!(
        "[mod]\nid = \"{MOD_ID}\"\nname = \"PaladinsCat IME Fix\"\nversion = \"{}\"\n\n[[mod.authors]]\nname = \"PaladinsCat\"\n",
        p.version
    )
}
fn owned(dir: &Path, p: &Package) -> Result<()> {
    validate(p)?;
    let root = dir
        .parent()
        .ok_or("IME directory has no parent")?
        .canonicalize()
        .map_err(error)?;
    let dir = safe(
        &root,
        dir.file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid IME directory")?,
    )?;
    let mut names = fs::read_dir(&dir)
        .map_err(error)?
        .map(|e| e.map(|e| e.file_name()).map_err(error))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from("dlls"),
            std::ffi::OsString::from("manifest.toml"),
        ]
    {
        return Err("IME mod contains unrecognized files".into());
    }
    safe(&dir, "manifest.toml")?;
    safe(&dir, &format!("dlls/{DLL}"))?;
    if fs::read_to_string(dir.join("manifest.toml")).map_err(error)? != manifest(p)
        || fs::read_dir(dir.join("dlls")).map_err(error)?.count() != 1
    {
        return Err("IME mod manifest/layout differs from receipt".into());
    }
    payload(&dir.join("dlls").join(DLL), p)
}
fn active(root: &Path) -> Result<Option<Package>> {
    let receipt = safe(root, &format!("{STATE}/ime-active.json"))?;
    if receipt.exists() {
        let p: Option<Package> = read(&receipt)?;
        if let Some(p) = &p {
            validate(p)?;
        }
        Ok(p)
    } else {
        Ok(None)
    }
}
pub fn installed_package(root: &Path) -> Result<Option<Package>> {
    let root = root.canonicalize().map_err(error)?;
    let p = active(&root)?;
    let target = safe(&root, TARGET)?;
    if let Some(p) = p {
        owned(&target, &p)?;
        Ok(Some(p))
    } else if target.exists() {
        Err("IME mod exists without a recognized receipt".into())
    } else {
        Ok(None)
    }
}
pub fn installed(root: &Path) -> Result<String> {
    Ok(installed_package(root)?
        .map(|p| format!("{} · {}", p.version, p.game_build))
        .unwrap_or_else(|| "Not installed".into()))
}
pub fn recovery_pending(root: &Path) -> bool {
    root.join(STATE).join("ime-journal.json").exists()
}
fn checkpoint_path(root: &Path, path: &str, family: &str) -> Result<PathBuf> {
    let prefix = format!("{STATE}/{family}/");
    let tail = path
        .strip_prefix(&prefix)
        .ok_or("Unexpected IME journal path")?;
    if !identity(tail) {
        return Err("Unsafe IME journal path".into());
    }
    safe(root, path)
}
fn finish(root: &Path) -> Result<()> {
    fs::rename(
        safe(root, &format!("{STATE}/ime-journal.json"))?,
        safe(root, &format!("{STATE}/ime-completed-{}.json", unique()))?,
    )
    .map_err(error)
}
fn recover(root: &Path, running: &dyn Fn() -> bool) -> Result<()> {
    let j: Journal = read(&safe(root, &format!("{STATE}/ime-journal.json"))?)?;
    if let Some(p) = &j.previous {
        validate(p)?;
    }
    if let Some(p) = &j.next {
        validate(p)?;
    }
    let target = safe(root, TARGET)?;
    let archive = checkpoint_path(root, &j.archive, "ime-archive")?;
    let staging = j
        .staging
        .as_ref()
        .map(|p| checkpoint_path(root, p, "ime-staging"))
        .transpose()?;
    let is_previous = target.exists()
        && j.previous
            .as_ref()
            .is_some_and(|p| owned(&target, p).is_ok());
    let is_next = target.exists() && j.next.as_ref().is_some_and(|p| owned(&target, p).is_ok());
    if target.exists() && !is_previous && !is_next {
        return Err("IME recovery refuses changed/unrecognized current files".into());
    }
    if archive.exists() {
        owned(
            &archive,
            j.previous.as_ref().ok_or("Unexpected IME backup")?,
        )?;
    }
    if let Some(staging) = &staging
        && staging.exists()
    {
        owned(staging, j.next.as_ref().ok_or("Unexpected IME staging")?)?;
    }
    if j.previous.is_some() && !is_previous && !archive.exists() {
        return Err("Previous IME backup is missing".into());
    }
    if is_previous && archive.exists() {
        return Err("Ambiguous duplicated IME installation/backup".into());
    }
    if running() {
        return Err("Close Paladins before IME recovery".into());
    }
    if is_next && !is_previous {
        let quarantine = safe(root, &format!("{STATE}/ime-disabled/{}", unique()))?;
        fs::create_dir_all(quarantine.parent().unwrap()).map_err(error)?;
        fs::rename(&target, quarantine).map_err(error)?;
    }
    if j.previous.is_some() && !is_previous {
        fs::rename(archive, &target).map_err(error)?;
    }
    save(
        &safe(root, &format!("{STATE}/ime-active.json"))?,
        &j.previous,
    )?;
    finish(root)
}
pub fn operate(
    action: Action,
    root: &Path,
    selected: Option<Cached>,
    progress: Progress<'_>,
) -> Result<String> {
    operate_with(
        action,
        root,
        selected,
        progress,
        &upk_patcher::is_game_running,
    )
}
fn operate_with(
    action: Action,
    root: &Path,
    selected: Option<Cached>,
    progress: Progress<'_>,
    running: &dyn Fn() -> bool,
) -> Result<String> {
    let (root, _lock) = engine::acquire(root, running)?;
    safe(&root, &format!("{STATE}/ime-journal.json"))?;
    if matches!(action, Action::Recover) {
        recover(&root, running)?;
        return Ok("Previous IME installation recovered and verified".into());
    }
    if recovery_pending(&root) {
        return Err("Recover the interrupted IME operation first".into());
    }
    let previous = active(&root)?;
    let target = safe(&root, TARGET)?;
    if let Some(p) = &previous {
        owned(&target, p)?;
    } else if target.exists() {
        return Err("Refusing unrecognized existing IME mod".into());
    }
    let next = if matches!(action, Action::Apply) {
        let c = selected
            .as_ref()
            .ok_or("Download/import an IME package first")?;
        verify_cached(c)?;
        let executable = safe(&root, &c.package.game_fingerprint.path)?;
        upk_patcher::require_hash(
            &executable,
            &c.package.game_fingerprint.sha256,
            "IME game build",
        )?;
        Some(c.package.clone())
    } else {
        None
    };
    let same_payload = previous
        .as_ref()
        .zip(next.as_ref())
        .is_some_and(|(old, new)| {
            old.version == new.version
                && old.dll_sha256 == new.dll_sha256
                && old.game_fingerprint == new.game_fingerprint
                && old.native_abi == new.native_abi
        });
    if previous == next || same_payload {
        // Publishing a draft changes provenance, not the already verified mod bytes.
        if previous != next {
            save(&safe(&root, &format!("{STATE}/ime-active.json"))?, &next)?;
        }
        return Ok("IME selection already matches installation".into());
    }
    let token = unique();
    let archive_rel = format!("{STATE}/ime-archive/{token}");
    let archive = checkpoint_path(&root, &archive_rel, "ime-archive")?;
    fs::create_dir_all(archive.parent().unwrap()).map_err(error)?;
    let staging_rel = next
        .as_ref()
        .map(|_| format!("{STATE}/ime-staging/{token}"));
    if let (Some(p), Some(relative)) = (&next, &staging_rel) {
        let staging = checkpoint_path(&root, relative, "ime-staging")?;
        fs::create_dir_all(staging.join("dlls")).map_err(error)?;
        let bytes = fs::read(selected.as_ref().unwrap().directory.join(DLL)).map_err(error)?;
        for (path, bytes) in [
            (staging.join("dlls").join(DLL), bytes),
            (staging.join("manifest.toml"), manifest(p).into_bytes()),
        ] {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .map_err(error)?;
            file.write_all(&bytes).map_err(error)?;
            file.sync_all().map_err(error)?;
        }
        owned(&staging, p)?;
    }
    let j = Journal {
        previous: previous.clone(),
        next: next.clone(),
        archive: archive_rel,
        staging: staging_rel,
    };
    save(&safe(&root, &format!("{STATE}/ime-journal.json"))?, &j)?;
    let result = (|| {
        if running() {
            return Err("Close Paladins before IME replacement".into());
        }
        progress("Applying verified IME mod files…".into());
        if previous.is_some() {
            fs::rename(&target, &archive).map_err(error)?;
        }
        if let Some(staging) = &j.staging {
            fs::create_dir_all(target.parent().unwrap()).map_err(error)?;
            fs::rename(checkpoint_path(&root, staging, "ime-staging")?, &target).map_err(error)?;
            owned(&target, next.as_ref().unwrap())?;
        }
        save(&safe(&root, &format!("{STATE}/ime-active.json"))?, &next)?;
        finish(&root)
    })();
    if let Err(e) = result {
        return match recover(&root, running) {
            Ok(()) => Err(format!("{e}; previous IME installation restored")),
            Err(recovery) => Err(format!("{e}; recovery pending: {recovery}")),
        };
    }
    Ok(if let Some(p) = next {
        format!(
            "IME {} installed and verified · loaded by Tempest on the next launch",
            p.version
        )
    } else {
        "IME mod disabled; verified files retained in the archive".into()
    })
}

#[cfg(test)]
mod tests;
