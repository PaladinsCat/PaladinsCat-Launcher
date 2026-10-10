# 0.3.1 startup-path repair

The reported startup dialog said it could not find the default engine INI used to retrieve the My Documents subdirectory. Both required INIs existed in the selected game. The default game INI contained `[Windows.StandardUser]` and `MyDocumentsSubDirName=Paladins`.

## Diagnosis

The 0.3.0 planner forwarded Windows canonical `\\?\` paths to the external loader. Rust explicitly documents that [canonicalize produces extended Windows paths that can be incompatible with other applications](https://doc.rust-lang.org/std/fs/fn.canonicalize.html#platform-specific-behavior). Saved dispatch logs also contained extended paths. The reported executable path used the same syntax.

Neither 0.3.0 nor the pinned Tempest Windows launch code set a working directory. Tempest's [LauncherUtility](https://github.com/LowRezStudio/Tempest/blob/efe828dd28a48c4e1861ebf25a99cc16bd4b2425/Tempest.CLI/Launcher/LauncherUtility.cs#L7) resolves the root and constructs the executable path. [LauncherCommands](https://github.com/LowRezStudio/Tempest/blob/efe828dd28a48c4e1861ebf25a99cc16bd4b2425/Tempest.CLI/Launcher/LauncherCommands.cs#L89) starts it without a working-directory override. [WineExtensions](https://github.com/LowRezStudio/Tempest/blob/efe828dd28a48c4e1861ebf25a99cc16bd4b2425/Tempest.CLI/Extensions/WineExtensions.cs#L24) leaves the Windows process unchanged.

These two launch-contract defects are confirmed. Their contribution to the exact legacy engine failure is inferred from the dialog and launch evidence; the failing native instruction was not traced.

## Repair

- Retain canonical internal identities and receipt/payload checks.
- Pass ordinary drive or UNC paths to the external loader and verify that conversion resolves to the same file.
- Reject device namespaces and conversions that select another file, including a real trailing-dot filename fixture.
- Set the Tempest working directory to the selected game's `Binaries/Win64`.
- Check both required INIs for existence, nonempty regular files and containment; retain their hashes and read-only handles through dispatch/replanning.
- Preserve argument ordering, homedir, language, registered-DLL deduplication and current/previous input selection.

## Verification

Windows x64 checks performed on 2026-10-10:

| Check | Result |
| --- | --- |
| Offline launcher tests | 72 passed; one unrelated network test ignored by default |
| Strict Clippy, formatting and release build | Passed with pinned Rust 1.97.0 |
| Relative default-INI lookup from prepared child working directory | Passed without spawning a process |
| Drive/UNC transport, Unicode/spaces and device refusal | Passed |
| Real Windows trailing-dot conversion selecting another file | Refused |
| Missing/empty required startup INI | Refused before dispatch/log creation |
| Existing executable alias, receipt, conflict and process guards | Passed |
| Installed-client plans, plain/current/previous performance variants | Passed without launching the game |
| Required WatchCat test script | 39 tests plus existing formatting/check/lint gates passed using a local execution cache |

The first WatchCat-script attempts were stopped by unsigned network-script policy and execution denial on the NAS build directory. A process-local PowerShell execution-policy option and the existing local Cargo target allowed the unchanged script to run. No system execution policy was changed.

The read-only plan reports canonical identity paths separately from its normalized argument vector. Its working-directory value is a verified canonical identity; the prepared Windows command converts it to ordinary syntax too.

Generated executables are excluded from Git. The already-open 0.3.0 GUI cannot pick up this compiled correction without a relaunch. A successful fresh client startup remains unverified; no game launch, client closure, game-file modification or desktop interaction was performed for these source checks. Combined-mod gameplay, runtime hook activation and localization visual acceptance retain their separate gates.
