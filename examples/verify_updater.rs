//! CPU-only Windows lifecycle check using owned Rust EXEs, never the live GUI.
#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use paladinscat_launcher::self_update;
    use std::{
        fs,
        os::windows::process::CommandExt,
        path::PathBuf,
        process::Command,
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    let fixture = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Pass target/debug/examples/updater_fixture.exe")?,
    );
    let old = fs::read(fixture).map_err(|e| e.to_string())?;
    self_update::validate_executable(&old, env!("CARGO_PKG_VERSION"))?;
    let mut new = old.clone();
    let parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|p| p.parse::<u16>().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let next_patch = parts[2]
        .checked_add(1)
        .ok_or("Fixture patch component exhausted")?;
    let next_version = format!("{}.{}.{}", parts[0], parts[1], next_patch);
    let version_ms = (u32::from(parts[0]) << 16) | u32::from(parts[1]);
    let version_ls = u32::from(parts[2]) << 16;
    let mut changed = 0;
    for offset in 0..new.len().saturating_sub(24) {
        if new[offset..offset + 4] == [0xbd, 0x04, 0xef, 0xfe]
            && new[offset + 8..offset + 12] == version_ms.to_le_bytes()
            && new[offset + 12..offset + 16] == version_ls.to_le_bytes()
        {
            new[offset + 12..offset + 16]
                .copy_from_slice(&(u32::from(next_patch) << 16).to_le_bytes());
            new[offset + 20..offset + 24]
                .copy_from_slice(&(u32::from(next_patch) << 16).to_le_bytes());
            changed += 1;
        }
    }
    if changed != 1 {
        return Err("Expected exactly one owned fixture VERSIONINFO resource".into());
    }
    self_update::validate_executable(&new, &next_version)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/updater-lifecycle")
        .join(stamp.to_string());
    let mut results = Vec::new();
    for scenario in ["success", "fail", "timeout", "locked"] {
        let dir = root.join(scenario);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        let target = dir.join(self_update::EXE);
        fs::write(&target, &old).map_err(|e| e.to_string())?;
        fs::write(dir.join("paladinscat-launcher.update.exe"), &new).map_err(|e| e.to_string())?;
        fs::write(dir.join("owned-updater-fixture"), scenario).map_err(|e| e.to_string())?;
        fs::write(dir.join("settings.json"), b"saved preferences").map_err(|e| e.to_string())?;
        // Keep an owned read handle that denies delete sharing. This exercises
        // Windows replacement failure without touching permissions or live apps.
        use std::os::windows::fs::OpenOptionsExt;
        let held_image = if scenario == "locked" {
            Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
                    .open(&target)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let status = Command::new(&target)
            .arg("--prepare-fixture")
            .creation_flags(0x08000200)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("{scenario}: fixture parent failed"));
        }
        let marker = dir.join(if scenario == "success" {
            "new-ready"
        } else {
            "old-restored"
        });
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(45) {
            if marker.exists() && !dir.join("paladinscat-launcher.update.json").exists() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if !marker.exists() {
            return Err(format!("{scenario}: no expected startup/recovery marker"));
        }
        if dir.join("paladinscat-launcher.update.json").exists() {
            return Err(format!("{scenario}: transaction cleanup did not finish"));
        }
        let actual = fs::read(&target).map_err(|e| e.to_string())?;
        if actual != if scenario == "success" { &new } else { &old }.as_slice() {
            return Err(format!("{scenario}: wrong final EXE"));
        }
        if fs::read(dir.join("settings.json")).map_err(|e| e.to_string())? != b"saved preferences" {
            return Err("Settings changed".into());
        }
        for name in [
            "paladinscat-launcher.previous.exe",
            "paladinscat-launcher.update.exe",
            "paladinscat-launcher.updater.exe",
            "paladinscat-launcher.update-armed.json",
            "paladinscat-launcher.update-ready.json",
            "paladinscat-launcher.update-journal.tmp",
        ] {
            if dir.join(name).exists() {
                return Err(format!("{scenario}: stale updater file {name}"));
            }
        }
        results.push(serde_json::json!({"scenario":scenario,"passed":true,"settings_preserved":true,"stale_files":false,"seconds":start.elapsed().as_secs_f32()}));
        drop(held_image);
        println!("{scenario}: passed");
    }
    paladinscat_launcher::engine::save_settings(
        &root.join("result.json"),
        &serde_json::json!({"passed":true,"cpu_only":true,"live_gui_changed":false,"game_launched":false,"results":results}),
    )?;
    println!("{}", root.join("result.json").display());
    Ok(())
}
