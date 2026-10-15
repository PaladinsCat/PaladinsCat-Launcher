//! Read-only public release check and exact dependency-source index generation.
use paladinscat_launcher::{self_update, sources};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, time::Duration};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["source-index"] {
        let lock = fs::read("Cargo.lock")?;
        let text = std::str::from_utf8(&lock)?;
        let mut packages = Vec::new();
        for block in text.split("[[package]]").skip(1) {
            let field = |key: &str| {
                block.lines().find_map(|line| {
                    let (name, value) = line.split_once('=')?;
                    (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
                })
            };
            let Some(source) = field("source") else {
                continue;
            };
            if source != "registry+https://github.com/rust-lang/crates.io-index" {
                return Err("Dependency requires an explicit source location".into());
            }
            let name = field("name").ok_or("Missing dependency name")?;
            let version = field("version").ok_or("Missing dependency version")?;
            let checksum = field("checksum").ok_or("Missing source checksum")?;
            if !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
                || checksum.len() != 64
                || !checksum.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Invalid locked dependency identity".into());
            }
            packages.push(json!({"name":name,"version":version,"sha256":checksum,
                "source_url":format!("https://static.crates.io/crates/{name}/{name}-{version}.crate")}));
        }
        if packages.is_empty() {
            return Err("No locked dependency sources".into());
        }
        let body = json!({"launcher_version":env!("CARGO_PKG_VERSION"),
            "cargo_lock_sha256":hash(&lock),"dependencies":packages});
        let path = Path::new("docs/dependency-sources.json");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        use std::io::Write;
        file.write_all(serde_json::to_string_pretty(&body)?.as_bytes())?;
        file.write_all(b"\n")?;
        println!("Source index: {} locked dependencies", packages.len());
        return Ok(());
    }
    let [mode, local] = args.as_slice() else {
        return Err("Use source-index or public LOCAL_EXE".into());
    };
    if mode != "public" {
        return Err("Unknown mode".into());
    }
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout(Duration::from_secs(120))
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .build();
    let get = |url: &str, limit: u64| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();
        agent
            .get(url)
            .set("User-Agent", "PaladinsCat-release-verification")
            .call()?
            .into_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err("Public response too large".into());
        }
        Ok(bytes)
    };
    let metadata = get(sources::LAUNCHER_RELEASE_API, 2 * 1024 * 1024)?;
    let self_update::Check::Available(release) = self_update::parse_release(&metadata, "0.0.0")?
    else {
        return Err("Published release unavailable".into());
    };
    if release.version != env!("CARGO_PKG_VERSION") {
        return Err("Wrong published version".into());
    }
    let metadata_json: serde_json::Value = serde_json::from_slice(&metadata)?;
    if metadata_json["assets"]
        .as_array()
        .ok_or("Missing assets")?
        .len()
        != 1
    {
        return Err("EXE-only release must have exactly one uploaded asset".into());
    }
    let url = format!(
        "https://github.com/{}/releases/download/v{}/{}",
        sources::LAUNCHER_REPOSITORY,
        release.version,
        self_update::EXE
    );
    let bytes = get(&url, 128 * 1024 * 1024)?;
    let local_bytes = fs::read(local)?;
    self_update::validate_executable(&bytes, &release.version)?;
    if bytes.len() as u64 != release.bytes || hash(&bytes) != release.sha256 || bytes != local_bytes
    {
        return Err("Public download/local EXE/metadata mismatch".into());
    }
    if !matches!(self_update::check()?, self_update::Check::Current) {
        return Err("Current launcher did not recognize its public release".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"passed":true,
        "version":release.version,"sha256":release.sha256,"bytes":release.bytes,
        "uploaded_assets":1,"public_download_matches_local":true,"pe_version_valid":true,
        "updater_previous_version":"available","updater_current_version":"current"}))?
    );
    Ok(())
}
