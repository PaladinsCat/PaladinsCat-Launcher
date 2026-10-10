# Launcher architecture

The GUI and installer moved from WatchCat's localization-patcher into this standalone public Rust crate. No build dependency reaches outside this repository. Native hook source stays in WatchCat. Locale authoring stays in PaladinsCat-locales; this launcher consumes its reviewed catalog and immutable release assets. Performance payloads stay in PaladinsCat-mods. The existing `%LOCALAPPDATA%/PaladinsCat/Localization` settings/cache and `.watchcat-localization` game receipts intentionally keep their names for compatibility.

Windows resolves the LocalAppData known folder with package redirection disabled instead of trusting a host's environment override. `paladinscat-launcher-cli --data-dir` prints the requested location without creating files. Windows can still virtualize file operations inherited from a packaged development host; CPU checks in that host recorded the effective cache path separately. Normal unpackaged use retains the existing patcher's settings location. This lookup does not claim to escape an MSIX container.

## Launch

**Play** validates the selected game, installed IME receipt, enabled performance package and native system fingerprints, then invokes the installed Tempest CLI without opening a console. The selected profile and three-letter game locale are passed as separate arguments. Custom DLLs appear before Tempest's `--` native-argument separator. Only one performance input variant is selected. The launcher does not close an existing client.

Canonical paths remain the internal identities for containment, receipts, deduplication and read-only file pins. External executable, game-root and DLL paths use ordinary Win32 drive/UNC syntax. Conversion must resolve back to the same file; unsupported device namespaces and conversions that redirect a trailing-dot/space component are refused. The Tempest child receives the game's `Binaries/Win64` as its working directory, which its Windows game child inherits. The two required startup INIs are checked under the selected root, hashed, pinned and compared again before dispatch. No game configuration is rewritten. See [0.3.1 verification](verification-0.3.1.md) for the startup-failure evidence and runtime boundary.

Tempest's automatic mod loading uses enabled V2 records in `.tempest/mods/mods.json`; directory presence alone does not enable a mod. Our own verified IME files without a registry entry are passed explicitly. A matching registered copy is omitted from explicit DLL arguments. Conflicting managed entries are refused. Unrelated Tempest mods remain under Tempest's control.

No background service is required. The in-process DLL retains its hooks after the GUI closes. Configuration and process discovery are distinct from verified hook activation; launch dispatch does not prove that the hooks initialized or that gameplay works. Turning off performance changes the next launch; the running client is not modified. IME enable/disable retains its existing guarded file transaction and recovery journal.

## Package and update contracts

Localization and IME use the existing versioned catalog schema, decoded hashes, game fingerprint and receipt/journal rollback. The performance v0.1.0 manifest, archive and source revision are pinned from the actual published assets. Archive extraction uses an exact filename allowlist, member hashes, sizes and checksums; executable contents are never invoked during validation. Source tests and cached packages do not prove performance gains or combined IME compatibility.

Game or Windows updates that break native pins refuse the affected mod. A newly supported build requires a reviewed package/profile. New performance releases require reviewed immutable pins in launcher source; automatic unreviewed latest-release switching is not implemented. Locale catalogs can advance independently through their governed publication workflow.

## Upstream maintenance

Tempest is the documented fork source. Keep its pinned upstream revision and GPL notice with the ported launch contracts. Review changes to upstream argument defaults, metadata layout and DLL loading before upgrading. Build-time sibling Tempest source dependencies and private diagnostic helpers are not required. The installed CLI is an explicit end-user runtime dependency in this version; a self-contained loader is not yet implemented.

## Verification boundaries

Fixture tests cover installation ownership, rollback, cache integrity, archive extraction and launch planning. CPU previews cover layout without desktop capture. Live combined-mod gameplay, portable IME runtime status and live disable remain unverified. The current inherited developer graphics policy requires the exact RTX 5080 identity before device allocation; a general public hardware-selection design is pending.
