# 0.3.0 verification

Source implementation checks performed on Windows x64, 2026-10-10.

| Check | Result |
| --- | --- |
| Standalone Rust crate, no sibling source dependencies | Passed |
| Format and strict Clippy across all targets | Passed |
| Offline unit tests | 68 passed; one network test ignored by default |
| Explicit published performance v0.1.0 download/import test | Passed; payloads not executed |
| Localization install/update/rollback/recovery | Temporary-file fixtures passed |
| IME enable/disable, receipts and concurrent-operation guards | Temporary-file fixtures passed |
| Tempest argument ordering, registered DLL deduplication and native-owner conflict refusal | Fixtures passed |
| Installed client and Windows DLL fingerprints | Read-only launch plans passed |
| Current and previous performance input variants | Read-only plans each selected one input DLL plus the world scheduler |
| Published localization/IME catalog | One localization and one IME package resolved; saved selections restored |
| GUI layout | Three CPU previews reviewed at 880 by 960; package/apply and toggle controls visible |
| User-data path | Known-folder lookup resists environment overrides; inherited MSIX file virtualization remains a host-specific boundary |
| Windows release executables | Built locally; generated artifacts excluded from Git |

The initial Windows CI run found a process-identity fixture failure caused by a
short-path TEMP alias comparing against the plan's full path. Executable identities
now resolve existing paths before comparison; the observation deadline is unchanged.
The regression covers requested/full/dotted aliases and inaccessible-path refusal.

The WatchCat workspace's required `scripts/test.ps1` also passed its 39 tests and existing format/check/lint gates during migration.

No game launch, game installation, desktop capture or foreground GUI interaction was performed for these checks. They do not establish live GUI GPU routing, native hook initialization, portable IME disable/restoration, combined-mod gameplay compatibility or performance improvement. Localization coverage and visual acceptance remain governed separately in the locale project.

The graphical build still enforces the inherited exact NVIDIA RTX 5080 device policy. CPU previews do not satisfy that policy. The installed Tempest CLI is currently required to launch the game; a bundled independent loader is not implemented.

## Reproduce

Run the commands in [CONTRIBUTING.md](../CONTRIBUTING.md). The explicit network check is:

```text
cargo test --locked --lib performance::tests::published_release_download_import_and_variant_selection -- --ignored --exact
```

Run `paladinscat-launcher-cli --dry-run GAME_ROOT --performance MANIFEST` with a verified cached manifest, then repeat with `--previous-input`. These prepare a cache and inspect launch inputs; they do not launch the game or change its installed files. Add `--launch` only when actually intending to start a client.

Run `source_audit` before publication to check the worktree and all reachable Git revisions for credential patterns and private/generated artifacts. GitHub secret scanning and push protection supplement this limited source-boundary check.
