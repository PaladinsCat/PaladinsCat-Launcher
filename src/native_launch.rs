//! Windows client dispatch and DLL loading owned by this launcher.
//!
//! Preserves Tempest's client startup delay and compatible mod selection. The
//! loader uses UTF-16 paths, remote module-relative addresses and finite waits.
//! It never attaches to an existing client or executes downloaded DLLs in tests.
use super::{engine, error, external_path, path_key};
use std::{
    ffi::c_void,
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::{
            Debug::{ReadProcessMemory, WriteProcessMemory},
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
            },
        },
        LibraryLoader::{GetModuleFileNameW, GetModuleHandleW, GetProcAddress},
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE,
            VirtualAllocEx, VirtualFreeEx, VirtualQuery,
        },
        ProcessStatus::K32GetModuleFileNameExW,
        Threading::{CreateRemoteThread, GetCurrentProcess, OpenProcess, WaitForSingleObject},
    },
};

const STARTUP_DELAY: Duration = Duration::from_secs(1);
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(30);
const REAP_TIMEOUT: Duration = Duration::from_secs(5);

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn modules(pid: u32) -> engine::Result<Vec<(PathBuf, usize, usize)>> {
    let process = Handle(unsafe { OpenProcess(0x410, 0, pid) }); // QUERY_INFORMATION | VM_READ
    if process.0.is_null() {
        return Err(format!(
            "Read child modules: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Snapshot can return ERROR_BAD_LENGTH while the loader list changes.
    let deadline = Instant::now() + Duration::from_secs(1);
    let snapshot = loop {
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if raw != INVALID_HANDLE_VALUE {
            break Handle(raw);
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(24) || Instant::now() >= deadline {
            return Err(format!("Read child module inventory: {e}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
    let mut more = unsafe { Module32FirstW(snapshot.0, &mut entry) };
    if more == 0 {
        return Err(format!(
            "Read first child module: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut result = Vec::new();
    while more != 0 {
        // Toolhelp's snapshot can lose non-ANSI characters even through its W
        // API. Resolve the snapshot's module handle against the actual child
        // with PSAPI's wide path query; never match a lossy question-mark path.
        let mut path = vec![0u16; 32768];
        let len = unsafe {
            K32GetModuleFileNameExW(
                process.0,
                entry.hModule,
                path.as_mut_ptr(),
                path.len() as u32,
            )
        };
        if len == 0 || len as usize >= path.len() {
            return Err(format!(
                "Read child module path: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path = PathBuf::from(String::from_utf16(&path[..len as usize]).map_err(error)?);
        result.push((path, entry.modBaseAddr as usize, entry.modBaseSize as usize));
        more = unsafe { Module32NextW(snapshot.0, &mut entry) };
    }
    let e = std::io::Error::last_os_error();
    if e.raw_os_error() != Some(18) {
        return Err(format!("Read remaining child modules: {e}"));
    }
    Ok(result)
}

fn wide(value: &str) -> engine::Result<Vec<u16>> {
    if value.contains('\0') {
        return Err("DLL path contains a NUL".into());
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

fn image_identity(process: HANDLE, base: usize) -> engine::Result<[u32; 4]> {
    let mut header = [0u8; 4096];
    let mut read = 0;
    if unsafe {
        ReadProcessMemory(
            process,
            base as *const c_void,
            header.as_mut_ptr().cast(),
            header.len(),
            &mut read,
        )
    } == 0
        || read != header.len()
    {
        return Err(format!(
            "Read loader image identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    if &header[..2] != b"MZ" {
        return Err("Invalid loader image DOS header".into());
    }
    let pe = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
    if pe > header.len() - 92 || &header[pe..pe + 4] != b"PE\0\0" {
        return Err("Invalid loader image PE header".into());
    }
    let machine = u16::from_le_bytes(header[pe + 4..pe + 6].try_into().unwrap());
    let magic = u16::from_le_bytes(header[pe + 24..pe + 26].try_into().unwrap());
    if machine != 0x8664 || magic != 0x20b {
        return Err("Standalone client loader requires Windows x64 images".into());
    }
    Ok([
        machine as u32,
        u32::from_le_bytes(header[pe + 8..pe + 12].try_into().unwrap()),
        u32::from_le_bytes(header[pe + 80..pe + 84].try_into().unwrap()),
        u32::from_le_bytes(header[pe + 88..pe + 92].try_into().unwrap()),
    ])
}

fn loader_address(child: &Child) -> engine::Result<usize> {
    let name = wide("kernel32.dll")?;
    let module = unsafe { GetModuleHandleW(name.as_ptr()) };
    if module.is_null() {
        return Err(format!(
            "Resolve system loader module: {}",
            std::io::Error::last_os_error()
        ));
    }
    let function = unsafe { GetProcAddress(module, c"LoadLibraryW".as_ptr().cast()) }
        .ok_or_else(|| format!("Resolve LoadLibraryW: {}", std::io::Error::last_os_error()))?;
    let address = function as usize;
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut info,
            std::mem::size_of_val(&info),
        )
    } == 0
    {
        return Err(format!(
            "Resolve loader address owner: {}",
            std::io::Error::last_os_error()
        ));
    }
    // GetProcAddress can forward kernel32's export into KernelBase. Resolve
    // the owning module rather than assuming identical ASLR addresses.
    let mut buffer = vec![0u16; 32768];
    let len = unsafe {
        GetModuleFileNameW(
            info.AllocationBase,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    };
    if len == 0 || len as usize >= buffer.len() {
        return Err(format!(
            "Resolve loader owner path: {}",
            std::io::Error::last_os_error()
        ));
    }
    let local = PathBuf::from(String::from_utf16(&buffer[..len as usize]).map_err(error)?);
    let key = path_key(&local)?;
    let rva = address
        .checked_sub(info.AllocationBase as usize)
        .ok_or("Invalid loader RVA")?;
    let matches = modules(child.id())?
        .into_iter()
        .filter_map(|(path, base, size)| {
            (path_key(&path).ok().as_deref() == Some(&key)).then_some((base, size))
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("Child loader module identity is missing or ambiguous".into());
    }
    if rva >= matches[0].1 {
        return Err("Child loader RVA escapes its owning image".into());
    }
    let local_identity =
        image_identity(unsafe { GetCurrentProcess() }, info.AllocationBase as usize)?;
    let remote_identity = image_identity(child.as_raw_handle(), matches[0].0)?;
    if local_identity != remote_identity || remote_identity[2] as usize != matches[0].1 {
        return Err("Child loader image build differs from the local resolved export".into());
    }
    matches[0]
        .0
        .checked_add(rva)
        .ok_or_else(|| "Child loader address overflow".into())
}

fn load(child: &Child, dll: &Path, timeout: Duration) -> engine::Result<()> {
    let process = child.as_raw_handle();
    let expected = path_key(dll)?;
    if modules(child.id())?
        .iter()
        .any(|(path, _, _)| path_key(path).ok().as_deref() == Some(&expected))
    {
        // An earlier selected module can import this exact pinned DLL. Match
        // identity, then keep loading idempotent instead of duplicating it.
        return Ok(());
    }
    let path = wide(&external_path(dll)?)?;
    let size = path.len().checked_mul(2).ok_or("DLL path size overflow")?;
    let loader = loader_address(child)?;
    let allocation = unsafe {
        VirtualAllocEx(
            process,
            std::ptr::null(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if allocation.is_null() {
        return Err(format!(
            "Allocate child DLL path: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut written = 0;
    if unsafe {
        WriteProcessMemory(
            process,
            allocation,
            path.as_ptr().cast(),
            size,
            &mut written,
        )
    } == 0
        || written != size
    {
        let e = std::io::Error::last_os_error();
        unsafe { VirtualFreeEx(process, allocation, 0, MEM_RELEASE) };
        return Err(format!("Write child DLL path: {e}"));
    }
    let start: unsafe extern "system" fn(*mut c_void) -> u32 =
        unsafe { std::mem::transmute(loader) };
    let raw = unsafe {
        CreateRemoteThread(
            process,
            std::ptr::null(),
            0,
            Some(start),
            allocation,
            0,
            std::ptr::null_mut(),
        )
    };
    if raw.is_null() {
        let e = std::io::Error::last_os_error();
        unsafe { VirtualFreeEx(process, allocation, 0, MEM_RELEASE) };
        return Err(format!("Start child DLL loader: {e}"));
    }
    let thread = Handle(raw);
    match unsafe { WaitForSingleObject(thread.0, timeout.as_millis().min(u32::MAX as u128) as u32) }
    {
        WAIT_OBJECT_0 => {
            // LoadLibraryW returns an HMODULE, which must not be truncated to
            // GetExitCodeThread's DWORD. Confirm the exact file in the module list.
            unsafe { VirtualFreeEx(process, allocation, 0, MEM_RELEASE) };
            let loaded = modules(child.id())?;
            if !loaded
                .iter()
                .any(|(path, _, _)| path_key(path).ok().as_deref() == Some(&expected))
            {
                return Err(format!(
                    "Windows loader did not load selected DLL: {}",
                    dll.display()
                ));
            }
            Ok(())
        }
        WAIT_TIMEOUT => {
            // The path is still owned by the running loader thread. The caller
            // terminates this newly-created child before releasing its process.
            Err(format!("Selected DLL load timed out: {}", dll.display()))
        }
        _ => Err(format!(
            "Wait for selected DLL: {}",
            std::io::Error::last_os_error()
        )),
    }
}

fn cleanup(child: &mut Child, reason: String) -> String {
    let mut error = reason;
    if child.try_wait().ok().flatten().is_none()
        && let Err(e) = child.kill()
    {
        error.push_str(&format!("; terminate new child {}: {e}", child.id()));
    }
    let until = Instant::now() + REAP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return error,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                error.push_str(&format!(
                    "; new child {} exit unconfirmed after 5 seconds",
                    child.id()
                ));
                return error;
            }
            Err(e) => {
                error.push_str(&format!("; reap new child {}: {e}", child.id()));
                return error;
            }
        }
    }
}

pub(super) fn dispatch(command: &mut Command, dlls: &[PathBuf]) -> engine::Result<Child> {
    dispatch_with(command, dlls, DISPATCH_TIMEOUT)
}

fn dispatch_with(
    command: &mut Command,
    dlls: &[PathBuf],
    dispatch_timeout: Duration,
) -> engine::Result<Child> {
    let mut child = command.spawn().map_err(error)?;
    let started = Instant::now();
    let result = (|| {
        if dlls.is_empty() {
            return Ok(());
        }
        // Match the upstream client loader delay, without imposing it on an
        // unmodified launch. Sequential loads make selected order deterministic.
        std::thread::sleep(STARTUP_DELAY);
        if let Some(status) = child.try_wait().map_err(error)? {
            return Err(format!("Game exited before DLL loading: {status}"));
        }
        for dll in dlls {
            let remaining = dispatch_timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err("Standalone DLL dispatch exceeded its loading deadline".into());
            }
            load(&child, dll, LOAD_TIMEOUT.min(remaining))?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        return Err(cleanup(&mut child, e));
    }
    // Successful dispatch relinquishes process handles; it must not kill the
    // persistent game when the launcher/helper exits. No kill-on-close job.
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, os::windows::process::CommandExt, process::Stdio, sync::OnceLock};

    #[test]
    #[ignore = "owned CPU-only subprocess fixture"]
    fn owned_child() {
        if let Some(path) = std::env::var_os("PC_CHILD_READY") {
            std::fs::write(path, std::process::id().to_string()).unwrap();
            let mut gate = [0u8; 1];
            let _ = std::io::stdin().read_exact(&mut gate);
        }
    }

    fn compile_fixture(source: &Path, dll: &Path) {
        let diagnostics = dll.with_extension("compiler.log");
        let stderr = std::fs::File::create(&diagnostics).unwrap();
        let mut compiler = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "--crate-name=pc_loader_fixture",
            ])
            .arg(source)
            .arg("-o")
            .arg(dll)
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(exit) = compiler.try_wait().unwrap() {
                assert!(
                    exit.success(),
                    "synthetic Rust fixture DLL failed to compile: {}",
                    std::fs::read_to_string(&diagnostics).unwrap_or_default()
                );
                break;
            }
            if Instant::now() >= deadline {
                panic!(
                    "{}",
                    cleanup(
                        &mut compiler,
                        "synthetic compiler exceeded 30 seconds".into()
                    )
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn fixture() -> &'static PathBuf {
        static DLL: OnceLock<PathBuf> = OnceLock::new();
        DLL.get_or_init(|| {
            let root = std::env::temp_dir()
                .join(format!("paladinscat-owned-loader-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let dll = root.join("번역 자료, standalone fixture.dll");
            let source =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/native_loader_fixture.rs");
            compile_fixture(&source, &dll);
            dll
        })
    }

    fn child_command(name: &str) -> (Command, PathBuf, PathBuf) {
        let root = fixture().parent().unwrap();
        let ready = root.join(format!("{name}-ready"));
        let marker = root.join(format!("{name}-loaded"));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "launch::native::tests::owned_child", "--ignored"])
            .env("PC_CHILD_READY", &ready)
            .env("PC_LOADER_MARKER", &marker)
            .creation_flags(0x08000200)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        (command, ready, marker)
    }

    #[test]
    fn unicode_dll_round_trip_and_successful_dispatch_survives_handle_release() {
        let (mut command, ready, marker) = child_command("unicode");
        let mut child = dispatch(&mut command, std::slice::from_ref(fixture())).unwrap();
        assert_eq!(
            std::fs::read_to_string(ready).unwrap(),
            child.id().to_string()
        );
        assert_eq!(std::fs::read(marker).unwrap(), b"fixture-loaded\n");
        assert!(child.try_wait().unwrap().is_none());
        assert!(
            modules(child.id())
                .unwrap()
                .iter()
                .any(|(path, _, _)| path_key(path).ok() == path_key(fixture()).ok())
        );
        // Keeping the input pipe proves it cannot finish naturally. Dropping
        // the launcher's process handle must leave this persistent child alive.
        let gate = child.stdin.take().unwrap();
        let pid = child.id();
        drop(child);
        let process = Handle(unsafe {
            windows_sys::Win32::System::Threading::OpenProcess(0x00100001, 0, pid)
        });
        assert!(!process.0.is_null());
        assert_eq!(unsafe { WaitForSingleObject(process.0, 25) }, WAIT_TIMEOUT);
        assert_ne!(
            unsafe { windows_sys::Win32::System::Threading::TerminateProcess(process.0, 1) },
            0
        );
        assert_eq!(
            unsafe { WaitForSingleObject(process.0, 5000) },
            WAIT_OBJECT_0
        );
        drop(gate);
    }

    fn assert_child_exited(ready: &Path) {
        let pid: u32 = std::fs::read_to_string(ready).unwrap().parse().unwrap();
        let process = Handle(unsafe { OpenProcess(0x00100000, 0, pid) });
        if !process.0.is_null() {
            assert_eq!(unsafe { WaitForSingleObject(process.0, 0) }, WAIT_OBJECT_0);
        }
    }

    #[test]
    fn import_dependency_is_loaded_once_when_also_selected_explicitly() {
        let root = fixture().parent().unwrap();
        let dep_source = root.join("dependency.rs");
        let dep = root.join("pc_loader_dependency.dll");
        std::fs::write(
            &dep_source,
            "#[unsafe(no_mangle)] pub extern \"system\" fn PcSyntheticDependency() -> u32 {42}",
        )
        .unwrap();
        compile_fixture(&dep_source, &dep);
        let source = root.join("importer.rs");
        let importer = root.join("importer.dll");
        let prefix = "#[link(name=\"pc_loader_dependency\",kind=\"raw-dylib\")] unsafe extern \"system\" { fn PcSyntheticDependency() -> u32; }\n";
        let body = include_str!("../examples/native_loader_fixture.rs")
            .replace("//!", "//")
            .replace(
                "if reason == 1 {",
                "if reason == 1 { if unsafe {super::PcSyntheticDependency()} != 42 {return 0;}",
            );
        std::fs::write(&source, format!("{prefix}{body}")).unwrap();
        compile_fixture(&source, &importer);
        let (mut command, ready, marker) = child_command("dependency");
        let mut path = root.as_os_str().to_os_string();
        path.push(";");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        command.env("PATH", path);
        let mut child = dispatch(&mut command, &[importer, dep.clone()]).unwrap();
        assert_eq!(std::fs::read(marker).unwrap(), b"fixture-loaded\n");
        assert!(
            modules(child.id())
                .unwrap()
                .iter()
                .any(|(path, _, _)| path_key(path).ok() == path_key(&dep).ok())
        );
        assert!(!cleanup(&mut child, "fixture cleanup".into()).contains("unconfirmed"));
        assert_child_exited(&ready);
    }

    #[test]
    fn failed_and_timed_out_loads_terminate_only_the_new_child() {
        let (mut unrelated_cmd, _, _) = child_command("unrelated");
        let mut unrelated = dispatch(&mut unrelated_cmd, &[]).unwrap();
        let (mut command, ready, _) = child_command("missing");
        let started = Instant::now();
        let err = dispatch(&mut command, &[fixture().with_file_name("missing.dll")]).unwrap_err();
        assert!(!err.is_empty());
        assert!(std::fs::read(&ready).is_ok());
        assert_child_exited(&ready);
        assert!(started.elapsed() < Duration::from_secs(7));
        assert!(unrelated.try_wait().unwrap().is_none());
        let (mut command, ready, marker) = child_command("timeout");
        command.env("PC_LOADER_DELAY", "1");
        let started = Instant::now();
        let err = dispatch_with(
            &mut command,
            std::slice::from_ref(fixture()),
            Duration::from_millis(1300),
        )
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(std::fs::read(marker).is_ok());
        assert_child_exited(&ready);
        assert!(started.elapsed() < Duration::from_secs(7));
        assert!(unrelated.try_wait().unwrap().is_none());
        assert!(!cleanup(&mut unrelated, "fixture cleanup".into()).contains("unconfirmed"));
    }
}
