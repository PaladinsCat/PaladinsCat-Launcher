# Launcher 0.3.8 preview

Historical local build/package evidence. The agent selected 0.3.8 and replaced
local staging without an approved release cutover; the human rejected that
workflow. These check results do not approve that version or cutover. Direct EXE
delivery and the [release/version contract](../CONTRIBUTING.md#artifact-format-and-release-version-control)
supersede the prior ZIP workflow.

Expanding repo settings no longer repeats the footer or displays the backend
launch report. The panel shows the selectable local data/downloads path and
Copy log path after a launch. Raw launch/error/cleanup diagnostics require a
separate Diagnostics expansion. Korean repo labels use 레포.

The real ENG/KOR egui CPU previews were inspected with repo settings open.
Success/pending launch results, failed cache clearing, cleanup warnings and all
three feature tabs retain short copy before Diagnostics is opened. A Unicode
log-path test verified the exact clipboard output without opening a file.

134 tests passed, with two existing opt-in fixtures ignored. Formatting,
strict all-target Clippy, Windows x64 release build, embedded-icon verification
and public source audit passed. The portable candidate retains Korean v175,
matching launcher source and all 424 locked dependency sources, notices,
catalog provenance and verified checksums.

No live GUI/game restart, installation or public release was performed.
The workstation's explicit RTX 5080 pin is preserved. Live gameplay and full
native translation acceptance remain separate from this UI/package verification.
