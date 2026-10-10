//! Immutable preview preparation; no publication or game installation.
use paladinscat_launcher::{engine, ime};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn sha(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 131072];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn assemble(
    bundle: &Path,
    ime_manifest: &Path,
    repository: &Path,
    tag: &str,
    locale_commit: &str,
    tooling_commit: &str,
) -> Result<PathBuf, String> {
    if tag.is_empty()
        || tag.len() > 100
        || !tag
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        || [locale_commit, tooling_commit]
            .iter()
            .any(|v| v.len() != 40 || !v.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("Use an immutable safe tag and exact source commits".into());
    }
    let cache = engine::data_dir();
    let cached = engine::import(bundle, &cache, &|p| println!("{p}"))?;
    let ime_cached = ime::import(ime_manifest, &cache)?;
    let output = repository
        .join("game-client/.release-staging/releases")
        .join(tag);
    if output.exists() {
        return Err(
            "Release preparation directory already exists; never replace immutable assets".into(),
        );
    }
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let base =
        format!("https://github.com/PaladinsCat/PaladinsCat-locales/releases/download/{tag}");
    let mut package = cached.package;
    package.source_commit = locale_commit.into();
    package.game_build = ime_cached.package.game_build.clone();
    package.game_fingerprint = Some(ime_cached.package.game_fingerprint.clone());
    package.min_tool_version = Some([0, 2, 0]);
    package.notes = "PREVIEW · Korean v172 with Tmoney RoundWind Regular. Approved repairs only; remaining translation and targeted visual reviews are incomplete. Supports the pinned Win64 8.1 client only.".into();
    for payload in &mut package.files {
        let name = Path::new(&payload.source)
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("Invalid asset filename")?;
        let asset = format!("{name}.gz");
        let destination = output.join(&asset);
        if destination.exists() {
            return Err("Duplicate release asset filename".into());
        }
        let mut input =
            File::open(cached.directory.join(&payload.source)).map_err(|e| e.to_string())?;
        let mut encoder = flate2::write::GzEncoder::new(
            File::create(&destination).map_err(|e| e.to_string())?,
            flate2::Compression::default(),
        );
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0; 131072];
        loop {
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
            encoder.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            bytes += n as u64;
        }
        encoder
            .finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        if format!("{:x}", digest.finalize()) != payload.artifact_sha256 {
            return Err("Payload changed during release assembly".into());
        }
        payload.artifact_bytes = Some(bytes);
        payload.download = Some(engine::Download {
            url: format!("{base}/{asset}"),
            sha256: sha(&destination)?,
            bytes: fs::metadata(&destination).map_err(|e| e.to_string())?.len(),
            encoding: "gzip".into(),
        });
        println!(
            "Prepared {asset}: {} bytes",
            payload.download.as_ref().unwrap().bytes
        );
    }
    engine::save_settings(&output.join("locale-package.json"), &package)?;
    let mut ime_package = ime_cached.package;
    ime_package.draft = false;
    ime_package.source_commit = tooling_commit.into();
    ime_package.notes = "PREVIEW · Korean IME compatibility for pinned Win64 8.1. Esc, rapid-I and Korean typing passed on frozen v4-r1; this portable build and live disable still require a fresh client trial.".into();
    ime_package.download = Some(engine::Download {
        url: format!("{base}/{}", ime::DLL),
        sha256: ime_package.dll_sha256.clone(),
        bytes: ime_package.dll_bytes,
        encoding: "raw".into(),
    });
    fs::copy(ime_cached.directory.join(ime::DLL), output.join(ime::DLL))
        .map_err(|e| e.to_string())?;
    engine::save_settings(&output.join("ime-package.json"), &ime_package)?;
    // Actual PE/ABI, identity and payload verification on the finalized public manifest.
    ime::import(&output.join("ime-package.json"), &cache)?;
    let catalog = serde_json::json!({"schema_version":1,
        "packages":[engine::Release {bundle_id:package.bundle_id.clone(),locale:package.locale.clone(),game_build:package.game_build.clone(),manifest_url:format!("{base}/locale-package.json"),manifest_sha256:sha(&output.join("locale-package.json"))?,notes:package.notes.clone()}],
        "ime_packages":[ime::Release {package_id:ime_package.package_id,version:ime_package.version,game_build:ime_package.game_build,manifest_url:format!("{base}/ime-package.json"),manifest_sha256:sha(&output.join("ime-package.json"))?,notes:ime_package.notes}]
    });
    engine::parse_catalog(&serde_json::to_vec(&catalog).map_err(|e| e.to_string())?)?;
    engine::save_settings(&output.join("catalog.json"), &catalog)?;
    engine::save_settings(
        &output.join("provenance.json"),
        &serde_json::json!({"schema_version":1,"channel":"preview","locale_source_commit":locale_commit,"ime_source_commit":tooling_commit,"packaging_binary_sha256":sha(&std::env::current_exe().map_err(|e|e.to_string())?)?,"native_bundle_id":package.bundle_id,"localization_complete":false,"portable_ime_live_verified":false,"game_files_changed":false}),
    )?;
    Ok(output)
}
