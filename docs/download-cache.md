# Downloaded package retention

The launcher keeps one most recently prepared package for each independent
family: translation, Korean IME and performance. New packages are verified first;
older managed download data is then sent to the Windows Recycle Bin. Unchanged
translation payloads are reused by SHA256. Compressed transport files are
discarded after decoding, so they do not duplicate the retained package forever.
Performance downloads can reuse the verified decoded package without fetching
its ZIP again. Failed preparation recycles only newly created attempt data and
preserves the previous verified generation.

**Clear downloads** (**다운로드 삭제** in KOR) is visible beside the bottom
repository disclosure. It runs in the background, is disabled during other launcher work and
clears downloaded/prepared packages for all three families. Completion resets
only prepared package paths/readiness, including after partial cleanup errors.
Game selection, interface/game language, profile and feature preferences remain.
Performance needs its package downloaded again before the next enabled launch.

Installed game/mod files, original backups, settings and launch logs are outside
download cleanup. The download owner uses one cross-process lock shared with
download/import and installer reads. Links/reparse points and paths outside that
owner are refused before recycling. A registered or currently configured game
with a pending localization journal must be recovered before cache cleanup.

Installed localization no longer depends on retired download paths. Before an
update/restore, receipt identity, live hashes and original backups are checked,
then the verified installed bytes form a game-owned recovery checkpoint. Keep
one prior-version checkpoint after an update, and at most the current checkpoint
plus one prior checkpoint during rollback/recovery. Completed journals do not
retain older checkpoint generations. Clearing downloads cannot remove these
checkpoints or game originals.

Recycling failures are reported as maintenance diagnostics in collapsed details.
A verified new preparation remains selected even after partial old-cache cleanup.
A post-commit checkpoint-cleanup error does not falsely mark a completed install
as requiring recovery. Removed data is recoverable from the Windows Recycle Bin;
emptying that bin releases its disk space.

Verification uses owned CPU fixtures: latest-only generations, unchanged hash
reuse, failed preparation, concurrent cache access, foreign links, recovery
journals, clear/update/rollback/restore, bounded checkpoints, cleanup failure and
ENG/KOR control/state/layout. Synthetic recycling moves fixture files within
their fixture owner; tests do not clear the user's downloads or invoke desktop
input. A release build and CPU previews are distinct from live GUI acceptance.

Implementation validation on October 14, 2026: all-target tests passed (120,
with two existing ignored tests), formatting and strict all-target Clippy passed,
and the optimized launcher/release CLI built successfully. ENG/KOR CPU previews
confirm the compact bottom row; public source audit covers 61 files and nine
revisions. The launcher PE retains the exact existing source icon.

The Windows recycler uses the documented [IFileOperation flags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags)
to force recycling, suppress Shell UI and stop on errors. It never invokes a
permanent-delete fallback. No running launcher, game or live user cache was
changed during implementation checks.
