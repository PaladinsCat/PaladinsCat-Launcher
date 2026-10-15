use super::*;
use std::{
    ffi::OsString,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        process::CommandExt,
    },
    process::{Child, Command},
    thread,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, ReplaceFileW},
    System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        QueryFullProcessImageNameW, WaitForSingleObject,
    },
};

fn identity(file: &File) -> Result<[u32; 3]> {
    use std::os::windows::io::AsRawHandle;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok([
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ])
}
pub(super) fn file_identity(path: &Path) -> Result<[u32; 3]> {
    plain(path)?;
    identity(&File::open(path).map_err(|e| e.to_string())?)
}
pub(super) fn create_owned(journal: &mut Journal, name: &str) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    if !matches!(name, STAGE | HELPER) {
        return Err("Unknown updater staging name".into());
    }
    let path = journal.root().join(name);
    plain(journal.root())?;
    let file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| format!("Cannot stage launcher update: {e}"))?;
    journal.owned_files.insert(name.into(), identity(&file)?);
    journal.save()?;
    Ok(file)
}

pub(super) struct Process(HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
impl Process {
    pub(super) fn open(pid: u32) -> Result<Self> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "Cannot open updater process {pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }
    pub(super) fn created(&self) -> Result<u64> {
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exit = created;
        let mut kernel = created;
        let mut user = created;
        if unsafe { GetProcessTimes(self.0, &mut created, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    fn image(&self) -> Result<PathBuf> {
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        if unsafe { QueryFullProcessImageNameW(self.0, 0, buffer.as_mut_ptr(), &mut size) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        PathBuf::from(OsString::from_wide(&buffer[..size as usize]))
            .canonicalize()
            .map_err(|e| e.to_string())
    }
    pub(super) fn same_live(pid: u32, created: u64) -> Result<bool> {
        match Self::open(pid) {
            Ok(process) => Ok(process.created()? == created
                && unsafe { WaitForSingleObject(process.0, 0) } == WAIT_TIMEOUT),
            Err(_) => {
                // Only INVALID_PARAMETER proves the PID is absent. Access denied
                // is not permission to assume an updater has exited.
                if std::io::Error::last_os_error().raw_os_error() == Some(87) {
                    Ok(false)
                } else {
                    Err(format!("Cannot establish updater process ownership: {pid}"))
                }
            }
        }
    }
    pub(super) fn wait(&self, duration: Duration) -> Result<()> {
        match unsafe {
            WaitForSingleObject(self.0, duration.as_millis().min(u32::MAX as u128) as u32)
        } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err("Launcher updater process has not exited".into()),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
}

pub(super) fn arm(journal: &mut Journal) -> Result<()> {
    let root = journal.root().to_path_buf();
    let mut child = Command::new(root.join(HELPER))
        .arg("--apply-launcher-update")
        .current_dir(&root)
        .creation_flags(0x08000200)
        .spawn()
        .map_err(|e| e.to_string())?;
    journal.helper = child.id();
    journal.helper_created = Process::open(child.id())?.created()?;
    journal.save()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("Launcher updater exited before it was ready".into());
        }
        if root.join(ARMED).exists() {
            let armed: (u32, u64) = serde_json::from_slice(&read(&root.join(ARMED), 128)?)
                .map_err(|e| e.to_string())?;
            if armed == (journal.helper, journal.helper_created) {
                return Ok(());
            }
            return Err("Launcher updater readiness identity mismatch".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    // This is our own exact Child handle. The original GUI is still alive and
    // no replacement is permitted until that parent handle signals exit.
    let _ = child.kill();
    let _ = child.wait();
    Err("Launcher updater did not become ready; current launcher was preserved".into())
}

pub(super) fn replace(target: &Path, replacement: &Path, backup: Option<&Path>) -> Result<()> {
    plain(target)?;
    plain(replacement)?;
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let target_wide = wide(target);
    let replacement_wide = wide(replacement);
    let backup_wide = backup.map(wide);
    if unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide
                .as_ref()
                .map_or(std::ptr::null(), |v| v.as_ptr()),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(format!(
            "Cannot replace launcher EXE: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn launch(target: &Path) -> Result<Child> {
    Command::new(target)
        .current_dir(target.parent().ok_or("Launcher directory absent")?)
        .creation_flags(0x08000200)
        .spawn()
        .map_err(|e| format!("Cannot restart launcher: {e}"))
}

fn wait_ready(journal: &Journal, child: &mut Child, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("Updated launcher exited during startup".into());
        }
        let path = journal.root().join(READY);
        if path.exists() {
            let ready: (u32, u64, String) =
                serde_json::from_slice(&read(&path, 256)?).map_err(|e| e.to_string())?;
            let process = Process::open(child.id())?;
            if ready.0 != child.id()
                || ready.1 != process.created()?
                || ready.2 != journal.release.sha256
                || process.image()? != journal.target
            {
                return Err("Updated launcher readiness identity mismatch".into());
            }
            matches(&journal.target, &journal.release.sha256)?;
            // Catch immediate post-frame exits before committing the backup.
            thread::sleep(Duration::from_millis(500));
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Ok(());
            }
            return Err("Updated launcher exited immediately after startup".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("Updated launcher did not confirm a working renderer and first UI frame".into())
}

fn rollback(journal: &mut Journal, error: String) -> Result<()> {
    let root = journal.root().to_path_buf();
    matches(&root.join(BACKUP), &journal.old_sha256)?;
    // If ReplaceFileW reported a partial failure, inspect the target instead of
    // deleting or overwriting an unexplained image. Preserve all recovery data.
    if journal.target.exists() {
        matches(&journal.target, &journal.release.sha256)?;
        replace(&journal.target, &root.join(BACKUP), None)?;
    } else {
        fs::rename(root.join(BACKUP), &journal.target).map_err(|e| e.to_string())?;
    }
    matches(&journal.target, &journal.old_sha256)?;
    journal.phase = Phase::RolledBack;
    journal.error = Some(error);
    journal.save()?;
    launch(&journal.target)?;
    Ok(())
}

pub(super) fn apply() -> Result<()> {
    let image = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if image.file_name().and_then(|n| n.to_str()) != Some(HELPER) {
        return Err("Updater must run from its owned helper filename".into());
    }
    let root = image.parent().ok_or("Updater directory absent")?;
    let started = Instant::now();
    let mut journal = loop {
        let journal = Journal::load(root)?;
        if journal.helper == std::process::id()
            && journal.helper_created == Process::open(std::process::id())?.created()?
        {
            break journal;
        }
        if started.elapsed() > Duration::from_secs(5) {
            return Err("Updater identity was not recorded by its parent".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    if journal.phase != Phase::Prepared || root.join(BACKUP).exists() {
        return Err("Updater transaction is not prepared".into());
    }
    matches(&image, &journal.old_sha256)?;
    matches(&journal.target, &journal.old_sha256)?;
    verify_payload(&root.join(STAGE), &journal.release)?;
    let parent = Process::open(journal.parent)?;
    if parent.created()? != journal.parent_created || parent.image()? != journal.target {
        return Err("Updater parent process identity changed".into());
    }
    write_new(
        &root.join(ARMED),
        &serde_json::to_vec(&(std::process::id(), journal.helper_created))
            .map_err(|e| e.to_string())?,
    )?;
    parent.wait(Duration::from_secs(60))?;
    if let Err(error) = complete(&mut journal) {
        journal.error = Some(error.clone());
        let _ = journal.save();
        if journal.phase == Phase::Committed {
            return Err(error);
        }
        if matches(&journal.target, &journal.old_sha256).is_ok() {
            journal.phase = Phase::RolledBack;
            journal.save()?;
            launch(&journal.target)?;
            return Ok(());
        }
        return rollback(&mut journal, error);
    }
    Ok(())
}

fn complete(journal: &mut Journal) -> Result<()> {
    let root = journal.root().to_path_buf();
    // No image is replaced while the original parent is alive. Recheck both
    // immutable pins after waiting and before invoking ReplaceFileW.
    matches(&journal.target, &journal.old_sha256)?;
    verify_payload(&root.join(STAGE), &journal.release)?;
    journal.phase = Phase::Replacing;
    journal.save()?;
    if let Err(error) = replace(&journal.target, &root.join(STAGE), Some(&root.join(BACKUP))) {
        if matches(&journal.target, &journal.old_sha256).is_ok() {
            journal.phase = Phase::RolledBack;
            journal.error = Some(error);
            journal.save()?;
            launch(&journal.target)?;
            return Ok(());
        }
        return rollback(journal, error);
    }
    matches(&root.join(BACKUP), &journal.old_sha256)?;
    journal.phase = Phase::Starting;
    journal.save()?;
    let mut child = match launch(&journal.target) {
        Ok(child) => child,
        Err(error) => return rollback(journal, error),
    };
    if let Err(error) = wait_ready(journal, &mut child, Duration::from_secs(30)) {
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            child.kill().map_err(|e| e.to_string())?;
        }
        child.wait().map_err(|e| e.to_string())?;
        return rollback(journal, error);
    }
    journal.phase = Phase::Committed;
    journal.save()
}

pub(super) fn ready() -> Result<()> {
    let target = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = target.parent().ok_or("Launcher directory absent")?;
    if !root.join(JOURNAL).exists() {
        return Ok(());
    }
    let mut journal = Journal::load(root)?;
    if !matches!(journal.phase, Phase::Replacing | Phase::Starting) {
        return Ok(());
    }
    matches(&target, &journal.release.sha256)?;
    if !Process::same_live(journal.helper, journal.helper_created)? {
        // A completed first frame after power loss establishes current startup;
        // the pinned old backup is retained until this commit is persisted.
        matches(&root.join(BACKUP), &journal.old_sha256)?;
        journal.phase = Phase::Committed;
        return journal.save();
    }
    write_new(
        &root.join(READY),
        &serde_json::to_vec(&(
            std::process::id(),
            Process::open(std::process::id())?.created()?,
            journal.release.sha256,
        ))
        .map_err(|e| e.to_string())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_identity_is_bound_to_creation_time_and_image() {
        let process = Process::open(std::process::id()).unwrap();
        let created = process.created().unwrap();
        assert!(Process::same_live(std::process::id(), created).unwrap());
        assert!(!Process::same_live(std::process::id(), created + 1).unwrap());
        assert_eq!(
            process.image().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert!(!Process::same_live(u32::MAX, 0).unwrap());
    }
    #[test]
    fn interrupted_partial_file_is_recovered_by_identity_and_a_collision_is_preserved() {
        use crate::self_update::tests::{directory, image, release};
        let root = directory();
        let old = image([0, 3, 8]);
        fs::write(root.join(EXE), &old).unwrap();
        let mut journal = Journal {
            schema: 1,
            parent: u32::MAX,
            parent_created: 0,
            helper: 0,
            helper_created: 0,
            target: root.join(EXE),
            old_sha256: sha(&old),
            release: release(),
            phase: Phase::Prepared,
            error: None,
            staged_sha256: None,
            owned_files: Default::default(),
        };
        journal.save().unwrap();
        let mut stage = create_owned(&mut journal, STAGE).unwrap();
        stage.write_all(b"partial download").unwrap();
        drop(stage);
        cleanup(&root.join(EXE)).unwrap();
        assert!(!root.join(STAGE).exists());
        assert!(!root.join(JOURNAL).exists());
        fs::write(root.join(STAGE), b"unrelated collision").unwrap();
        journal.owned_files.clear();
        journal.save().unwrap();
        assert!(create_owned(&mut journal, STAGE).is_err());
        assert!(cleanup(&root.join(EXE)).is_err());
        assert_eq!(fs::read(root.join(STAGE)).unwrap(), b"unrelated collision");
    }
}
