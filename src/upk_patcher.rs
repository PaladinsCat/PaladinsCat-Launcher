//! Transactional installer for prebuilt localization bundles.
//!
//! UPKs are rebuilt offline so their Scaleform length/offset tables remain valid.
//! Runtime Apply/Revert only installs hash-pinned artifacts and verified backups.

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_VERSION: u32 = 1;
const STATE_DIR: &str = ".watchcat-localization";
const ACTIVE_MANIFEST: &str = "active.json";
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub fn apply_localization(locale: &str, game_path: &str) -> Result<String, String> {
    if is_game_running() {
        return Err("Paladins.exe is running. Stop the game first.".into());
    }
    apply_bundle(&bundle_directory(locale)?, locale, game_path)
}

pub(crate) fn apply_bundle(
    bundle_dir: &Path,
    locale: &str,
    game_path: &str,
) -> Result<String, String> {
    let game_root = canonical_dir(Path::new(game_path), "game directory")?;
    let locale = locale.to_uppercase();
    let bundle: Bundle = read_json(&bundle_dir.join("bundle.json"))?;
    validate_bundle(&bundle, &locale)?;

    let state_root = game_root.join(STATE_DIR);
    let state_path = state_root.join(ACTIVE_MANIFEST);
    if state_path.exists() {
        let active: ActiveState = read_json(&state_path)?;
        if active.applied
            && active.locale == locale
            && active.bundle_id == bundle.bundle_id
            && verify_active(&active).is_ok()
        {
            return Ok(format!(
                "{} already applied ({} files).",
                locale,
                active.files.len()
            ));
        }
        return Err(format!(
            "Localization state requires recovery or revert: {}",
            state_path.display()
        ));
    }

    let mut files = Vec::with_capacity(bundle.files.len());
    for entry in &bundle.files {
        validate_relative(&entry.source)?;
        validate_relative(&entry.target)?;
        let artifact = bundle_dir.join(&entry.source);
        let target = game_root.join(&entry.target);
        if !artifact
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(bundle_dir.canonicalize().map_err(|e| e.to_string())?)
            || !target
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&game_root)
        {
            return Err("Bundle source or target resolves outside its directory.".into());
        }
        require_hash(&artifact, &entry.artifact_sha256, "artifact")?;
        require_hash(&target, &entry.stock_sha256, "stock target")?;
        files.push((entry, artifact, target));
    }

    let backup_root = state_root.join("backups").join(&bundle.bundle_id);
    let mut active_files = Vec::with_capacity(files.len());
    for (entry, _, target) in &files {
        let backup = backup_root.join(&entry.target);
        copy_verified(target, &backup, &entry.stock_sha256)?;
        active_files.push(ActiveFile {
            target: target.clone(),
            backup,
            stock_sha256: normalize_hash(&entry.stock_sha256)?,
            artifact_sha256: normalize_hash(&entry.artifact_sha256)?,
        });
    }

    let mut active = ActiveState {
        version: STATE_VERSION,
        bundle_id: bundle.bundle_id.clone(),
        locale: locale.clone(),
        game_root: game_root.clone(),
        applied: false,
        files: active_files,
    };
    save_json_atomic(&state_path, &active)?;

    let mut installed = 0usize;
    let install_result = (|| {
        for ((_, artifact, target), state) in files.iter().zip(&active.files) {
            replace_verified(artifact, target, &state.artifact_sha256)?;
            installed += 1;
        }
        verify_active(&active)?;
        active.applied = true;
        save_json_atomic(&state_path, &active)
    })();

    if let Err(error) = install_result {
        let rollback = rollback_prefix(&active.files, installed);
        if rollback.is_ok() {
            let _ = fs::remove_file(&state_path);
        }
        return Err(match rollback {
            Ok(()) => format!("Apply failed and was rolled back: {error}"),
            Err(rollback_error) => format!(
                "Apply failed: {error}; automatic rollback also failed: {rollback_error}. State: {}",
                state_path.display()
            ),
        });
    }

    Ok(format!(
        "{} applied: {} verified localization files.",
        locale,
        active.files.len()
    ))
}

