# PaladinsCat Launcher contributors

Follow the parent PaladinsCat workspace publication boundary and canonical OpenViking SOP. This independent, public Rust repository owns the launcher GUI, download/rollback code and launch workflow. WatchCat retains native IME/performance hook implementation and private research. PaladinsCat-locales retains locale authoring and release catalogs; PaladinsCat-mods retains performance release payloads.

## Main-only workspace

The user requires this project's existing checkout to stay on `main` and prohibits agent-created pull requests. Do not create or switch to feature branches, auxiliary worktrees, forks or alternate source checkouts. Do not create a PR for a fix, release or catalog update. Before editing, verify `git branch --show-current` and `git worktree list --porcelain`. Preserve existing fixes and unrelated work when correcting checkout state. Generic contributor PR instructions do not override this direct human rule. Keep review and checks on existing `main`; publication requires its established authorization and must respect remote protections. A protection failure is not permission to create a PR or bypass it.

All runtime and automation code is Rust. Keep the existing Tempest CLI launch contract, validated package pins, native fingerprints, receipt/journal ownership and manual existing-client exit. Never infer runtime hooks from a checkbox or prepared download. Do not execute a downloaded DLL to validate it. No task approval authorizes Computer Use or a foreground launch without the separately required human method/instruction approval.

Graphics workloads retain NVIDIA RTX 5080 selection before device allocation and verified startup identity. Builds, tests and CPU previews are not graphics-routing evidence.

Use the commands in CONTRIBUTING.md. Preserve public GPL attribution. Never publish private WatchCat code, recordings, runtime credentials, game/system DLLs or Tempest binaries. Owner-directed agent work uses existing `main` without a PR; external contributor procedures are not agent authorization. Generated files remain ignored. Avoid promotional or redundant descriptions in the GUI and public release copy.
