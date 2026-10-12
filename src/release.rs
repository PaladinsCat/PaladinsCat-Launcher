//! Local release preparation only. Never pushes, publishes or applies a package.
mod release_assets;
use paladinscat_launcher::{engine, ime};
use sha2::{Digest, Sha256};
use std::{
    env,
    path::{Path, PathBuf},
};

fn repository(path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !root.join(".git").exists() || !root.join("game-client/source-manifest.json").is_file() {
        return Err("Select the existing paladinscat-locales repository".into());
    }
    Ok(root)
}
fn validate(root: &Path) -> Result<(), String> {
    let read = |path: &Path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let catalog = engine::parse_catalog(&read(&root.join("game-client/catalog.json"))?)?;
    let native = root.join("game-client/native/translations");
    let mut count = 0;
    for entry in std::fs::read_dir(&native).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_none_or(|v| v != "json") {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_slice(&read(&path)?).map_err(|e| e.to_string())?;
        for field in ["names_file", "lore_file", "skills_file"] {
            if let Some(file) = value[field].as_str() {
                let dependency = native
                    .join(file)
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if !dependency.starts_with(root) {
                    return Err("Native dictionary dependency escapes locale repository".into());
                }
                let _: serde_json::Value =
                    serde_json::from_slice(&read(&dependency)?).map_err(|e| e.to_string())?;
            }
        }
        if let Some(pins) = value["approved_name_decisions"].as_array() {
            for pin in pins {
                let dependency = native
                    .join(pin["file"].as_str().ok_or("Approval file absent")?)
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if !dependency.starts_with(root) {
                    return Err("Approval receipt escapes locale repository".into());
                }
                let bytes = read(&dependency)?;
                if format!("{:x}", Sha256::digest(&bytes))
                    != pin["sha256"].as_str().ok_or("Approval hash absent")?
                {
                    return Err("Approval receipt hash mismatch".into());
                }
                let decision: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if decision["approved"] != true || !decision["caption_targets"].is_object() {
                    return Err("Native name receipt is not approved".into());
                }
            }
        }
        count += 1;
    }
    if count == 0 {
        return Err("No native authoring inputs found".into());
    }
    println!(
        "Validated {count} native inputs and their dependencies; catalog has {} locale / {} IME releases. No files changed.",
        catalog.packages.len(),
        catalog.ime_packages.len()
    );
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    if let [
        mode,
        bundle,
        ime_manifest,
        repo,
        tag,
        locale_commit,
        tool_commit,
    ] = args.as_slice()
        && mode == "assemble-preview"
    {
        let output = release_assets::assemble(
            Path::new(bundle),
            Path::new(ime_manifest),
            &repository(repo)?,
            tag,
            locale_commit,
            tool_commit,
        )?;
        println!(
            "Prepared immutable preview assets at {}; nothing published or installed",
            output.display()
        );
        return Ok(());
    }
    if let [mode, url, cache] = args.as_slice()
        && mode == "verify-downloads"
    {
        let catalog = engine::check_catalog(url)?;
        if catalog.packages.is_empty() || catalog.ime_packages.is_empty() {
            return Err(
                "Both translation and IME releases are required for this readiness check".into(),
            );
        }
        for release in &catalog.packages {
            let c = engine::download(release, Path::new(cache), &|p| println!("{p}"))?;
            println!("Verified remote translation {}", c.package.bundle_id);
        }
        for release in &catalog.ime_packages {
            let c = ime::download(release, Path::new(cache), &|p| println!("{p}"))?;
            println!("Verified remote IME {}", c.package.version);
        }
        return Ok(());
    }
    if let [mode, url] = args.as_slice()
        && mode == "use-catalog"
    {
        let catalog = engine::check_catalog(url)?;
        let locale = catalog
            .packages
            .first()
            .ok_or("Catalog has no translation package")?;
        let ime_release = catalog
            .ime_packages
            .first()
            .ok_or("Catalog has no IME package")?;
        let cache = engine::data_dir();
        let locale = engine::download(locale, &cache, &|p| println!("{p}"))?;
        let ime = ime::download(ime_release, &cache, &|p| println!("{p}"))?;
        let path = cache.join("settings.json");
        let mut settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if !settings.is_object() {
            return Err("GUI settings must be an object".into());
        }
        settings["catalog_url"] = url.clone().into();
        settings["prepared_translation"] = locale
            .directory
            .join("bundle.json")
            .display()
            .to_string()
            .into();
        settings["prepared_ime"] = ime
            .directory
            .join("ime-package.json")
            .display()
            .to_string()
            .into();
        engine::save_settings(&path, &settings)?;
        println!(
            "Configured verified GitHub catalog and both prepared packages; no game files changed"
        );
        return Ok(());
    }
    if let [mode, bundle, ime_manifest] = args.as_slice()
        && mode == "stage-gui"
    {
        let cache = engine::data_dir();
        let translation =
            engine::import(Path::new(bundle), &cache, &|message| println!("{message}"))?;
        let ime_package = ime::import(Path::new(ime_manifest), &cache)?;
        let settings_path = cache.join("settings.json");
        let mut settings: serde_json::Value = if settings_path.exists() {
            serde_json::from_slice(&std::fs::read(&settings_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        } else {
            serde_json::json!({"game_folder":"", "repository":paladinscat_launcher::sources::LOCALE_REPOSITORY, "catalog_url":paladinscat_launcher::sources::catalog_url(paladinscat_launcher::sources::LOCALE_REPOSITORY)})
        };
        if !settings.is_object() {
            return Err("GUI settings must be an object".into());
        }
        settings["prepared_translation"] = translation
            .directory
            .join("bundle.json")
            .display()
            .to_string()
            .into();
        settings["prepared_ime"] = ime_package
            .directory
            .join("ime-package.json")
            .display()
            .to_string()
            .into();
        engine::save_settings(&settings_path, &settings)?;
        println!(
            "Prepared {} and IME {} for the GUI; all payloads verified. No game files changed.",
            translation.package.bundle_id, ime_package.package.version
        );
        return Ok(());
    }
    if let [mode, path] = args.as_slice()
        && mode == "validate"
    {
        return validate(&repository(path)?);
    }
    if args.len() != 4 || args[0] != "ime" {
        return Err("Usage: paladinscat-release ime DLL LOCALES_REPO MAJOR.MINOR.PATCH | validate LOCALES_REPO | stage-gui BUNDLE_JSON IME_PACKAGE_JSON".into());
    }
    let repository = repository(&args[2])?;
    let output = repository
        .join("game-client/.release-staging/ime")
        .join(&args[3]);
    let package = ime::prepare(Path::new(&args[1]), &output, &args[3])?;
    println!(
        "Prepared local draft {} at {}. No catalog publication or game changes.",
        package.package_id,
        output.display()
    );
    Ok(())
}