pub fn revert_localization(_locale: Option<&str>, game_path: &str) -> Result<String, String> {
    if is_game_running() {
        return Err("Paladins.exe is running. Stop the game first.".into());
    }
    revert_bundle(game_path)
}

pub(crate) fn revert_bundle(game_path: &str) -> Result<String, String> {
    let game_root = canonical_dir(Path::new(game_path), "game directory")?;
    let state_path = game_root.join(STATE_DIR).join(ACTIVE_MANIFEST);
    if !state_path.exists() {
        return Ok("No localization bundle is applied.".into());
    }
    let active: ActiveState = read_json(&state_path)?;
    if active.version != STATE_VERSION || active.game_root != game_root {
        return Err("Localization state does not match this game installation.".into());
    }

    for file in &active.files {
        if !file
            .target
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&game_root)
            || !file
                .backup
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(game_root.join(STATE_DIR))
        {
            return Err("Localization receipt resolves outside this installation.".into());
        }
    }

    for file in &active.files {
        require_hash(&file.backup, &file.stock_sha256, "backup")?;
        let live = sha256_file(&file.target)?;
        if live != file.artifact_sha256 && live != file.stock_sha256 {
            return Err(format!(
                "Refusing to overwrite changed file: {} (hash {})",
                file.target.display(),
                live
            ));
        }
    }

    let mut restored = 0usize;
    for file in &active.files {
        if sha256_file(&file.target)? == file.stock_sha256 {
            restored += 1;
            continue;
        }
        if let Err(error) = replace_verified(&file.backup, &file.target, &file.stock_sha256) {
            return Err(format!(
                "Revert stopped after {restored} file(s): {error}. Verified backups remain in {}",
                file.backup.parent().unwrap_or(Path::new(".")).display()
            ));
        }
        restored += 1;
    }
    for file in &active.files {
        require_hash(&file.target, &file.stock_sha256, "restored target")?;
    }
    fs::remove_file(&state_path)
        .map_err(|e| format!("Failed to clear {}: {e}", state_path.display()))?;

    Ok(format!(
        "Reverted {}: {} files restored from verified backups.",
        active.locale, restored
    ))
}

/// Recover one explicitly identified out-of-band file to its verified stock backup.
///
/// This is intentionally narrower than Revert: the caller must provide the exact
/// currently installed SHA-256, and the target must already belong to active.json.
pub fn recover_file_to_stock(
    game_path: &str,
    relative_target: &str,
    expected_live_sha256: &str,
) -> Result<String, String> {
    if is_game_running() {
        return Err("Paladins.exe is running. Stop the game first.".into());
    }

    let game_root = canonical_dir(Path::new(game_path), "game directory")?;
    let relative_target = Path::new(relative_target);
    validate_relative(relative_target)?;
    let state_path = game_root.join(STATE_DIR).join(ACTIVE_MANIFEST);
    let active: ActiveState = read_json(&state_path)?;
    if active.version != STATE_VERSION || active.game_root != game_root || !active.applied {
        return Err("Localization state does not match this game installation.".into());
    }

    let target = game_root.join(relative_target);
    let file = active
        .files
        .iter()
        .find(|file| file.target == target)
        .ok_or_else(|| {
            format!(
                "Target is not recorded in active state: {}",
                target.display()
            )
        })?;

    require_hash(&file.backup, &file.stock_sha256, "backup")?;
    require_hash(
        &file.target,
        expected_live_sha256,
        "expected changed target",
    )?;
    replace_verified(&file.backup, &file.target, &file.stock_sha256)?;
    require_hash(&file.target, &file.stock_sha256, "recovered target")?;

    Ok(format!(
        "Recovered {} to verified stock hash {}.",
        relative_target.display(),
        file.stock_sha256
    ))
}

fn rollback_prefix(files: &[ActiveFile], installed: usize) -> Result<(), String> {
    for file in files.iter().take(installed).rev() {
        replace_verified(&file.backup, &file.target, &file.stock_sha256)?;
    }
    Ok(())
}

