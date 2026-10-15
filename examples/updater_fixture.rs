//! Owned CPU-only EXE used to exercise real Windows updater process handoff.
//! Never a game payload or a public launcher release.
#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(3);
    }
}
#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use paladinscat_launcher::{engine, self_update};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        os::windows::process::CommandExt,
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    fn created(pid: u32) -> Result<u64, String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut a: FILETIME = std::mem::zeroed();
            let mut b = a;
            let mut c = a;
            let mut d = a;
            let ok = GetProcessTimes(handle, &mut a, &mut b, &mut c, &mut d);
            CloseHandle(handle);
            if ok == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok((u64::from(a.dwHighDateTime) << 32) | u64::from(a.dwLowDateTime))
        }
    }
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(result) = self_update::helper_mode(&args) {
        return result;
    }
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = exe.parent().ok_or("Fixture directory absent")?;
    if !root.join("owned-updater-fixture").is_file() {
        return Err("Owned fixture marker required".into());
    }
    let journal_path = root.join("paladinscat-launcher.update.json");
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    if args.get(1).map(String::as_str) == Some("--prepare-fixture") {
        let old = fs::read(&exe).map_err(|e| e.to_string())?;
        let new =
            fs::read(root.join("paladinscat-launcher.update.exe")).map_err(|e| e.to_string())?;
        let parts = env!("CARGO_PKG_VERSION")
            .split('.')
            .map(|p| p.parse::<u16>().map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let next_patch = parts[2]
            .checked_add(1)
            .ok_or("Fixture patch component exhausted")?;
        let next_version = format!("{}.{}.{}", parts[0], parts[1], next_patch);
        self_update::validate_executable(&new, &next_version)?;
        fs::write(root.join("paladinscat-launcher.updater.exe"), &old)
            .map_err(|e| e.to_string())?;
        let mut journal = serde_json::json!({"schema":1,"parent":std::process::id(),"parent_created":created(std::process::id())?,"helper":0,"helper_created":0,"target":exe,"old_sha256":hash(&old),"release":{"version":next_version,"bytes":new.len(),"sha256":hash(&new)},"phase":"Prepared","error":null,"staged_sha256":hash(&new)});
        engine::save_settings(&journal_path, &journal)?;
        let mut child = Command::new(root.join("paladinscat-launcher.updater.exe"))
            .arg("--apply-launcher-update")
            .creation_flags(0x08000200)
            .spawn()
            .map_err(|e| e.to_string())?;
        journal["helper"] = child.id().into();
        journal["helper_created"] = created(child.id())?.into();
        engine::save_settings(&journal_path, &journal)?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("Fixture helper exited before arming".into());
            }
            if root.join("paladinscat-launcher.update-armed.json").exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        return Err("Fixture helper did not arm".into());
    }
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if journal["phase"] == "RolledBack" {
        fs::write(root.join("old-restored"), b"restored").map_err(|e| e.to_string())?;
    } else if journal["release"]["sha256"] == hash(&fs::read(&exe).map_err(|e| e.to_string())?) {
        let scenario =
            fs::read_to_string(root.join("owned-updater-fixture")).map_err(|e| e.to_string())?;
        if scenario == "fail" {
            return Err("Owned new EXE startup failure".into());
        }
        if scenario == "timeout" {
            thread::sleep(Duration::from_secs(40));
            return Ok(());
        }
        self_update::mark_ready()?;
        fs::write(root.join("new-ready"), b"ready").map_err(|e| e.to_string())?;
    } else {
        return Err("Unexpected fixture image".into());
    }
    thread::sleep(Duration::from_secs(2));
    self_update::startup_cleanup()?;
    Ok(())
}
