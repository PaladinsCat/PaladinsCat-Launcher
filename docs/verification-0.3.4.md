# Launcher 0.3.4 feature sources

The owner's existing `main` workflow publishes the reviewed locale catalog and
launcher corrections. No new branch, worktree or pull request is part of this
release. Existing launch-default corrections are included, preserving saved
language/profile selections.

Repository identities and URLs are consolidated in `src/sources.rs`. Each
feature tab contains its repository link, source details and relevant status.
Localization and Korean IME use the configured locale catalog; Performance uses
the reviewed pinned mods release. The common header contains only the brand.

Verified 2026-10-12:

- 78 locked launcher tests passed; the existing optional performance network
  test remains ignored. Cases cover independent locale/IME counts, retained
  downloaded selections, catalog-source invalidation, asynchronous tab changes
  and actual egui text placement under each selected tab.
- Formatting, strict all-target Clippy, release build and public source audit
  passed. The compiled executable icon matches the 4,059-byte source PNG.
- All three prepared CPU previews were rendered and inspected. The common
  header has no locale repository; Performance shows only the mods repository.
- The published locale `main` is `1bde91a58ca9dbd917e5e74d764ef41cb0d7b78c`.
  Its catalog offers one localization and one IME package. GitHub marks the
  existing preview PR47 merged at `3015e5cf293569f1248287a5b95448972b0ed44d`.
- The production download verifier retrieved every translation payload and IME
  payload from that raw `main` catalog into a separate local cache and verified
  the manifest, size and hash contracts without game installation.
- CPU startup inspection restored the saved Korean v172, IME 0.1.0 and
  performance 0.1.0 selections. Source diagnostics identify each feature's own
  repository. Explicit saved profile/language choices remain selected.
- WatchCat's required script passed 40 tests and configured checks using its
  existing local Cargo target. The initial NAS-target executable was denied
  execution; no per-directory ACL or share policy was changed.

CPU previews do not establish live GPU routing. An updated GUI's renderer
identity must match its new process and version; live relaunch remains subject
to the current human instruction.

Package publication does not complete all Korean translations or portable IME
gameplay/live restoration acceptance. Those existing gates remain separate.