fn validate_bundle(bundle: &Bundle, expected_locale: &str) -> Result<(), String> {
    if bundle.version != STATE_VERSION || bundle.locale.to_uppercase() != expected_locale {
        return Err("Bundle version/locale mismatch.".into());
    }
    if bundle.bundle_id.trim().is_empty() || bundle.files.is_empty() {
        return Err("Bundle id/files must not be empty.".into());
    }
    if !bundle
        .bundle_id
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err("Bundle id must be a safe directory name.".into());
    }
    let mut targets = std::collections::HashSet::new();
    for entry in &bundle.files {
        validate_relative(&entry.source)?;
        validate_relative(&entry.target)?;
        if !targets.insert(entry.target.to_string_lossy().to_ascii_lowercase()) {
            return Err("Bundle contains duplicate target paths.".into());
        }
    }
    Ok(())
}

fn verify_active(active: &ActiveState) -> Result<(), String> {
    for file in &active.files {
        require_hash(&file.backup, &file.stock_sha256, "backup")?;
        require_hash(&file.target, &file.artifact_sha256, "patched target")?;
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(format!(
            "Bundle path must be relative and contained: {}",
            path.display()
        ));
    }
    Ok(())
}

fn canonical_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("{} not found: {}", label, path.display()));
    }
    path.canonicalize()
        .map_err(|e| format!("Failed to resolve {} {}: {e}", label, path.display()))
}

fn copy_verified(source: &Path, destination: &Path, expected: &str) -> Result<(), String> {
    if destination.exists() {
        return require_hash(destination, expected, "existing backup");
    }
    let parent = destination.parent().ok_or("Destination has no parent")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    fs::copy(source, destination).map_err(|e| {
        format!(
            "Failed to copy {} to {}: {e}",
            source.display(),
            destination.display()
        )
    })?;
    fs::OpenOptions::new()
        .write(true)
        .open(destination)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    require_hash(destination, expected, "copied file")
}

