//! Explicit, finite game dispatch through the installed Tempest CLI.
//!
//! Launch/profile and enabled V2-mod discovery are adapted from LowRezStudio/Tempest
//! at efe828dd28a48c4e1861ebf25a99cc16bd4b2425:
//! Tempest.CLI/Launcher/{LauncherCommands,LauncherUtility}.cs,
//! Tempest.CLI/Mods/ModCommands.cs and Tempest.CLI/TempestPathUtility.cs.
//! Upstream source: https://github.com/LowRezStudio/Tempest (GPL-3.0-or-later).
//! This backend never launches Paladins.exe directly, changes existing mod metadata,
//! kills an existing client, or treats CLI dispatch as proof that hooks are active.

use crate::{engine, ime, performance, upk_patcher};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const GAME: &str = "Binaries/Win64/Paladins.exe";
const REGISTRY: &str = ".tempest/mods/mods.json";
const MAX_METADATA: u64 = 4 * 1024 * 1024;
const MAX_MODS: usize = 4096;
const MAX_DISCOVERED_DLLS: usize = 512;
const OBSERVATION: Duration = Duration::from_secs(8);

#[derive(Clone, Debug)]
pub struct Options {
    pub game_root: PathBuf,
    pub tempest_cli: PathBuf,
    pub language: String,
    pub homedir: String,
    pub performance: Option<performance::Cached>,
    pub rollback_input: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub args: Vec<String>,
    pub game_root: PathBuf,
    pub game_executable: PathBuf,
    pub tempest_cli: PathBuf,
    /// Verified managed payloads, including any loaded once through the registry.
    pub dlls: Vec<PathBuf>,
    registry: Option<(PathBuf, String)>,
}

#[derive(Clone, Debug)]
pub struct LaunchResult {
    pub launcher_pid: u32,
    pub game_pid: Option<u32>,
    pub log: PathBuf,
    pub status: String,
}

