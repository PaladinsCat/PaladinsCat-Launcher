# Launcher 0.3.7 release preparation

Prepared as a Windows x64 preview with the published Korean v175 catalog.
The launcher offers ENG/KOR interface language, a game-language dropdown,
compact status copy, latest-package download retention and 다운로드 삭제.
After applying Korean, select KOR under Game language before Play. A verified
installed-language mismatch now shows that direct action in ENG or KOR.

Public startup selects supported DX12 hardware before allocating its device
and refuses software rendering. An explicit per-user RTX 5080 pin preserves
the developer workstation's stricter identity checks.

Source checks on 2026-10-14: 132 tests passed, with two existing opt-in fixtures
ignored; formatting and strict all-target Clippy passed. The real downloader
verified Korean v175 and IME 0.1.0 using the existing cache without installing
game files or executing downloaded DLLs.
The Windows x64 release build, exact embedded icon and public source boundary
audit passed. Korean and English language-menu CPU previews were inspected.

Portable release preparation supplies matching source and all locked Cargo
dependency sources, retained GPL/third-party notices, catalog provenance and
SHA-256 checksums. Exact artifact hashes are in the prepared release manifest.
Runtime gameplay and live GUI/GPU acceptance remain separate from these source
and package checks. Native translation coverage remains a preview.
