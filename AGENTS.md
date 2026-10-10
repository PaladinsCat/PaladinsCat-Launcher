# PaladinsCat Launcher contributors

Follow the parent PaladinsCat workspace publication boundary and canonical OpenViking SOP. This independent, public Rust repository owns the launcher GUI, download/rollback code and launch workflow. WatchCat retains native IME/performance hook implementation and private research. PaladinsCat-locales retains locale authoring and release catalogs; PaladinsCat-mods retains performance release payloads.

## Main-only workspace

The user requires this project's existing checkout to stay on `main`. Do not create or switch to feature branches, auxiliary worktrees or alternate source checkouts. Before editing, verify `git branch --show-current` and `git worktree list --porcelain`. Preserve existing fixes and unrelated work when correcting checkout state. GitHub publication still follows the parent publication boundary; a local checkout correction does not authorize bypassing owner review or protected `main`.

All runtime and automation code is Rust. Keep the existing Tempest CLI launch contract, validated package pins, native fingerprints, receipt/journal ownership and manual existing-client exit. Never infer runtime hooks from a checkbox or prepared download. Do not execute a downloaded DLL to validate it. No task approval authorizes Computer Use or a foreground launch without the separately required human method/instruction approval.

Graphics workloads retain NVIDIA RTX 5080 selection before device allocation and verified startup identity. Builds, tests and CPU previews are not graphics-routing evidence.

Use the commands in CONTRIBUTING.md. Preserve public GPL attribution. Never publish private WatchCat code, recordings, runtime credentials, game/system DLLs or Tempest binaries. Use owner-reviewed pull requests after bootstrap; generated files remain ignored. Avoid promotional or redundant descriptions in the GUI and public release copy.
