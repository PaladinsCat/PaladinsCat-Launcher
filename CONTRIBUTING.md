# Contributing

Use Rust. The owner's agent workflow stays in the existing `main` checkout and
does not create branches, worktrees, forks or pull requests. Review the source
and run the checks below before an authorized publication. Remote protections
remain binding. External community contributions use owner-reviewed pull requests;
that procedure does not authorize an agent working for the owner to create one.

`rust-toolchain.toml` pins Rust 1.97.0 with Clippy/rustfmt. CI uses the same version;
upgrade the source and CI pins together under the applicable owner review workflow.

```text
cargo fmt --all --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bin paladinscat-launcher --bin paladinscat-launcher-cli --bin paladinscat-release
cargo run --locked --example verify_icon -- target/release/paladinscat-launcher.exe
```

Run `paladinscat-launcher --preview OUTPUT.png`, `--preview-ime OUTPUT.png` or `--preview-performance OUTPUT.png` for a CPU layout preview. These do not open or capture a desktop window. Unit tests use temporary fixtures; they do not install game packages or load downloaded/game DLLs. Loader tests use owned synthetic Rust DLL fixtures.

`--preview-details OUTPUT.png` and `--preview-details-kor OUTPUT.png` render the
expanded repo panel with a synthetic launch result to review compact ENG/KOR copy.

`assets/paladinscat.png` supplies both the native window icon and the generated Windows executable icon. The build wraps the original PNG in an ICO under Cargo's output directory; do not maintain a second logo asset. `verify_icon` reads the compiled PE icon as data and compares it with the original PNG without starting the launcher.

Do not commit downloaded packages, generated binaries, recordings, credentials or game files. Keep `dist/` to one most-recent verified staging version; do not create per-test version directories. Recycle an older staged version only after its successor passes the checks above. Review and pin immutable package manifests and payload hashes before updating a release. Preserve existing game receipts and cache paths when changing product names. Keep launch selection distinct from verified runtime hook activation. No successful dry-run, build or CPU preview proves gameplay compatibility or performance.

The public graphics policy selects supported hardware before device allocation and refuses software fallback. An explicit per-user `graphics-policy.json` RTX 5080 pin retains strict vendor/device/name/PCI checks on the developer workstation. CPU checks do not establish live GPU routing.

For a prepared portable release, freeze the reviewed source on the existing
`main` HEAD, vendor the locked dependencies under `target/release/vendor`, then
run `paladinscat-release prepare-launcher RELEASE_BIN_DIR dist/VERSION SOURCE_COMMIT CATALOG_JSON`.
Use the exact HEAD commit and a verified catalog. Preparation creates a portable
ZIP, matching source ZIP with dependency sources, catalog snapshot, manifest and
checksums. It verifies archive members and refuses stale binaries, changed source
or an existing output version. Preparation does not publish a GitHub release.
