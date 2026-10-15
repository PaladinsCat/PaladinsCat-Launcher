//! Public package sources shared by download code, release tooling and feature tabs.

pub const LAUNCHER_REPOSITORY: &str = "PaladinsCat/PaladinsCat-Launcher";
pub const LAUNCHER_RELEASE_API: &str =
    "https://api.github.com/repos/PaladinsCat/PaladinsCat-Launcher/releases/latest";

pub const LOCALE_REPOSITORY: &str = "PaladinsCat/PaladinsCat-locales";
pub const PERFORMANCE_REPOSITORY: &str = "PaladinsCat/PaladinsCat-mods";
pub const PERFORMANCE_RELEASE_API: &str =
    "https://api.github.com/repos/PaladinsCat/PaladinsCat-mods/releases/latest";
pub const PERFORMANCE_RELEASE_URL: &str =
    "https://github.com/PaladinsCat/PaladinsCat-mods/releases/latest";

pub fn repository_url(repository: &str) -> String {
    format!("https://github.com/{}", repository.trim())
}

pub fn catalog_url(repository: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/main/game-client/catalog.json",
        repository.trim()
    )
}

pub fn performance_asset_url(version: &str, name: &str) -> String {
    format!("https://github.com/{PERFORMANCE_REPOSITORY}/releases/download/v{version}/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_urls_keep_their_own_repository_and_release_contracts() {
        assert!(catalog_url(LOCALE_REPOSITORY).contains("PaladinsCat-locales/main/game-client/"));
        assert!(repository_url(PERFORMANCE_REPOSITORY).ends_with("/PaladinsCat-mods"));
        assert_eq!(
            performance_asset_url("0.1.2", "manifest.json"),
            "https://github.com/PaladinsCat/PaladinsCat-mods/releases/download/v0.1.2/manifest.json"
        );
        assert!(!PERFORMANCE_RELEASE_API.contains("PaladinsCat-locales"));
    }
}
