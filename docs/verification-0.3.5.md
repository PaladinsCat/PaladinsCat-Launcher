# Launcher 0.3.5 navigation

Performance is first and selected on startup. Localization contains the package
language selector. `KOR` reveals Translation and Korean IME patch subpages;
other languages reveal no IME option. The selector is visible on either Korean
subpage. Switching away from Korean returns to Translation without changing
the installed IME state, prepared package or saved launch settings.

Verified 2026-10-12:

- 79 locked tests passed; the existing optional performance network test remains
  ignored. The actual egui-frame regression covers menu ordering, Korean-only
  visibility, language-selector placement and unchanged IME configuration.
- Formatting, strict all-target Clippy, release build, source audit and the
  compiled PaladinsCat icon check passed.
- Prepared Performance, Korean Translation and Korean IME CPU previews were
  rendered and visually inspected. Localization retains vertical scrolling for
  controls below the initial viewport.
- WatchCat's required script passed 40 tests and its configured gates using the
  existing local Cargo target.

These checks neither install packages nor change game files. They do not establish
live GUI rendering or runtime hook behavior. Those require their own current
human launch instruction and instance evidence. Package preview and translation
coverage status are unchanged by this navigation update.
