# Replay files and game profiles

The launcher defaults to language `INT` and profile `PaladinsCat`. The profile is forwarded through Tempest's `--homedir`, which becomes the game's `-homedir` argument. Existing explicit settings remain selected.

## Storage and playback

Wolper Replay's installed documentation specifies `.demo` recordings in the normal game `ChaosGame\Demos` directory. It exposes them on its Replay page and requires returning to the main menu before starting playback. On Windows, the observed game-home layout is:

```text
<Documents>\My Games\<profile>\ChaosGame\Demos
```

Use the Windows Documents known folder; it can be redirected. Recordings belong to the selected profile, while Tempest's enabled mod registry belongs to the game installation. A loaded replay mod does not make another profile's recording library available.

Changing profiles does not migrate recordings. The new `PaladinsCat` home can therefore initially have no recordings even when another profile contains them. Different profile settings and client/mod builds may also affect playback; path isolation alone does not establish the cause of a particular failed load.

## Working with an existing recording

Use the original profile to access its library. To reuse a recording in another profile, explicitly copy/import the selected `.demo` into that profile's `ChaosGame\Demos`, retain the original, and verify the copy's hash. Refuse a different destination file with the same name rather than overwriting it. Return to the main menu and select the recording through Wolper's Replay page. Keep the same client/mod build for comparison.

No automatic migration or replay importer is implemented in this launcher version. Native absolute-path playback support has not been established and must not be advertised as a workaround.

## Source evidence

Tempest source inspected at `01081efe3cd966dbc8f84822640b10f12f3f4158`:

- [Frontend profile selection](https://github.com/LowRezStudio/Tempest/blob/01081efe3cd966dbc8f84822640b10f12f3f4158/Tempest.Launcher/src/lib/core/launch.ts#L29-L36): its Season 8.1 frontend selects `Paladins`; other versions derive a sanitized name.
- [CLI game arguments](https://github.com/LowRezStudio/Tempest/blob/01081efe3cd966dbc8f84822640b10f12f3f4158/Tempest.CLI/Launcher/LauncherCommands.cs#L96-L109): with default arguments enabled, forwards the selected homedir; a missing value defaults to `Tempest`.
- [Mod discovery](https://github.com/LowRezStudio/Tempest/blob/01081efe3cd966dbc8f84822640b10f12f3f4158/Tempest.CLI/Launcher/LauncherCommands.cs#L147-L160): mod loading is tied to the game installation.

The installed Wolper Replay 3.1.0 README, lines 5–8, supplies the storage and playback instructions. Its package supplied no native loader source or source URL. The audit checked files and launch logs without launching the game or changing recordings. This does not change the launch-contract upstream revision pinned elsewhere in this repository.
