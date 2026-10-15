# PaladinsCat Launcher contributors

Follow the parent PaladinsCat workspace publication boundary and canonical OpenViking SOP. This independent, public Rust repository owns the launcher GUI, download/rollback code and launch workflow. WatchCat retains native IME/performance hook implementation and private research. PaladinsCat-locales retains locale authoring and release catalogs; PaladinsCat-mods retains performance release payloads.

## Main-only workspace

The user requires this project's existing checkout to stay on `main` and prohibits agent-created pull requests. Do not create or switch to feature branches, auxiliary worktrees, forks or alternate source checkouts. Do not create a PR for a fix, release or catalog update. Before editing, verify `git branch --show-current` and `git worktree list --porcelain`. Preserve existing fixes and unrelated work when correcting checkout state. Generic contributor PR instructions do not override this direct human rule. Keep review and checks on existing `main`; publication requires its established authorization and must respect remote protections. A protection failure is not permission to create a PR or bypass it.

## Launcher delivery and version control

The human's 2026-10-14 direction is EXE delivery for launcher testing and releases:
provide `paladinscat-launcher.exe` directly, without requiring ZIP extraction.
For the explicitly authorized 2026-10-15 EXE-only GitHub release, upload only the
GUI EXE. Link matching source and notices in the tagged public repository and
the exact dependency-source index from the release page. Source downloads are
separate from running the GUI; no launcher ZIP or optional CLI asset is required.

UI fixes, checks and documentation work preserve the existing package version and
release staging. Do not increment Cargo/PE versions, create a versioned dist,
replace/recycle an existing dist, create a tag or publish a release unless the
human has authorized that release cutover. An occupied executable or a packager
that refuses an existing version is an implementation constraint, not authority
to invent a new version. Continue source work within existing authorization;
never manufacture a cutover to get around a packaging guard. Record an approved
release's version, source commit, EXE hash and publication state. Existing approval
persists within its scope; a new build hash alone does not require another approval.
See CONTRIBUTING.md for the direct-EXE artifact contract and self-update release requirements.

Self-update stays pinned to this launcher's own GitHub stable releases, independent
of editable game-package sources. Preserve checksum/size/PE-version validation,
exact process ownership, startup acknowledgement, one rollback copy and cleanup.
Test with owned CPU-only Rust fixtures. A test payload never authorizes a version
cutover, public release or workstation/VM GUI restart.

All runtime and automation code is Rust. This repository owns standalone Windows x64 game dispatch and DLL loading; it does not invoke or require an installed Tempest CLI. Preserve compatible profile/native defaults and enabled V2 `.tempest` registry contracts, validated package pins, native fingerprints, receipt/journal ownership and manual existing-client exit. Historical CLI-based verification remains evidence for its recorded version, not a current runtime dependency. Never infer runtime hooks from a checkbox, dry-run or prepared download. Do not execute a downloaded DLL to validate it. No task approval authorizes Computer Use or a foreground launch without the separately required human method/instruction approval.

Public builds select supported hardware before device allocation and refuse software fallback. The developer workstation retains an explicit per-user RTX 5080 pin in `graphics-policy.json`, with vendor/device/name/PCI checks and verified startup identity. Do not disable this local pin during release preparation. Builds, tests and CPU previews are not graphics-routing evidence.

Preserve the human-established execution target across launcher restart and verification follow-ups. A request in an established VM context does not authorize dispatch on the local workstation. Follow **Execution target continuity before dispatch** in the canonical OpenViking `viking://resources/sop/principles.md`; bind the direct human context and target identity to the active session. Release preparation does not resume stopped VM, GUI or game verification or authorize Computer Use.

Use the commands in CONTRIBUTING.md. Preserve public GPL attribution. Never publish private WatchCat code, recordings, runtime credentials, game/system DLLs or Tempest binaries. Owner-directed agent work uses existing `main` without a PR; external contributor procedures are not agent authorization. Generated files remain ignored. Avoid promotional or redundant descriptions in the GUI and public release copy.
