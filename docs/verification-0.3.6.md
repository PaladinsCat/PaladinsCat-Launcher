# Launcher 0.3.6 standalone launch

Version 0.3.6 owns Windows x64 game dispatch and DLL loading in Rust. It no
longer requires an installed Tempest CLI. Profile/native defaults and enabled
V2 `.tempest/mods/mods.json` registry compatibility remain, together with managed
package pins, receipt ownership and existing-client refusal. Tempest GPL
attribution and earlier versioned verification records are retained.

The GUI removes the Tempest executable field and browse control. The CLI removes
`--tempest-cli`; dry-run output identifies `game_executable` and the complete
`load_dlls` list while retaining `hooks_verified: false`. Existing settings with
the retired `tempest_cli` key keep their saved language, profile, package paths,
enablement and source selections.

## Source and build checks

Verified 2026-10-13:

- 84 locked all-target tests passed.
- Formatting, strict all-target Clippy, the release build and compiled icon
  verification passed.
- Owned CPU-only subprocess tests compiled synthetic Rust DLLs and verified
  Unicode-path loading, import dependency deduplication, load failure/timeout
  cleanup and process/handle lifetimes. These fixtures do not execute downloaded
  game packages or start the game.
- Settings migration tests preserved all existing selections while ignoring
  legacy string, null and object values under the retired Tempest path key.

The Rust loader resolves the owning system image of `LoadLibraryW`, verifies
the matching child image and computes its remote export address. It verifies
the loaded DLL's exact file identity through the child module inventory.
Loading and failed-child cleanup have finite waits; successful handle release
does not terminate the persistent game.

## Prepared local artifacts

The checked release GUI and CLI were staged under the existing `dist` root in
`dist/0.3.6`. The copied files were rehashed and matched their release outputs:

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `paladinscat-launcher.exe` | 18,116,608 | `b55bb7ce7ca815e8b4d4b600031c01e1d64753953cbab6043e71c9ef54e04491` |
| `paladinscat-launcher-cli.exe` | 2,868,224 | `e4b28eb751953e448f57cc0067f34a6327614dafa85ab40d4176e6e9bec0950b` |

`SHA256SUMS.txt` beside the binaries contains the same hashes. These are local
prepared artifacts; this staging step did not publish or commit them.

## Single Windows VM startup

One explicitly authorized integration launch ran through
`examples/interactive_launch_test.rs`, which calls the same `launch::start`
backend as **Play**. A one-shot interactive-token task dispatched that helper
in the logged-on desktop session rather than a service session. It used a
create-new result file to refuse a repeated dispatch; no recurring trigger was
created.

The VM had no installed Tempest application and no Tempest process. The selected
Win64 game started in desktop session 1. Its module inventory confirmed
`WolperReplay.dll` loaded, and its startup log contained `UUILogin::LoginReady 0`
with an active render thread. The game remained alive for more than three
minutes after the launch helper and remote command session exited. A later
inventory found it absent before the VM power-off operation; the game's exit
cause is unknown and is not attributed to that later shutdown. These are
recorded observations from that single instance; subsequent host/VM
configuration changes are separate work.

## Remaining verification

The GUI window was not opened. Its strict RTX 5080 selection policy remains
unchanged, and this VM startup is not live GUI GPU-routing evidence.

The startup log marker and loaded replay DLL do not establish authentication,
replay behavior, gameplay, reporting, performance gains or combined
IME/performance compatibility. Portable IME runtime status and live disable
also remain unverified. No second game launch, package publication or completed
gameplay/performance acceptance is claimed by this record.
