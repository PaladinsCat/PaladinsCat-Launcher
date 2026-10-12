//! Public package sources shared by download code, release tooling and feature tabs.

pub const LOCALE_REPOSITORY: &str = "PaladinsCat/PaladinsCat-locales";
pub const PERFORMANCE_REPOSITORY: &str = "PaladinsCat/PaladinsCat-mods";
pub const PERFORMANCE_VERSION: &str = "0.1.0";
pub const PERFORMANCE_RELEASE_URL: &str =
    "https://github.com/PaladinsCat/PaladinsCat-mods/releases/tag/v0.1.0";
pub const PERFORMANCE_BASE_URL: &str =
    "https://github.com/PaladinsCat/PaladinsCat-mods/releases/download/v0.1.0";

pub fn repository_url(repository: &str) -> String {
    format!("https://github.com/{}", repository.trim())
}

pub fn catalog_url(repository: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/main/game-client/catalog.json",
        repository.trim()
    )
}

pub fn performance_asset_url(name: &str) -> String {
    format!("{PERFORMANCE_BASE_URL}/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_urls_keep_their_own_repository_and_release_contracts() {
        assert!(catalog_url(LOCALE_REPOSITORY).contains("PaladinsCat-locales/main/game-client/"));
        assert!(repository_url(PERFORMANCE_REPOSITORY).ends_with("/PaladinsCat-mods"));
        assert_eq!(
            performance_asset_url("manifest.json"),
            format!("{PERFORMANCE_BASE_URL}/manifest.json")
        );
        assert!(!PERFORMANCE_BASE_URL.contains("PaladinsCat-locales"));
    }
}
