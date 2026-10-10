# Contributing

Use Rust and submit a pull request to protected `main`. Changes require owner review after repository bootstrap.

```text
cargo fmt --all --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bin paladinscat-launcher --bin paladinscat-release
```

Run `paladinscat-launcher --preview OUTPUT.png`, `--preview-ime OUTPUT.png` or `--preview-performance OUTPUT.png` for a CPU layout preview. These do not open or capture a desktop window. Unit tests use temporary fixtures; they do not install game packages or load native DLLs.

Do not commit downloaded packages, generated binaries, recordings, credentials or game files. Review and pin immutable package manifests and payload hashes before updating a release. Preserve existing game receipts and cache paths when changing product names. Keep launch selection distinct from verified runtime hook activation. No successful dry-run, build or CPU preview proves gameplay compatibility or performance.

The current graphics policy preserves the developer's exact RTX 5080 hardware selection requirement. CPU checks do not establish live GPU routing. Changes to the public adapter-selection policy need an explicit reviewed design.
