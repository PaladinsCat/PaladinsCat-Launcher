# Launcher v0.3.8

The owner authorized this first stable release on 2026-10-15 with the instruction
"release the launcher. exe only". The version remains 0.3.8. Historical preview
records describe earlier local builds and do not establish this release's checks.

The release provides one uploaded asset, `paladinscat-launcher.exe`. It includes
ENG/KOR launcher localization, compact package versions, Steam/Epic/Tempest game
folder discovery, manual folder selection, PC/Steam/Epic launch options and
verified self-update through this launcher's own stable GitHub releases.

The tagged repository contains the matching Rust source, build instructions,
[GPL license](../LICENSE), [Tempest and third-party notices](../NOTICE.md),
font/icon licenses, Cargo.lock and the [dependency source index](dependency-sources.json).
Every dependency entry supplies the exact source download URL and locked SHA-256.
These sources are freely accessible independently of the executable download.

Release checks cover formatting, all-target unit tests and strict Clippy, public
source audit, Windows x64 build, embedded icon, ENG/KOR CPU layout previews and
owned Windows updater fixtures. The public download is checked against GitHub's
SHA-256/size, the local EXE and embedded PE version. Discovery of the live stable
release is checked for current and earlier versions without replacing a GUI.

No GUI/game restart or package installation is part of publication. CPU and
fixture evidence does not establish gameplay, platform login or native hook
acceptance. The developer workstation's explicit RTX 5080 pin is preserved.
