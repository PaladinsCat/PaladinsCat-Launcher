# PaladinsCat Launcher

## Use

1. Select your Paladins game folder.
2. Download a localization package and apply it while the game is closed.
3. Set **Game language** to **Korean (KOR)** to match the installed Korean package.
4. Choose any IME and performance options, then select **Play**.

Use the language menu at the top right to choose **ENG** or **KOR** for the launcher. This choice is saved separately from the game's language and profile.

Localization packages come from [PaladinsCat-locales](https://github.com/PaladinsCat/PaladinsCat-locales).
Performance packages come from [PaladinsCat-mods](https://github.com/PaladinsCat/PaladinsCat-mods).
Use **Restore originals** to revert localization. Disable performance for the next launch, or select its retained input version. Close the game before changing installed IME files.

The launcher starts the validated Windows x64 game executable and loads enabled DLLs directly. No Tempest installation is required. Existing Tempest-compatible profile and enabled-mod settings are retained.

See [version 0.3.7 verification](docs/verification-0.3.7.md) for the recorded checks and their limits.

## Development

Windows x64 and Rust are required. See [CONTRIBUTING.md](CONTRIBUTING.md) and [architecture](docs/architecture.md).

## Fork source

This launcher is a Rust adaptation of the launch workflow from [LowRezStudio/Tempest](https://github.com/LowRezStudio/Tempest), our fork source. Game dispatch and DLL loading are implemented in this repository. It does not include Tempest's Svelte frontend, C# CLI or Zig injector source.

The launch contract was reviewed at upstream revision `efe828dd28a48c4e1861ebf25a99cc16bd4b2425`. Changes are maintained independently; upstream improvements are reviewed and ported into Rust. This repository is not currently a GitHub-network fork of the upstream monorepo.

GPL-3.0-or-later. Tempest copyright and third-party notices are retained in [NOTICE.md](NOTICE.md).
