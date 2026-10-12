# Launcher 0.3.3 profile defaults

The GUI and CLI share `launch::DEFAULT_LANGUAGE` (`INT`) and `launch::DEFAULT_PROFILE` (`PaladinsCat`). Missing legacy settings use these defaults. Explicit saved choices survive deserialization, including the selected packages; the launcher does not migrate game profiles or recordings.

Verification:

- 73 launcher tests passed, including the legacy-settings/default and explicit-profile preservation assertions. The optional network test remains ignored.
- Format, strict Clippy and the locked release build passed.
- The public-source boundary audit passed.
- The packaged CLI dry-run against the selected Season 8.1 installation emitted `--homedir PaladinsCat` and `-Language=INT`. A second dry-run retained explicit `--profile paladinslive --language KOR` overrides. Neither started the game.
- The packaged GUI reports Windows file version 0.3.3. Its executable icon still matches the original PaladinsCat PNG (4,059 bytes).
- WatchCat's required `scripts/test.ps1` passed: 39 tests plus its format/check/Clippy gates.

These are CPU-only source, build and launch-plan checks. No GUI relaunch, game launch, desktop capture, replay migration or live playback was performed. Existing settings are preserved; an already-saved profile remains selected until the user changes it.
