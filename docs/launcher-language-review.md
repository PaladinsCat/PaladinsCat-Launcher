# Launcher language review

Implemented 2026-10-13 in the existing main checkout. Korean wording is an initial general translation, awaiting the owner's review.

The header selector supports ENG and KOR, displays English and 한국어 in its menu, and saves `ui_language` independently of the game language/profile/package selections. Its globe outline, 36-pixel trigger, 16-pixel icon, 12-pixel code/chevron, right alignment, dark popup, rounded rows and selected color follow the PaladinsCat frontend's `components/nav.tsx` LanguageMenu and supplied reference. The language codes use the requested ENG/KOR labels.

Review wording in [src/i18n.rs](../src/i18n.rs): the exact English/Korean pairs and anchored status templates are together. All visible section headings, controls, file-dialog captions, common progress/install/recovery states and launcher error captions are covered. Brand/package identities, paths, release notes and detailed native/OS diagnostics retain their contents.

CPU previews generated from the actual egui layout are under `target/release`:

- `launcher-menu-eng-20261013.png`
- `launcher-menu-kor-20261013.png`
- `launcher-eng-20261013.png`
- `launcher-performance-kor-20261013.png`
- `launcher-translation-kor-20261013.png`
- `launcher-ime-kor-20261013.png`

Headless interaction checks exercise opening, ENG → KOR → ENG switching, selection dismissal, Escape/outside dismissal, header placement, settings round-trip, existing game/package preservation, and switching combined startup failures without changing diagnostic payloads. The locked all-target tests passed (95 passed, two intentionally ignored), as did formatting, strict Clippy, the release build, exact-source executable icon verification, and the public source boundary audit. Korean menu/Translation/IME previews were visually inspected; the popup animation was allowed to settle before saving.

Prepared `target/release/paladinscat-launcher.exe` is 18,597,376 bytes, SHA256 `3f424d028d847196ce6668537c49cde2c8dbb86cdfbf2128d71f156ca3737918`. The user's existing saved settings were byte-identical before and after verification.

Preview rendering uses CPU rasterization. The live launcher was not restarted, and these images do not establish live GPU routing. No game launch, package installation, settings migration on the user's existing file, publication, commit, branch, worktree or PR was performed for this UI change.

## Layout and footer correction, 2026-10-13

Repository links, publication counts, refresh and settings now sit in a bottom
section that starts collapsed. Localization and its package-language selector
share one navigation row in ENG and KOR, including the IME page. Game language
uses an INT/KOR dropdown and preserves an existing saved locale code.

The footer accepts only fixed short localized states. An observed game launch
shows `Game started`; a requested launch without observation shows `Launch requested`.
Process IDs, initialization caveats, paths and raw errors require expansion of
the bottom details. Full backend/CLI diagnostics remain available. Regression
checks exercise observed/pending launch, arbitrary errors/progress, disclosure
and preserved diagnostic payloads in both interface languages.

Launcher verification: 99 tests passed, two intentionally ignored; formatting,
strict all-target Clippy, release build, exact embedded icon and public source
boundary audit passed (58 worktree files, nine revisions). Four actual egui CPU
previews were inspected under `target/ui-layout/release`: `layout-eng.png`,
`layout-kor.png`, `layout-ime-kor.png` and `layout-performance-eng.png`.

The standard output path is occupied by the existing launcher. The verified
corrected executable is `target/ui-layout/release/paladinscat-launcher.exe`,
18,605,056 bytes, SHA256 `4ba8b73e1b46a7777f7afe4cdfd0ba9a3ed9412b5b72d56c3c5c4ecfb0a7e5f9`.
Saved settings retain SHA256 `6a9ab61c1eef05dcbcbe0b9b662519a6b6cceaaa90e335f91db39d58374f678b`.
The open GUI was preserved.
