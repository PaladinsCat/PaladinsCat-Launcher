# Package version labels

The product shows a numeric revision and client build, such as `v177 · 8.1`.
It never shows task descriptions such as gold, objective, approved or messages,
or a content hash as part of that label. The same formatter serves catalog
choices and locally prepared packages, including legacy manifests.

Keep complete package identities unchanged in cache ownership, receipts, hash
checks and diagnostics. A presentation correction must not rename an installed
receipt or change game bytes. New locale publications use a clean version/hash
identity and a plain release title such as `Korean v177`; translation details
belong in release notes. Preserve old immutable metadata and issue a new metadata
revision when a pinned manifest needs correction.
