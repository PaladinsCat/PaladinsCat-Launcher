//! Finite development-command runner. A Windows job cancels the complete child tree.
use std::{
    ffi::c_void,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::Command,
};

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working: usize,
    max_working: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(job: *mut c_void, class: i32, info: *const c_void, size: u32)
    -> i32;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn TerminateJobObject(job: *mut c_void, code: u32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtResumeProcess(process: *mut c_void) -> i32;
}
struct Job(*mut c_void);
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn run() -> Result<i32, String> {
    let args: Vec<_> = std::env::args().collect();
    // Finite regression fixture; never installed in the GUI or scheduled.
    if args.get(1).is_some_and(|a| a == "--fixture-child") {
        if args.get(2).is_some_and(|a| a == "tree") {
            let child = Command::new(&args[0])
                .args(["--fixture-child", "sleep"])
                .creation_flags(0x08000000)
                .spawn()
                .map_err(|e| e.to_string())?;
            println!("FIXTURE_CHILD_PID={}", child.id());
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
        return Ok(0);
    }
    if args.len() < 5 || args[1] != "--timeout-seconds" || args[3] != "--" {
        return Err("Usage: launcher-budget --timeout-seconds N -- PROGRAM ARGS".into());
    }
    let seconds: u32 = args[2].parse().map_err(|_| "Invalid timeout")?;
    if !(1..=600).contains(&seconds) {
        return Err("Timeout must be 1–600 seconds".into());
    }
    let job = Job(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    if job.0.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut limits = ExtendedLimits::default();
    limits.basic.flags = 0x2000; // KILL_ON_JOB_CLOSE
    if unsafe {
        SetInformationJobObject(
            job.0,
            9,
            (&limits as *const ExtendedLimits).cast(),
            std::mem::size_of::<ExtendedLimits>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // Suspend before assigning the job: descendants cannot escape the timeout.
    let mut child = Command::new(&args[4])
        .args(&args[5..])
        .creation_flags(0x08000004)
        .spawn()
        .map_err(|e| e.to_string())?;
    if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } == 0
        || unsafe { NtResumeProcess(child.as_raw_handle()) } < 0
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::last_os_error().to_string());
    }
    match unsafe { WaitForSingleObject(child.as_raw_handle(), seconds * 1000) } {
        0 => Ok(child.wait().map_err(|e| e.to_string())?.code().unwrap_or(1)),
        258 => {
            unsafe {
                TerminateJobObject(job.0, 124);
            }
            let _ = child.wait();
            eprintln!("TIMEOUT: cancelled child process tree after {seconds}s");
            Ok(124)
        }
        _ => {
            let error = std::io::Error::last_os_error().to_string();
            unsafe {
                TerminateJobObject(job.0, 1);
            }
            Err(error)
        }
    }
}
fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    });
}
