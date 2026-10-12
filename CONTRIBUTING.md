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
cargo build --locked --release --bin paladinscat-launcher --bin paladinscat-release
cargo run --locked --example verify_icon -- target/release/paladinscat-launcher.exe
```

Run `paladinscat-launcher --preview OUTPUT.png`, `--preview-ime OUTPUT.png` or `--preview-performance OUTPUT.png` for a CPU layout preview. These do not open or capture a desktop window. Unit tests use temporary fixtures; they do not install game packages or load native DLLs.

`assets/paladinscat.png` supplies both the native window icon and the generated Windows executable icon. The build wraps the original PNG in an ICO under Cargo's output directory; do not maintain a second logo asset. `verify_icon` reads the compiled PE icon as data and compares it with the original PNG without starting the launcher.

Do not commit downloaded packages, generated binaries, recordings, credentials or game files. Review and pin immutable package manifests and payload hashes before updating a release. Preserve existing game receipts and cache paths when changing product names. Keep launch selection distinct from verified runtime hook activation. No successful dry-run, build or CPU preview proves gameplay compatibility or performance.

The current graphics policy preserves the developer's exact RTX 5080 hardware selection requirement. CPU checks do not establish live GPU routing. Changes to the public adapter-selection policy need an explicit reviewed design.
