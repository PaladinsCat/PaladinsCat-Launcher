# Performance releases

The launcher discovers the latest stable release in the Performance repository.
GitHub supplies each release asset's digest and size; the verified manifest
supplies package members and game/Windows compatibility fingerprints. Package
versions, payload checksums and compatibility values are never compiled into the
launcher. The displayed version and manifest link use the discovered or prepared
package.

Downloads verify the immutable release URLs, all asset digests and sizes, archive
membership, embedded manifest, and checksum inventories before preparation. A
prepared generation keeps its verified release metadata for offline startup.
Imports from outside that owned cache are verified against the corresponding
GitHub release. No package helper or DLL is executed during verification.

Regression checks cover future compatible versions, wrong sources, missing
digests, duplicate assets, malformed archives, changed native files, offline
restore and compiled release data. Release validation also exercises the actual
GitHub download/import and both input variants without starting a game.
