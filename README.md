# PaladinsCat Launcher

Localization, Korean IME compatibility and performance mods for Paladins.

## Use

1. Select your Paladins game folder and installed Tempest CLI.
2. Download a localization package and apply it while the game is closed.
3. Choose the IME and performance options, then select **Play**.

Localization packages come from [PaladinsCat-locales](https://github.com/PaladinsCat/PaladinsCat-locales).
Performance packages come from [PaladinsCat-mods](https://github.com/PaladinsCat/PaladinsCat-mods).
Use **Restore originals** to revert localization. Disable performance for the next launch, or select its retained input version. Close the game before changing installed IME files.

This initial source release retains the existing GUI's RTX 5080 requirement. Performance gains and combined IME/performance gameplay compatibility have not been verified. See [verification](docs/verification-0.3.0.md) for the tested boundaries.

## Development

Windows x64 and Rust are required. See [CONTRIBUTING.md](CONTRIBUTING.md) and [architecture](docs/architecture.md).

## Fork source

This launcher is a Rust adaptation of the launch workflow from [LowRezStudio/Tempest](https://github.com/LowRezStudio/Tempest), our fork source. It uses the installed Tempest CLI for game launch and DLL loading. It does not include Tempest's Svelte frontend, C# CLI or Zig injector source.

The launch contract was reviewed at upstream revision `efe828dd28a48c4e1861ebf25a99cc16bd4b2425`. Changes are maintained independently; upstream improvements are reviewed and ported into Rust. This repository is not currently a GitHub-network fork of the upstream monorepo.

GPL-3.0-or-later. Tempest copyright and third-party notices are retained in [NOTICE.md](NOTICE.md).