fn replace_verified(source: &Path, destination: &Path, expected: &str) -> Result<(), String> {
    let parent = destination.parent().ok_or("Destination has no parent")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stem = destination
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("asset");
    let staged = parent.join(format!(".{stem}.watchcat-{nonce}.new"));
    #[cfg(not(windows))]
    let previous = parent.join(format!(".{stem}.watchcat-{nonce}.old"));

    fs::copy(source, &staged)
        .map_err(|e| format!("Failed to stage {}: {e}", destination.display()))?;
    if let Err(error) = require_hash(&staged, expected, "staged file") {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    #[cfg(windows)]
    {
        fs::OpenOptions::new()
            .write(true)
            .open(&staged)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        atomic_replace(&staged, destination)?;
        require_hash(destination, expected, "installed file")
    }
    #[cfg(not(windows))]
    {
        fs::rename(destination, &previous)
            .map_err(|e| format!("Failed to stage current {}: {e}", destination.display()))?;
        if let Err(error) = fs::rename(&staged, destination) {
            let _ = fs::rename(&previous, destination);
            let _ = fs::remove_file(&staged);
            return Err(format!(
                "Failed to install {}: {error}",
                destination.display()
            ));
        }
        if let Err(error) = require_hash(destination, expected, "installed file") {
            let _ = fs::remove_file(destination);
            let _ = fs::rename(&previous, destination);
            return Err(error);
        }
        fs::remove_file(&previous).map_err(|e| {
            format!(
                "Installed but failed to remove staging file {}: {e}",
                previous.display()
            )
        })
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        ReplaceFileW(
            to.as_ptr(),
            from.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(format!(
            "Atomic file replacement failed: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn save_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().ok_or("Manifest path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{ACTIVE_MANIFEST}.tmp-{}-{nonce}",
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut staging = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| format!("Failed to create {}: {e}", temp.display()))?;
    staging
        .write_all(&bytes)
        .and_then(|_| staging.sync_all())
        .map_err(|e| e.to_string())?;
    drop(staging);
    #[cfg(windows)]
    if path.exists() {
        return atomic_replace(&temp, path);
    }
    if path.exists() {
        let old = parent.join(format!(
            ".{ACTIVE_MANIFEST}.old-{}-{nonce}",
            std::process::id()
        ));
        fs::rename(path, &old).map_err(|e| e.to_string())?;
        if let Err(error) = fs::rename(&temp, path) {
            let _ = fs::rename(&old, path);
            return Err(error.to_string());
        }
        fs::remove_file(old).map_err(|e| e.to_string())?;
    } else {
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid {}: {e}", path.display()))
}

pub(crate) fn require_hash(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let expected = normalize_hash(expected)?;
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(format!(
            "{} hash mismatch for {}: expected {}, got {}",
            label,
            path.display(),
            expected,
            actual
        ));
    }
    Ok(())
}

fn normalize_hash(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("Invalid SHA-256: {value}"));
    }
    Ok(value)
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Failed to hash {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn is_game_running() -> bool {
    let mut command = Command::new("tasklist");
    command.args(["/FI", "IMAGENAME eq Paladins.exe"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .output()
        .map(|out| {
            !out.status.success()
                || String::from_utf8_lossy(&out.stdout)
                    .to_ascii_lowercase()
                    .contains("paladins.exe")
        })
        .unwrap_or(true)
}

fn project_root_path() -> Result<PathBuf, String> {
    let mut starts = Vec::new();
    if let Ok(path) = std::env::current_dir() {
        starts.push(path);
    }
    if let Ok(path) = std::env::current_exe()
        && let Some(parent) = path.parent()
    {
        starts.push(parent.to_path_buf());
    }
    for start in starts {
        for candidate in start.ancestors() {
            if candidate.join("tools/upk-patcher/patches").is_dir() {
                return Ok(candidate.to_path_buf());
            }
        }
    }
    Err("Could not locate tools/upk-patcher/patches from the runtime.".into())
}

pub(crate) fn bundle_directory(locale: &str) -> Result<PathBuf, String> {
    if locale.is_empty()
        || !locale
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Invalid localization locale".into());
    }
    Ok(project_root_path()?
        .join("tools/upk-patcher/patches")
        .join(locale.to_uppercase()))
}

/// Verify the selected artifacts and every installed receipt/backup against it.
/// Returns whether the selected bundle is currently installed.
pub(crate) fn verify_selected_bundle(locale: &str, game_path: &str) -> Result<bool, String> {
    let dir = bundle_directory(locale)?;
    verify_bundle_at(&dir, locale, game_path)
}

fn verify_bundle_at(dir: &Path, locale: &str, game_path: &str) -> Result<bool, String> {
    let bundle: Bundle = read_json(&dir.join("bundle.json"))?;
    validate_bundle(&bundle, &locale.to_uppercase())?;
    let root = canonical_dir(Path::new(game_path), "game directory")?;
    let state = root.join(STATE_DIR).join(ACTIVE_MANIFEST);
    let active: Option<ActiveState> = if state.exists() {
        Some(read_json(&state)?)
    } else {
        None
    };
    if let Some(active) = &active {
        if !active.applied
            || active.version != STATE_VERSION
            || active.game_root != root
            || active.bundle_id != bundle.bundle_id
            || active.locale != bundle.locale
            || active.files.len() != bundle.files.len()
        {
            return Err(
                "Selected bundle differs from installed state; recover or revert first.".into(),
            );
        }
        verify_active(active)?;
    }
    for entry in &bundle.files {
        validate_relative(&entry.source)?;
        validate_relative(&entry.target)?;
        require_hash(&dir.join(&entry.source), &entry.artifact_sha256, "artifact")?;
        let target = root.join(&entry.target);
        if let Some(active) = &active {
            let matches: Vec<_> = active
                .files
                .iter()
                .filter(|file| file.target == target)
                .collect();
            if matches.len() != 1
                || matches[0].artifact_sha256 != entry.artifact_sha256
                || matches[0].stock_sha256 != entry.stock_sha256
            {
                return Err("Bundle/receipt file mapping differs".into());
            }
        } else {
            require_hash(&target, &entry.stock_sha256, "stock target")?;
        }
    }
    Ok(active.is_some())
}

/// Switch only after the candidate is complete; recover the old selection on failure.
pub(crate) fn install_font_bundle(
    locale: &str,
    game_path: &str,
    candidate: &Path,
    expected_manifest_hash: &str,
) -> Result<String, String> {
    install_font_bundle_at(
        &bundle_directory(locale)?,
        locale,
        game_path,
        candidate,
        expected_manifest_hash,
        is_game_running,
    )
}

fn install_font_bundle_at(
    dir: &Path,
    locale: &str,
    game_path: &str,
    candidate: &Path,
    expected_manifest_hash: &str,
    game_running: impl Fn() -> bool,
) -> Result<String, String> {
    if game_running() {
        return Err("Stop Paladins before updating fonts.".into());
    }
    let selected = dir.join("bundle.json");
    require_hash(
        &selected,
        expected_manifest_hash,
        "unchanged selected manifest",
    )?;
    let previous: serde_json::Value = read_json(&selected)?;
    let next: serde_json::Value = read_json(candidate)?;
    let was_applied = verify_bundle_at(dir, locale, game_path)?;
    if game_running() {
        return Err("Stop Paladins before updating fonts.".into());
    }
    let result = (|| {
        if was_applied {
            revert_bundle(game_path)?;
        }
        if game_running() {
            return Err("Stop Paladins before updating fonts.".into());
        }
        save_json_atomic(&selected, &next)?;
        let message = apply_bundle(dir, locale, game_path)?;
        verify_bundle_at(dir, locale, game_path)?;
        Ok(message)
    })();
    if let Err(error) = result {
        let recovery: Result<(), String> = (|| {
            if game_running() {
                return Err("Stop Paladins to restore the previous bundle.".into());
            }
            revert_bundle(game_path)?;
            save_json_atomic(&selected, &previous)?;
            if was_applied {
                apply_bundle(dir, locale, game_path)?;
            }
            verify_bundle_at(dir, locale, game_path)?;
            Ok(())
        })();
        return Err(match recovery {
            Ok(()) => format!("Font update failed; previous bundle restored: {error}"),
            Err(recovery) => {
                format!("Font update failed: {error}; recovery requires attention: {recovery}")
            }
        });
    }
    result
}

#[derive(serde::Deserialize)]
struct Bundle {
    version: u32,
    bundle_id: String,
    locale: String,
    files: Vec<BundleFile>,
}

#[derive(serde::Deserialize)]
struct BundleFile {
    source: PathBuf,
    target: PathBuf,
    stock_sha256: String,
    artifact_sha256: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ActiveState {
    version: u32,
    bundle_id: String,
    locale: String,
    game_root: PathBuf,
    applied: bool,
    files: Vec<ActiveFile>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ActiveFile {
    target: PathBuf,
    backup: PathBuf,
    stock_sha256: String,
    artifact_sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_paths_cannot_escape() {
        assert!(validate_relative(Path::new("ChaosGame/file.upk")).is_ok());
        assert!(validate_relative(Path::new("../outside.upk")).is_err());
        assert!(validate_relative(Path::new("C:/outside.upk")).is_err());
    }

    #[test]
    fn hashes_are_strict_and_normalized() {
        assert_eq!(normalize_hash(&"A".repeat(64)).unwrap(), "a".repeat(64));
        assert!(normalize_hash("not-a-hash").is_err());
    }

    fn font_fixture() -> (PathBuf, PathBuf, serde_json::Value) {
        let root = std::env::temp_dir().join(format!(
            "watchcat-font-switch-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = root.join("bundle");
        let game = root.join("game");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("font.upk"), b"stock").unwrap();
        fs::write(dir.join("old.upk"), b"old font").unwrap();
        fs::write(dir.join("new.upk"), b"new font").unwrap();
        let old = serde_json::json!({"version":1,"locale":"KOR","bundle_id":"font-old",
            "files":[{"source":"old.upk","target":"font.upk",
                "stock_sha256":sha256_file(&game.join("font.upk")).unwrap(),
                "artifact_sha256":sha256_file(&dir.join("old.upk")).unwrap()}]});
        save_json_atomic(&dir.join("bundle.json"), &old).unwrap();
        apply_bundle(&dir, "KOR", game.to_str().unwrap()).unwrap();
        let mut next = old;
        next["bundle_id"] = "font-new".into();
        next["files"][0]["source"] = "new.upk".into();
        next["files"][0]["artifact_sha256"] = sha256_file(&dir.join("new.upk")).unwrap().into();
        (dir, game, next)
    }

    #[test]
    fn manual_font_switch_verifies_new_receipt_and_keeps_stock_backup() {
        let (dir, game, next) = font_fixture();
        let candidate = dir.join("candidate.json");
        save_json_atomic(&candidate, &next).unwrap();
        let hash = sha256_file(&dir.join("bundle.json")).unwrap();
        install_font_bundle_at(
            &dir,
            "KOR",
            game.to_str().unwrap(),
            &candidate,
            &hash,
            || false,
        )
        .unwrap();
        assert_eq!(fs::read(game.join("font.upk")).unwrap(), b"new font");
        assert!(verify_bundle_at(&dir, "KOR", game.to_str().unwrap()).unwrap());
        let active: ActiveState = read_json(&game.join(STATE_DIR).join(ACTIVE_MANIFEST)).unwrap();
        assert_eq!(active.bundle_id, "font-new");
        assert_eq!(fs::read(&active.files[0].backup).unwrap(), b"stock");
    }

    #[test]
    fn failed_font_switch_restores_previous_selection_and_installation() {
        let (dir, game, mut next) = font_fixture();
        next["files"][0]["artifact_sha256"] = "0".repeat(64).into();
        let candidate = dir.join("candidate.json");
        save_json_atomic(&candidate, &next).unwrap();
        let hash = sha256_file(&dir.join("bundle.json")).unwrap();
        let error = install_font_bundle_at(
            &dir,
            "KOR",
            game.to_str().unwrap(),
            &candidate,
            &hash,
            || false,
        )
        .unwrap_err();
        assert!(error.contains("previous bundle restored"), "{error}");
        assert_eq!(fs::read(game.join("font.upk")).unwrap(), b"old font");
        assert!(verify_bundle_at(&dir, "KOR", game.to_str().unwrap()).unwrap());
        let current: Bundle = read_json(&dir.join("bundle.json")).unwrap();
        assert_eq!(current.bundle_id, "font-old");
    }

    #[test]
    fn changed_font_selection_is_refused_before_reverting() {
        let (dir, game, next) = font_fixture();
        let candidate = dir.join("candidate.json");
        save_json_atomic(&candidate, &next).unwrap();
        let error = install_font_bundle_at(
            &dir,
            "KOR",
            game.to_str().unwrap(),
            &candidate,
            &"0".repeat(64),
            || false,
        )
        .unwrap_err();
        assert!(error.contains("unchanged selected manifest"));
        assert_eq!(fs::read(game.join("font.upk")).unwrap(), b"old font");
        // An already-applied old bundle must not be reported as the new bundle.
        save_json_atomic(&dir.join("bundle.json"), &next).unwrap();
        assert!(
            apply_bundle(&dir, "KOR", game.to_str().unwrap())
                .unwrap_err()
                .contains("recovery or revert")
        );
    }

    #[test]
    fn font_switch_blocks_running_game_before_touching_manifest_or_files() {
        let (dir, game, next) = font_fixture();
        let selected = dir.join("bundle.json");
        let before = fs::read(&selected).unwrap();
        let candidate = dir.join("candidate.json");
        save_json_atomic(&candidate, &next).unwrap();
        let error = install_font_bundle_at(
            &dir,
            "KOR",
            game.to_str().unwrap(),
            &candidate,
            &sha256_file(&selected).unwrap(),
            || true,
        )
        .unwrap_err();
        assert!(error.contains("Stop Paladins"));
        assert_eq!(fs::read(&selected).unwrap(), before);
        assert_eq!(fs::read(game.join("font.upk")).unwrap(), b"old font");
    }
}
