# Launcher 0.3.2 icon correction

The native window used eframe's default icon because its viewport had no explicit icon. The executable also lacked a Windows icon resource.

Both now use the existing `assets/paladinscat.png`. The window decodes that embedded PNG; the Rust build script wraps its unchanged PNG bytes in an ICO and compiles the executable resource using pinned `winresource` 0.1.31. No second authored logo is introduced.

Verification:

- 73 launcher tests passed; the existing optional network test remains ignored.
- Format, strict Clippy, locked release build and public-source boundary check passed.
- The `verify_icon` example mapped the built executable as data and verified its icon payload is exactly the source PNG (4,059 bytes). CI runs this check after the release build.
- The packaged executable also passed that resource check. Its Windows version metadata reports 0.3.2 and PaladinsCat Launcher.

These are CPU-only source/build/resource checks. No desktop capture or GUI restart was performed; the already-running 0.3.1 window requires a restart to display the updated window icon. RTX 5080 selection and verification remain unchanged.