pub fn default_tempest_cli() -> PathBuf {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Tempest Launcher/tempest-cli.exe")
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn argument_path(path: &Path) -> engine::Result<String> {
    let value = path.to_str().ok_or("Launch paths must be Unicode")?;
    if value.is_empty() || value.contains(',') || value.chars().any(char::is_control) {
        return Err("Launch paths cannot contain commas or control characters".into());
    }
    Ok(value.to_owned())
}

fn path_key(path: &Path) -> engine::Result<String> {
    let value = argument_path(path)?;
    Ok(value
        .strip_prefix(r"\\?\UNC\")
        .map(|tail| format!(r"\\{tail}"))
        .unwrap_or_else(|| value.trim_start_matches(r"\\?\").to_owned())
        .replace('/', "\\")
        .to_lowercase())
}

fn inside(root: &Path, path: &Path) -> engine::Result<PathBuf> {
    let path = path.canonicalize().map_err(error)?;
    let root_key = path_key(root)?;
    let path_key = path_key(&path)?;
    if path_key != root_key && !path_key.starts_with(&format!("{root_key}\\")) {
        return Err(format!(
            "Launch path escapes selected game: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> engine::Result<T> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(error)?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err("Launch metadata exceeds the size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid {}: {e}", path.display()))
}

#[derive(Deserialize)]
struct LocaleReceipt {
    version: u32,
    locale: String,
    game_root: PathBuf,
    applied: bool,
    files: Vec<LocaleFile>,
}
#[derive(Deserialize)]
struct LocaleFile {
    target: PathBuf,
    backup: PathBuf,
    stock_sha256: String,
    artifact_sha256: String,
}

fn verify_state(root: &Path, locale: &str) -> engine::Result<()> {
    let state = root.join(".watchcat-localization");
    if state.exists() {
        inside(root, &state)?;
    }
    if engine::recovery_pending(root) || ime::recovery_pending(root) {
        return Err("Recover the interrupted localization/IME operation before launching".into());
    }
    let receipt = state.join("active.json");
    if !receipt.exists() {
        return Ok(());
    }
    let receipt: LocaleReceipt = read_json(&inside(root, &receipt)?)?;
    if receipt.version != 1
        || !receipt.applied
        || path_key(&receipt.game_root.canonicalize().map_err(error)?)? != path_key(root)?
        || receipt.locale != locale
        || receipt.files.is_empty()
        || receipt.files.len() > MAX_MODS
    {
        return Err(
            "Installed localization receipt/profile requires recovery or a matching language"
                .into(),
        );
    }
    let state = state.canonicalize().map_err(error)?;
    let mut targets = HashSet::new();
    for file in receipt.files {
        let target = inside(root, &file.target)?;
        if !targets.insert(path_key(&target)?) {
            return Err("Installed localization receipt repeats a target".into());
        }
        let backup = inside(&state, &file.backup)?;
        upk_patcher::require_hash(&target, &file.artifact_sha256, "installed localization")?;
        upk_patcher::require_hash(&backup, &file.stock_sha256, "localization rollback backup")?;
    }
    Ok(())
}

fn validate_profile(options: &Options) -> engine::Result<()> {
    if options.language.len() != 3 || !options.language.bytes().all(|c| c.is_ascii_uppercase()) {
        return Err(
            "Language must be a three-letter uppercase game locale, e.g. INT or KOR".into(),
        );
    }
    if options.homedir.is_empty()
        || options.homedir.len() > 64
        || !options
            .homedir
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err(
            "Home profile must contain 1–64 letters, digits, hyphens or underscores".into(),
        );
    }
    if options.rollback_input && options.performance.is_none() {
        return Err("Input rollback requires the selected performance package".into());
    }
    if !options.game_root.is_absolute() || !options.tempest_cli.is_absolute() {
        return Err("Game and installed Tempest CLI paths must be absolute".into());
    }
    argument_path(&options.game_root)?;
    argument_path(&options.tempest_cli)?;
    Ok(())
}

/// Read-only plan: folders never enable a feature; IME requires its verified receipt,
/// and performance requires an explicitly selected, verified cached package.
pub fn plan(options: &Options) -> engine::Result<Plan> {
    plan_with(options, |options, root| {
        upk_patcher::require_hash(
            &root.join(GAME),
            performance::GAME_SHA,
            "supported game executable",
        )?;
        let mut selected = Vec::new();
        if let Some(package) = ime::installed_package(root)? {
            upk_patcher::require_hash(
                &root.join(GAME),
                &package.game_fingerprint.sha256,
                "IME game executable",
            )?;
            selected.push(inside(
                root,
                &root.join(ime::TARGET).join("dlls").join(ime::DLL),
            )?);
        }
        if let Some(package) = &options.performance {
            let system = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .ok_or("Windows SystemRoot is unavailable")?
                .join("System32");
            performance::preflight(package, root, &system)?;
            for dll in performance::dlls(package, options.rollback_input)? {
                selected.push(dll.canonicalize().map_err(error)?);
            }
        }
        Ok(selected)
    })
}

fn plan_with(
    options: &Options,
    select: impl FnOnce(&Options, &Path) -> engine::Result<Vec<PathBuf>>,
) -> engine::Result<Plan> {
    validate_profile(options)?;
    let root = options.game_root.canonicalize().map_err(error)?;
    for folder in ["Binaries", "Engine", "ChaosGame"] {
        if !root.join(folder).is_dir() {
            return Err("Select the game root containing Binaries, Engine and ChaosGame".into());
        }
        inside(&root, &root.join(folder))?;
    }
    let executable = inside(&root, &root.join(GAME))?;
    if !executable.is_file() {
        return Err("The selected Win64 game executable is missing".into());
    }
    let cli = options.tempest_cli.canonicalize().map_err(error)?;
    if !cli.is_file()
        || !cli
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("tempest-cli.exe"))
    {
        return Err("Select the installed tempest-cli.exe".into());
    }
    argument_path(&cli)?;
    verify_state(&root, &options.language)?;
    let dlls = select(options, &root)?;
    let mut unique = HashSet::new();
    for dll in &dlls {
        if !dll.is_absolute() || !dll.is_file() || !unique.insert(path_key(dll)?) {
            return Err("Selected DLL paths must be existing, absolute and unique".into());
        }
    }
    let automatic = audit_registry(&root, &dlls)?;
    let registry = if root.join(REGISTRY).exists() {
        let path = inside(&root, &root.join(REGISTRY))?;
        let sha = upk_patcher::sha256_file(&path)?;
        Some((path, sha))
    } else {
        None
    };
    let explicit: Vec<_> = dlls
        .iter()
        .filter(|dll| !automatic.contains(&path_key(dll).expect("DLL paths already validated")))
        .map(|dll| argument_path(dll))
        .collect::<engine::Result<_>>()?;
    let mut args = vec![
        "launch".into(),
        argument_path(&root)?,
        "--platform".into(),
        "Win64".into(),
        "--homedir".into(),
        options.homedir.clone(),
    ];
    if !explicit.is_empty() {
        args.extend(["--dll".into(), explicit.join(",")]);
    }
    args.extend([
        "--".into(),
        "-nosteam".into(),
        "-EACALT".into(),
        "-eac-nop-loaded".into(),
        "-COOKFORDEMO".into(),
        "-seekfreeloadingpcconsole".into(),
        format!("-Language={}", options.language),
    ]);
    Ok(Plan {
        args,
        game_root: root,
        game_executable: executable,
        tempest_cli: cli,
        dlls,
        registry,
    })
}

fn enabled_default() -> bool {
    true
}

/// PascalCase and Enabled=true reflect upstream System.Text.Json / ModRecord defaults.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TempestMod {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default = "enabled_default")]
    enabled: bool,
    #[serde(default)]
    installed_files: Vec<PathBuf>,
    #[serde(default)]
    owned_files: Vec<PathBuf>,
}

fn mod_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}

fn family(value: &str) -> bool {
    let name: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "paladinscatime",
        "watchcatime",
        "imecompat",
        "paladinscatperformance",
        "paladinsperformance",
        "watchcatnativeinput",
        "watchcatworldscheduler",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

fn discover(root: &Path, dir: &Path, depth: usize, dlls: &mut Vec<PathBuf>) -> engine::Result<()> {
    if depth > 16 || dlls.len() > MAX_DISCOVERED_DLLS {
        return Err("Tempest mod DLL inventory exceeds the bounded depth/count".into());
    }
    let dir = inside(root, dir)?;
    for entry in fs::read_dir(dir).map_err(error)? {
        let path = entry.map_err(error)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(error)?;
        // Junction/symlink traversal can create cycles or escape the selected installation.
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = metadata.file_type().is_symlink();
        if reparse {
            return Err("Tempest mod DLL inventory contains a junction or symbolic link".into());
        }
        if metadata.is_dir() {
            discover(root, &path, depth + 1, dlls)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("dll"))
        {
            dlls.push(inside(root, &path)?);
            if dlls.len() > MAX_DISCOVERED_DLLS {
                return Err("Tempest mod DLL inventory exceeds the bounded count".into());
            }
        }
    }
    Ok(())
}

/// Return verified selected DLLs that Tempest will already load exactly once.
fn audit_registry(root: &Path, selected: &[PathBuf]) -> engine::Result<HashSet<String>> {
    let registry = root.join(REGISTRY);
    if !registry.exists() {
        return Ok(HashSet::new());
    }
    let mods: Vec<TempestMod> = read_json(&inside(root, &registry)?)?;
    if mods.len() > MAX_MODS {
        return Err("Tempest mod registry exceeds the count limit".into());
    }
    let selected: HashSet<_> = selected
        .iter()
        .map(|p| path_key(p))
        .collect::<engine::Result<_>>()?;
    let mut ids = HashSet::new();
    let mut automatic = HashSet::new();
    let mut discovered_count = 0;
    for record in mods {
        if !record.id.is_empty() && !ids.insert(record.id.to_lowercase()) {
            return Err("Tempest mod registry repeats an ID; resolve it before launching".into());
        }
        if !record.enabled {
            continue;
        }
        let managed = family(&record.id) || family(&record.name);
        if managed
            && (!record.kind.eq_ignore_ascii_case("V2")
                || !record.id.eq_ignore_ascii_case(ime::MOD_ID))
        {
            return Err(format!(
                "Conflicting manually registered managed mod: {}",
                record.id
            ));
        }
        for owned in record
            .installed_files
            .iter()
            .chain(record.owned_files.iter())
        {
            if owned
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(family)
            {
                let owned = if owned.is_absolute() {
                    owned.clone()
                } else {
                    root.join(owned)
                };
                let owned = inside(root, &owned)?;
                if !record.id.eq_ignore_ascii_case(ime::MOD_ID)
                    || !selected.contains(&path_key(&owned)?)
                {
                    return Err(format!(
                        "Conflicting native DLL owner in Tempest registry: {}",
                        record.id
                    ));
                }
            }
        }
        if !record.kind.eq_ignore_ascii_case("V2") {
            continue;
        }
        if !mod_id(&record.id) {
            return Err("Enabled Tempest V2 mod has an unsafe/missing ID".into());
        }
        let dir = root.join(".tempest/v2/mods").join(&record.id).join("dlls");
        if !dir.exists() {
            if managed {
                return Err("Registered IME mod has no verified selected DLL".into());
            }
            continue;
        }
        let mut dlls = Vec::new();
        discover(root, &dir, 0, &mut dlls)?;
        discovered_count += dlls.len();
        if discovered_count > MAX_DISCOVERED_DLLS {
            return Err("Tempest enabled DLL inventory exceeds count limit".into());
        }
        if managed && dlls.is_empty() {
            return Err("Registered IME mod has no verified selected DLL".into());
        }
        for dll in dlls {
            let key = path_key(&dll)?;
            let managed_dll = dll.file_name().and_then(|s| s.to_str()).is_some_and(family);
            if (managed || managed_dll)
                && (!record.id.eq_ignore_ascii_case(ime::MOD_ID) || !selected.contains(&key))
            {
                return Err(format!(
                    "Conflicting enabled native mod DLL: {}",
                    dll.display()
                ));
            }
            if !automatic.insert(key) {
                return Err("Tempest would load the same DLL more than once".into());
            }
        }
    }
    Ok(automatic)
}

#[derive(Clone, Debug)]
struct GameProcess {
    pid: u32,
    executable: Option<PathBuf>,
}

trait Dispatch {
    fn id(&self) -> u32;
    fn poll(&mut self) -> engine::Result<Option<i32>>;
}
impl Dispatch for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }
    fn poll(&mut self) -> engine::Result<Option<i32>> {
        Ok(self
            .try_wait()
            .map_err(error)?
            .map(|s| s.code().unwrap_or(-1)))
    }
}

trait Host {
    fn inventory(&self) -> engine::Result<Vec<GameProcess>>;
    fn dispatch(&self, plan: &Plan, log: &Path) -> engine::Result<Box<dyn Dispatch>>;
    fn pause(&self);
}
struct WindowsHost;
impl Host for WindowsHost {
    fn inventory(&self) -> engine::Result<Vec<GameProcess>> {
        game_processes()
    }
    fn dispatch(&self, plan: &Plan, log: &Path) -> engine::Result<Box<dyn Dispatch>> {
        let out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(log)
            .map_err(error)?;
        let err = out.try_clone().map_err(error)?;
        let mut command = Command::new(&plan.tempest_cli);
        command
            .args(&plan.args)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000200); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        }
        Ok(Box::new(command.spawn().map_err(error)?))
    }
    fn pause(&self) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ensure_idle(processes: &[GameProcess]) -> engine::Result<()> {
    if processes.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Paladins is already running (PIDs {}). This launcher leaves every existing client intact; close the intended client before Play.",
            processes
                .iter()
                .map(|p| p.pid.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn pinned_handles(plan: &Plan) -> engine::Result<Vec<File>> {
    std::iter::once(&plan.game_executable)
        .chain(std::iter::once(&plan.tempest_cli))
        .chain(plan.dlls.iter())
        .chain(plan.registry.iter().map(|(path, _)| path))
        .map(|path| {
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1); // Keep verified files read-only until startup observation.
            }
            options.open(path).map_err(error)
        })
        .collect()
}

/// Call only from an explicit Play action or an explicit CLI --launch request.
/// Observation is bounded; the caller receives pending status if startup takes longer.
pub fn start(options: &Options, log_dir: &Path) -> engine::Result<LaunchResult> {
    start_with(
        options,
        log_dir,
        &WindowsHost,
        || plan(options),
        OBSERVATION,
    )
}

fn start_with(
    options: &Options,
    log_dir: &Path,
    host: &dyn Host,
    planner: impl Fn() -> engine::Result<Plan>,
    observation: Duration,
) -> engine::Result<LaunchResult> {
    ensure_idle(&host.inventory()?)?;
    // Share the patch/install lock so another launcher or installer cannot race Play.
    let (_root, _lock) = engine::acquire(&options.game_root, &|| {
        host.inventory().map(|p| !p.is_empty()).unwrap_or(true)
    })?;
    let prepared = planner()?;
    let _payload_guards = pinned_handles(&prepared)?;
    // Revalidate hashes/profile/registry after read-only payload locks are held.
    if planner()? != prepared {
        return Err("Launch plan changed during final validation".into());
    }
    ensure_idle(&host.inventory()?)?;
    fs::create_dir_all(log_dir).map_err(error)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let log = log_dir.join(format!("launch-{stamp}.log"));
    let mut child = host.dispatch(&prepared, &log)?;
    let launcher_pid = child.id();
    let start = Instant::now();
    let executable = path_key(&prepared.game_executable)?;
    loop {
        for process in host.inventory()? {
            if process
                .executable
                .as_deref()
                .map(path_key)
                .transpose()?
                .as_ref()
                == Some(&executable)
            {
                return Ok(LaunchResult {
                    launcher_pid,
                    game_pid: Some(process.pid),
                    log,
                    status: format!(
                        "Tempest dispatched (PID {launcher_pid}); fresh selected game observed (PID {}). Mod initialization is not yet verified.",
                        process.pid
                    ),
                });
            }
        }
        if let Some(code) = child.poll()?
            && code != 0
        {
            return Err(format!(
                "Tempest dispatch exited with code {code}; no selected game was observed. See {}",
                log.display()
            ));
        }
        if start.elapsed() >= observation {
            return Ok(LaunchResult {
                launcher_pid,
                game_pid: None,
                log,
                status: format!(
                    "Tempest dispatched (PID {launcher_pid}); selected game startup is still unconfirmed. Mod initialization is not yet verified."
                ),
            });
        }
        host.pause();
    }
}

#[cfg(not(windows))]
fn game_processes() -> engine::Result<Vec<GameProcess>> {
    Err("Game launch is supported on Windows only".into())
}

#[cfg(windows)]
fn game_processes() -> engine::Result<Vec<GameProcess>> {
    use std::ffi::c_void;
    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        name: [u16; 260],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            name: *mut u16,
            length: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let raw = unsafe { CreateToolhelp32Snapshot(2, 0) };
    if raw.is_null() || raw as isize == -1 {
        return Err(error(std::io::Error::last_os_error()));
    }
    let snapshot = Handle(raw);
    let mut entry: Entry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<Entry>() as u32;
    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    if more == 0 {
        return Err(error(std::io::Error::last_os_error()));
    }
    let mut games = Vec::new();
    while more != 0 {
        let end = entry
            .name
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.name.len());
        if String::from_utf16_lossy(&entry.name[..end]).eq_ignore_ascii_case("Paladins.exe") {
            let process = unsafe { OpenProcess(0x1000, 0, entry.pid) };
            let executable = if process.is_null() {
                None
            } else {
                let process = Handle(process);
                let mut buffer = vec![0u16; 32768];
                let mut length = buffer.len() as u32;
                if unsafe {
                    QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length)
                } == 0
                {
                    None
                } else {
                    String::from_utf16(&buffer[..length as usize])
                        .ok()
                        .map(PathBuf::from)
                }
            };
            games.push(GameProcess {
                pid: entry.pid,
                executable,
            });
        }
        more = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let last = std::io::Error::last_os_error();
    if last.raw_os_error() != Some(18) {
        return Err(error(last));
    } // ERROR_NO_MORE_FILES
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SERIAL: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        home: PathBuf,
        options: Options,
    }
    impl Fixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let home = std::env::temp_dir().join(format!(
                "paladinscat-launch-fixture-{}-{stamp}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let root = home.join("game");
            for dir in ["Binaries/Win64", "ChaosGame", "Engine"] {
                fs::create_dir_all(root.join(dir)).unwrap();
            }
            fs::write(root.join(GAME), b"fixture-game").unwrap();
            let cli = home.join("tempest-cli.exe");
            fs::write(&cli, b"not-an-executable").unwrap();
            Self {
                home,
                options: Options {
                    game_root: root,
                    tempest_cli: cli,
                    language: "KOR".into(),
                    homedir: "paladinslive".into(),
                    performance: None,
                    rollback_input: false,
                },
            }
        }
        fn plan(&self, selected: &[PathBuf]) -> engine::Result<Plan> {
            plan_with(&self.options, |_, _| Ok(selected.to_vec()))
        }
        fn dll(&self, relative: &str) -> PathBuf {
            let path = self.options.game_root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"fixture-dll").unwrap();
            path.canonicalize().unwrap()
        }
        fn registry(&self, value: serde_json::Value) -> PathBuf {
            let path = self.options.game_root.join(REGISTRY);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            path
        }
        fn receipt_ime(&self) -> PathBuf {
            let source = self.home.join("source-ime.dll");
            fs::write(&source, ime_image()).unwrap();
            let package = ime::prepare(&source, &self.home.join("prepared-ime"), "0.1.0").unwrap();
            let dir = self.options.game_root.join(ime::TARGET);
            fs::create_dir_all(dir.join("dlls")).unwrap();
            fs::write(dir.join("manifest.toml"), "[mod]\nid = \"PaladinsCat-IME\"\nname = \"PaladinsCat IME Fix\"\nversion = \"0.1.0\"\n\n[[mod.authors]]\nname = \"PaladinsCat\"\n").unwrap();
            let dll = dir.join("dlls").join(ime::DLL);
            fs::copy(source, &dll).unwrap();
            let receipt = self
                .options
                .game_root
                .join(".watchcat-localization/ime-active.json");
            fs::create_dir_all(receipt.parent().unwrap()).unwrap();
            fs::write(receipt, serde_json::to_vec(&package).unwrap()).unwrap();
            assert_eq!(
                ime::installed_package(&self.options.game_root).unwrap(),
                Some(package)
            );
            dll.canonicalize().unwrap()
        }
        fn locale_receipt(&self) -> PathBuf {
            let target = self.dll("ChaosGame/fixture.dat");
            let backup = self.dll(".watchcat-localization/backups/fixture.dat");
            let sha = format!("{:x}", Sha256::digest(b"fixture-dll"));
            let receipt = self
                .options
                .game_root
                .join(".watchcat-localization/active.json");
            fs::write(&receipt, serde_json::to_vec(&serde_json::json!({
                "version":1,"locale":"KOR","game_root":self.options.game_root.canonicalize().unwrap(),"applied":true,
                "files":[{"target":target,"backup":backup,"stock_sha256":sha,"artifact_sha256":sha}]
            })).unwrap()).unwrap();
            receipt
        }
    }

    // Deliberately non-executable PE fixture: tests inspect the loader contract only.
    fn ime_image() -> Vec<u8> {
        let mut b = vec![0u8; 2048];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        b[64..68].copy_from_slice(b"PE\0\0");
        b[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        b[70..72].copy_from_slice(&1u16.to_le_bytes());
        b[84..86].copy_from_slice(&240u16.to_le_bytes());
        b[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        b[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        let set = |b: &mut [u8], at: usize, value: u32| {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes())
        };
        for (at, value) in [
            (200, 4096),
            (204, 1024),
            (340, 4096),
            (344, 1536),
            (348, 512),
            (364, 0xe0000040),
            (532, 9),
            (536, 9),
            (540, 4184),
            (544, 4224),
            (548, 4264),
        ] {
            set(&mut b, at, value);
        }
        let names = [
            "WatchCatImeStatus",
            "WatchCatImeState",
            "WatchCatImeFlags",
            "WatchCatImeDisable",
            "WatchCatImePointers",
            "WatchCatImeAbi",
            "WatchCatImeWindow",
            "WatchCatImeWindowError",
            "WatchCatImeCounters",
        ];
        let mut at = 720;
        for (i, name) in names.iter().enumerate() {
            set(&mut b, 600 + i * 4, if i == 5 { 5120 } else { 5504 });
            set(&mut b, 640 + i * 4, 4096 + (at - 512) as u32);
            b[680 + i * 2..682 + i * 2].copy_from_slice(&(i as u16).to_le_bytes());
            b[at..at + name.len()].copy_from_slice(name.as_bytes());
            at += name.len() + 1;
        }
        for (i, value) in [4u64, 9, 7, 1].iter().enumerate() {
            b[1536 + i * 8..1544 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        b
    }

    #[test]
    fn plan_preserves_profile_and_places_multiple_dlls_before_passthrough() {
        let mut f = Fixture::new();
        f.options.language = "FRA".into();
        f.options.homedir = "community-profile_2".into();
        let first = f.dll("cache/input.dll");
        let second = f.dll("cache/world.dll");
        let p = f.plan(&[first.clone(), second.clone()]).unwrap();
        let dll = p.args.iter().position(|a| a == "--dll").unwrap();
        let separator = p.args.iter().position(|a| a == "--").unwrap();
        assert!(dll + 1 < separator);
        assert_eq!(
            p.args[dll + 1],
            format!("{},{}", first.display(), second.display())
        );
        assert!(p.args.contains(&"community-profile_2".into()));
        assert_eq!(p.args.last().unwrap(), "-Language=FRA");
        assert_eq!(p.tempest_cli, f.options.tempest_cli.canonicalize().unwrap());
        assert_eq!(
            p.game_executable,
            f.options.game_root.join(GAME).canonicalize().unwrap()
        );
        assert!(!f.options.game_root.join(".watchcat-localization").exists());
    }

    #[test]
    fn profile_injection_and_ambiguous_dll_paths_are_rejected() {
        let mut f = Fixture::new();
        for locale in ["kor", "KOR -windowed", "INT\n", "KOR,"] {
            f.options.language = locale.into();
            assert!(f.plan(&[]).is_err());
        }
        f.options.language = "KOR".into();
        for home in ["", "../other", "-homedir=x", "safe\r"] {
            f.options.homedir = home.into();
            assert!(f.plan(&[]).is_err());
        }
        f.options.homedir = "paladinslive".into();
        let comma = f.dll("cache/comma,name.dll");
        assert!(f.plan(&[comma]).is_err());
        let unique = f.dll("cache/unique.dll");
        assert!(f.plan(&[unique.clone(), unique]).is_err());
        f.options.rollback_input = true;
        assert!(f.plan(&[]).is_err());
    }

    #[test]
    fn verified_unregistered_ime_uses_explicit_load_once() {
        let f = Fixture::new();
        let dll = f.receipt_ime();
        let p = f.plan(std::slice::from_ref(&dll)).unwrap();
        assert_eq!(p.dlls.as_slice(), std::slice::from_ref(&dll));
        let i = p.args.iter().position(|a| a == "--dll").unwrap();
        assert_eq!(p.args[i + 1], dll.to_str().unwrap());
        assert!(!f.options.game_root.join(REGISTRY).exists());
    }

    #[test]
    fn verified_registered_ime_omits_explicit_duplicate_and_preserves_registry() {
        let f = Fixture::new();
        let dll = f.receipt_ime();
        let registry = f.registry(serde_json::json!([{"Id":"PaladinsCat-IME","Name":"PaladinsCat IME Fix","Kind":"V2","Enabled":true}]));
        let original = fs::read(&registry).unwrap();
        let p = f.plan(std::slice::from_ref(&dll)).unwrap();
        assert_eq!(p.dlls, [dll]);
        assert!(!p.args.contains(&"--dll".into()));
        assert_eq!(fs::read(registry).unwrap(), original);
    }

    #[test]
    fn plain_mod_folders_and_prepared_cache_do_not_enable_features() {
        let f = Fixture::new();
        f.dll(".tempest/v2/mods/Unregistered/dlls/unregistered.dll");
        f.dll("cache/PaladinsCatImeFix.dll");
        let p = f.plan(&[]).unwrap();
        assert!(p.dlls.is_empty());
        assert!(!p.args.contains(&"--dll".into()));
        assert_eq!(ime::installed_package(&f.options.game_root).unwrap(), None);
        f.dll(&format!("{}/dlls/{}", ime::TARGET, ime::DLL));
        assert!(
            ime::installed_package(&f.options.game_root)
                .unwrap_err()
                .contains("without a recognized receipt")
        );
    }

    #[test]
    fn unselected_registered_native_family_and_duplicate_ids_are_refused() {
        let f = Fixture::new();
        f.dll(&format!("{}/dlls/{}", ime::TARGET, ime::DLL));
        f.registry(serde_json::json!([{"Id":"PaladinsCat-IME","Kind":"V2","Enabled":true}]));
        assert!(f.plan(&[]).unwrap_err().contains("Conflicting"));
        f.registry(serde_json::json!([{"Id":"Other","Kind":"V2","Enabled":true},{"Id":"oTHER","Kind":"V2","Enabled":false}]));
        assert!(f.plan(&[]).unwrap_err().contains("repeats an ID"));
        f.registry(serde_json::json!([{"Id":"paladins-performance","Kind":"V2","Enabled":true}]));
        assert!(
            f.plan(&[])
                .unwrap_err()
                .contains("Conflicting manually registered")
        );
    }

    #[test]
    fn enabled_manual_native_owner_is_refused_even_under_an_unrelated_id() {
        let f = Fixture::new();
        f.dll(".tempest/v2/mods/Other/dlls/watchcat-native-input-compact-modifier.dll");
        f.registry(serde_json::json!([{"Id":"Other","Kind":"V2","Enabled":true}]));
        assert!(
            f.plan(&[])
                .unwrap_err()
                .contains("Conflicting enabled native")
        );
        f.registry(serde_json::json!([{"Id":"Other","Kind":"V2","Enabled":false}]));
        assert!(f.plan(&[]).is_ok());
        f.registry(serde_json::json!([{"Id":"OldInput","Kind":"V1","Enabled":true,"OwnedFiles":[".tempest/v2/mods/Other/dlls/watchcat-native-input-compact-modifier.dll"]}]));
        assert!(
            f.plan(&[])
                .unwrap_err()
                .contains("Conflicting native DLL owner")
        );
    }

    #[test]
    fn enabled_unrelated_mods_are_preserved_and_unsafe_metadata_is_refused() {
        let f = Fixture::new();
        f.dll(".tempest/v2/mods/Independent/dlls/independent.dll");
        let registry =
            f.registry(serde_json::json!([{"Id":"Independent","Kind":"V2","Enabled":true}]));
        let original = fs::read(&registry).unwrap();
        assert!(f.plan(&[]).is_ok());
        assert_eq!(fs::read(&registry).unwrap(), original);
        fs::write(&registry, b"broken-json").unwrap();
        assert!(f.plan(&[]).is_err());
        f.registry(serde_json::json!([{"Id":"../escape","Kind":"V2","Enabled":true}]));
        assert!(f.plan(&[]).unwrap_err().contains("unsafe/missing ID"));
    }

    #[test]
    fn incomplete_locale_receipt_and_pending_journals_block_play() {
        let f = Fixture::new();
        let receipt = f.locale_receipt();
        assert!(f.plan(&[]).is_ok());
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        value["applied"] = false.into();
        fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(f.plan(&[]).unwrap_err().contains("requires recovery"));
        value["applied"] = true.into();
        fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
        fs::write(
            f.options.game_root.join("ChaosGame/fixture.dat"),
            b"unexpected-change",
        )
        .unwrap();
        assert!(f.plan(&[]).unwrap_err().contains("hash mismatch"));
        fs::write(
            f.options
                .game_root
                .join(".watchcat-localization/ime-journal.json"),
            b"{}",
        )
        .unwrap();
        assert!(f.plan(&[]).unwrap_err().contains("Recover the interrupted"));
    }

    #[test]
    fn actual_plan_rejects_unsupported_game_and_does_not_spawn_or_write() {
        let f = Fixture::new();
        assert!(
            plan(&f.options)
                .unwrap_err()
                .contains("supported game executable hash mismatch")
        );
        assert!(!f.options.game_root.join(".watchcat-localization").exists());
        assert!(!f.home.join("logs").exists());
    }

    struct FakeChild {
        code: Option<i32>,
    }
    impl Dispatch for FakeChild {
        fn id(&self) -> u32 {
            77
        }
        fn poll(&mut self) -> engine::Result<Option<i32>> {
            Ok(self.code)
        }
    }
    struct FakeHost {
        snapshots: RefCell<VecDeque<Vec<GameProcess>>>,
        dispatched: Cell<usize>,
        code: Option<i32>,
        inventory_error: bool,
    }
    impl Host for FakeHost {
        fn inventory(&self) -> engine::Result<Vec<GameProcess>> {
            if self.inventory_error {
                return Err("inventory unavailable".into());
            }
            let mut snapshots = self.snapshots.borrow_mut();
            if snapshots.len() > 1 {
                Ok(snapshots.pop_front().unwrap())
            } else {
                Ok(snapshots.front().cloned().unwrap_or_default())
            }
        }
        fn dispatch(&self, _: &Plan, _: &Path) -> engine::Result<Box<dyn Dispatch>> {
            self.dispatched.set(self.dispatched.get() + 1);
            Ok(Box::new(FakeChild { code: self.code }))
        }
        fn pause(&self) {}
    }
    fn fake(snapshots: Vec<Vec<GameProcess>>) -> FakeHost {
        FakeHost {
            snapshots: RefCell::new(snapshots.into()),
            dispatched: Cell::new(0),
            code: None,
            inventory_error: false,
        }
    }

    #[test]
    fn running_or_unqueryable_client_refuses_dispatch_without_killing_it() {
        let f = Fixture::new();
        let host = fake(vec![vec![GameProcess {
            pid: 42,
            executable: None,
        }]]);
        let error = start_with(
            &f.options,
            &f.home.join("logs"),
            &host,
            || f.plan(&[]),
            Duration::ZERO,
        )
        .unwrap_err();
        assert!(error.contains("42"));
        assert_eq!(host.dispatched.get(), 0);
        assert!(!f.options.game_root.join(".watchcat-localization").exists());
        let mut host = fake(vec![vec![]]);
        host.inventory_error = true;
        assert!(
            start_with(
                &f.options,
                &f.home.join("logs"),
                &host,
                || f.plan(&[]),
                Duration::ZERO
            )
            .is_err()
        );
        assert_eq!(host.dispatched.get(), 0);
    }

    #[test]
    fn client_appearing_during_preflight_refuses_dispatch() {
        let f = Fixture::new();
        let host = fake(vec![
            vec![],
            vec![],
            vec![GameProcess {
                pid: 43,
                executable: None,
            }],
        ]);
        assert!(
            start_with(
                &f.options,
                &f.home.join("logs"),
                &host,
                || f.plan(&[]),
                Duration::ZERO
            )
            .is_err()
        );
        assert_eq!(host.dispatched.get(), 0);
    }

    #[test]
    fn dispatch_observation_is_bounded_and_never_claims_hooks_active() {
        let f = Fixture::new();
        let host = fake(vec![vec![]]);
        let r = start_with(
            &f.options,
            &f.home.join("logs"),
            &host,
            || f.plan(&[]),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(r.launcher_pid, 77);
        assert_eq!(r.game_pid, None);
        assert!(r.status.contains("startup is still unconfirmed"));
        assert!(r.status.contains("not yet verified"));
        assert_eq!(host.dispatched.get(), 1);
        let host = fake(vec![
            vec![],
            vec![],
            vec![],
            vec![GameProcess {
                pid: 88,
                executable: Some(f.options.game_root.join(GAME)),
            }],
        ]);
        let r = start_with(
            &f.options,
            &f.home.join("logs"),
            &host,
            || f.plan(&[]),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(r.game_pid, Some(88));
        assert!(r.status.contains("not yet verified"));
    }

    #[test]
    fn changed_final_plan_and_nonzero_dispatch_exit_are_distinct_failures() {
        let f = Fixture::new();
        let host = fake(vec![vec![]]);
        let calls = Cell::new(0);
        let result = start_with(
            &f.options,
            &f.home.join("logs"),
            &host,
            || {
                let mut p = f.plan(&[])?;
                calls.set(calls.get() + 1);
                if calls.get() > 1 {
                    p.args.push("unexpected".into());
                }
                Ok(p)
            },
            Duration::ZERO,
        );
        assert!(
            result
                .unwrap_err()
                .contains("changed during final validation")
        );
        assert_eq!(host.dispatched.get(), 0);
        let mut host = fake(vec![vec![]]);
        host.code = Some(7);
        assert!(
            start_with(
                &f.options,
                &f.home.join("logs"),
                &host,
                || f.plan(&[]),
                Duration::ZERO
            )
            .unwrap_err()
            .contains("exited with code 7")
        );
    }
}
