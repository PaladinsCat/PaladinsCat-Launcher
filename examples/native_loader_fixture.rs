//! Synthetic loader test only. Compiled as a Rust DLL by the owned-child test;
//! never installed into or attached to a game. No third-party native binary.
#[cfg(windows)]
mod fixture {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetEnvironmentVariableW(name: *const u16, output: *mut u16, length: u32) -> u32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn WriteFile(
            file: *mut c_void,
            data: *const u8,
            count: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn Sleep(milliseconds: u32);
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn DllMain(
        _module: *mut c_void,
        reason: u32,
        _reserved: *mut c_void,
    ) -> i32 {
        if reason == 1 {
            // OS-only operations under the loader lock. No Rust filesystem,
            // allocations, synchronization or worker threads in DllMain.
            const NAME: &[u16] = &[
                80, 67, 95, 76, 79, 65, 68, 69, 82, 95, 77, 65, 82, 75, 69, 82, 0,
            ];
            const DELAY: &[u16] = &[
                80, 67, 95, 76, 79, 65, 68, 69, 82, 95, 68, 69, 76, 65, 89, 0,
            ];
            let mut path = [0u16; 32768];
            let len = unsafe {
                GetEnvironmentVariableW(NAME.as_ptr(), path.as_mut_ptr(), path.len() as u32)
            };
            if len > 0 && (len as usize) < path.len() {
                let handle = unsafe {
                    CreateFileW(
                        path.as_ptr(),
                        0x40000000,
                        1,
                        std::ptr::null_mut(),
                        2,
                        0x80,
                        std::ptr::null_mut(),
                    )
                };
                if !handle.is_null() && handle as isize != -1 {
                    let mut written = 0;
                    unsafe {
                        WriteFile(
                            handle,
                            b"fixture-loaded\n".as_ptr(),
                            15,
                            &mut written,
                            std::ptr::null_mut(),
                        );
                        CloseHandle(handle);
                    }
                }
            }
            let mut delay = [0u16; 8];
            if unsafe {
                GetEnvironmentVariableW(DELAY.as_ptr(), delay.as_mut_ptr(), delay.len() as u32)
            } > 0
            {
                unsafe { Sleep(30_000) };
            }
        }
        1
    }
}

#[allow(dead_code)]
fn main() {}
