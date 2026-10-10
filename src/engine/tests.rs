use super::*;
use sha2::{Digest, Sha256};
use std::sync::{
    Barrier,
    atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Fixture {
    home: PathBuf,
    game: PathBuf,
    cache: PathBuf,
    target: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "paladinscat-patcher-fixture-{}-{}-{}",
            std::process::id(),
            stamp(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let game = home.join("game");
        let cache = home.join("cache");
        let target = game.join("ChaosGame/Localization/KOR/Lang_KOR.dat");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"original").unwrap();
        Self {
            home,
            game,
            cache,
            target,
        }
    }
    fn package(&self, name: &str, bytes: &[u8]) -> Cached {
        let source = self.home.join(name);
        fs::create_dir_all(&source).unwrap();
        let package = Package {
            version: 1,
            bundle_id: name.into(),
            locale: "KOR".into(),
            game_build: "fixture-build".into(),
            platform: platform(),
            source_commit: String::new(),
            notes: String::new(),
            min_tool_version: None,
            game_fingerprint: None,
            files: vec![Payload {
                source: "Lang_KOR.dat".into(),
                target: "ChaosGame/Localization/KOR/Lang_KOR.dat".into(),
                stock_sha256: digest(b"original"),
                artifact_sha256: digest(bytes),
                artifact_bytes: Some(bytes.len() as u64),
                download: None,
            }],
        };
        fs::write(source.join("Lang_KOR.dat"), bytes).unwrap();
        save(&source.join("bundle.json"), &package).unwrap();
        import(&source.join("bundle.json"), &self.cache, &|_| {}).unwrap()
    }
    fn action(&self, action: Action, package: Option<Cached>) -> Result<String> {
        transition(action, &self.game, package, &self.cache, &|| false, &|_| {})
    }
}

#[test]
fn parallel_fixtures_have_independent_game_files() {
    let fixtures = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..32).map(|_| scope.spawn(Fixture::new)).collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    let homes: HashSet<_> = fixtures.iter().map(|f| &f.home).collect();
    assert_eq!(homes.len(), fixtures.len());
    for (index, fixture) in fixtures.iter().enumerate() {
        fs::write(&fixture.target, index.to_string()).unwrap();
    }
    for (index, fixture) in fixtures.iter().enumerate() {
        assert_eq!(
            fs::read(&fixture.target).unwrap(),
            index.to_string().as_bytes()
        );
    }
}

#[test]
fn concurrent_json_writers_in_one_directory_preserve_each_manifest() {
    let f = Fixture::new();
    let directory = f.home.join("manifests");
    fs::create_dir(&directory).unwrap();
    let barrier = Barrier::new(16);
    for writer in 0..16 {
        save(&directory.join(format!("{writer}.json")), &"initial").unwrap();
    }
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..16)
            .map(|writer| {
                let directory = &directory;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for generation in 0..8 {
                        save(
                            &directory.join(format!("{writer}.json")),
                            &serde_json::json!({"writer": writer, "generation": generation}),
                        )
                        .unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    for writer in 0..16 {
        let value: serde_json::Value = json(&directory.join(format!("{writer}.json"))).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"writer": writer, "generation": 7})
        );
    }
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 16);
}

#[test]
fn clean_install_update_rollback_restore_preserves_originals() {
    let f = Fixture::new();
    let one = f.package("version-one", b"translation one");
    let two = f.package("version-two", b"translation two");
    f.action(Action::Apply, Some(one)).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"translation one");
    f.action(Action::Apply, Some(two)).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"translation two");
    f.action(Action::Rollback, None).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"translation one");
    f.action(Action::Restore, None).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"original");
    assert!(!recovery_pending(&f.game));
    assert_eq!(installed(&f.game).unwrap(), "Original game files");
}
#[test]
fn changed_game_file_is_retained_and_update_refused() {
    let f = Fixture::new();
    let one = f.package("version-one", b"one");
    let two = f.package("version-two", b"two");
    f.action(Action::Apply, Some(one)).unwrap();
    fs::write(&f.target, b"external modification").unwrap();
    assert!(f.action(Action::Apply, Some(two)).is_err());
    assert_eq!(fs::read(&f.target).unwrap(), b"external modification");
    assert!(!recovery_pending(&f.game));
}
#[test]
fn incompatible_game_fingerprint_blocks_before_transaction() {
    let f = Fixture::new();
    let mut package = f.package("version-one", b"one");
    package.package.game_fingerprint = Some(Fingerprint {
        path: "ChaosGame/Localization/KOR/Lang_KOR.dat".into(),
        sha256: digest(b"different build"),
    });
    save(&package.directory.join("bundle.json"), &package.package).unwrap();
    assert!(
        f.action(Action::Apply, Some(package))
            .unwrap_err()
            .contains("game build")
    );
    assert_eq!(fs::read(&f.target).unwrap(), b"original");
    assert!(!recovery_pending(&f.game));
}
#[test]
fn corrupted_cache_is_refused_without_game_changes() {
    let f = Fixture::new();
    let package = f.package("version-one", b"one");
    fs::write(package.directory.join("Lang_KOR.dat"), b"tamper").unwrap();
    assert!(f.action(Action::Apply, Some(package)).is_err());
    assert_eq!(fs::read(&f.target).unwrap(), b"original");
}
#[test]
fn rollback_keeps_original_executable_fingerprint() {
    let f = Fixture::new();
    let executable = f.game.join("client.exe");
    fs::write(&executable, b"supported").unwrap();
    let mut one = f.package("version-one", b"one");
    one.package.game_fingerprint = Some(Fingerprint {
        path: "client.exe".into(),
        sha256: digest(b"supported"),
    });
    save(&one.directory.join("bundle.json"), &one.package).unwrap();
    let two = f.package("version-two", b"two");
    f.action(Action::Apply, Some(one)).unwrap();
    f.action(Action::Apply, Some(two)).unwrap();
    fs::write(executable, b"updated game").unwrap();
    assert!(
        f.action(Action::Rollback, None)
            .unwrap_err()
            .contains("game build")
    );
    assert_eq!(fs::read(&f.target).unwrap(), b"two");
    assert!(!recovery_pending(&f.game));
}
#[test]
fn future_tool_version_is_refused() {
    let f = Fixture::new();
    let mut p = f.package("version-one", b"one").package;
    p.min_tool_version = Some(tool_version());
    validate(&p).unwrap();
    let mut future = tool_version();
    future[0] += 1;
    p.min_tool_version = Some(future);
    assert!(validate(&p).unwrap_err().contains("newer"));
}
#[test]
fn source_or_target_traversal_and_duplicate_targets_are_rejected() {
    let f = Fixture::new();
    let mut p = f.package("version-one", b"one").package;
    p.files[0].target = "../outside".into();
    assert!(validate(&p).is_err());
    p.files[0].target = "Binaries/Win64/Paladins.exe".into();
    assert!(validate(&p).is_err());
    p.files[0].target = "ChaosGame/Localization/KOR/Lang_KOR.dat".into();
    let mut second = p.files[0].clone();
    second.source = "other.dat".into();
    p.files.push(second);
    assert!(validate(&p).is_err());
}
#[test]
fn running_game_and_concurrent_operation_are_blocked() {
    let f = Fixture::new();
    let package = f.package("version-one", b"one");
    assert!(
        transition(
            Action::Apply,
            &f.game,
            Some(package.clone()),
            &f.cache,
            &|| true,
            &|_| {}
        )
        .is_err()
    );
    assert!(!f.game.join(STATE).exists());
    let (_, _lock) = acquire(&f.game, &|| false).unwrap();
    assert!(
        f.action(Action::Apply, Some(package))
            .unwrap_err()
            .contains("Another patch operation")
    );
    assert_eq!(fs::read(&f.target).unwrap(), b"original");
}
#[test]
fn interrupted_update_recovers_previous_translation() {
    let f = Fixture::new();
    let one = f.package("version-one", b"one");
    let two = f.package("version-two", b"two");
    f.action(Action::Apply, Some(one)).unwrap();
    let root = f.game.canonicalize().unwrap();
    let previous = snapshot(&root, &f.cache).unwrap();
    save(
        &root.join(STATE).join("patcher-journal.json"),
        &Journal {
            previous,
            next: Some(two.clone()),
        },
    )
    .unwrap();
    upk_patcher::revert_bundle(root.to_str().unwrap()).unwrap();
    upk_patcher::apply_bundle(&two.directory, "KOR", root.to_str().unwrap()).unwrap();
    assert!(recovery_pending(&f.game));
    f.action(Action::Recover, None).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"one");
    assert!(!recovery_pending(&f.game));
}
#[test]
fn interrupted_first_install_recovers_originals() {
    let f = Fixture::new();
    let one = f.package("version-one", b"one");
    let root = f.game.canonicalize().unwrap();
    fs::create_dir_all(root.join(STATE)).unwrap();
    save(
        &root.join(STATE).join("patcher-journal.json"),
        &Journal {
            previous: None,
            next: Some(one.clone()),
        },
    )
    .unwrap();
    upk_patcher::apply_bundle(&one.directory, "KOR", root.to_str().unwrap()).unwrap();
    f.action(Action::Recover, None).unwrap();
    assert_eq!(fs::read(&f.target).unwrap(), b"original");
}
#[test]
fn malicious_original_receipt_cannot_write_outside_game() {
    let f = Fixture::new();
    let one = f.package("version-one", b"one");
    f.action(Action::Apply, Some(one)).unwrap();
    let outside = f.home.join("outside.dat");
    fs::write(&outside, b"one").unwrap();
    let path = f.game.join(STATE).join("active.json");
    let mut value: serde_json::Value = json(&path).unwrap();
    value["files"][0]["target"] = outside.display().to_string().into();
    save(&path, &value).unwrap();
    assert!(f.action(Action::Restore, None).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"one");
}
#[test]
fn catalog_rejects_non_github_sources_in_production_policy() {
    assert!(trusted_url("https://example.com/catalog.json").is_err());
    assert!(trusted_url("https://github.com@evil.example/catalog.json").is_err());
    assert!(trusted_url("https://raw.githubusercontent.com/owner/repo/main/catalog.json").is_ok());
}
#[test]
fn catalog_supports_independent_ime_releases_and_legacy_catalogs() {
    let old = parse_catalog(br#"{"schema_version":1,"packages":[]}"#).unwrap();
    assert!(old.ime_packages.is_empty());
    let value = serde_json::json!({"schema_version":1,"packages":[],"ime_packages":[{
        "package_id":"paladinscat-ime-0-1-0","version":"0.1.0","game_build":"8.1",
        "manifest_url":"https://github.com/PaladinsCat/PaladinsCat-locales/releases/download/ime-0.1.0/ime-package.json",
        "manifest_sha256":"a".repeat(64)
    }]});
    let (base, worker) = server(
        std::collections::HashMap::from([("/catalog".into(), serde_json::to_vec(&value).unwrap())]),
        1,
    );
    let catalog = check_catalog(&format!("{base}/catalog")).unwrap();
    worker.join().unwrap();
    assert_eq!(catalog.ime_packages.len(), 1);
    let mut bad = value;
    bad["ime_packages"][0]["manifest_sha256"] = "bad".into();
    assert!(parse_catalog(&serde_json::to_vec(&bad).unwrap()).is_err());
}

fn server(
    routes: std::collections::HashMap<String, Vec<u8>>,
    requests: usize,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        for _ in 0..requests {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let text = String::from_utf8_lossy(&request[..count]);
            let path = text.split_whitespace().nth(1).unwrap();
            let bytes = routes.get(path).unwrap();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            socket.write_all(bytes).unwrap();
        }
    });
    (address, worker)
}
#[test]
fn download_cache_reuses_verified_payload_and_rejects_tamper() {
    let f = Fixture::new();
    let bytes = b"remote translation";
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    let compressed = encoder.finish().unwrap();
    let (url, worker) = server(
        std::collections::HashMap::from([("/asset".into(), compressed.clone())]),
        1,
    );
    let mut package = f.package("download-template", bytes).package;
    package.bundle_id = "published-version".into();
    package.source_commit = "a".repeat(40);
    package.game_fingerprint = Some(Fingerprint {
        path: "ChaosGame/Localization/KOR/Lang_KOR.dat".into(),
        sha256: digest(b"original"),
    });
    package.files[0].download = Some(Download {
        url: format!("{url}/asset"),
        sha256: digest(&compressed),
        bytes: compressed.len() as u64,
        encoding: "gzip".into(),
    });
    let manifest = serde_json::to_vec(&package).unwrap();
    let (base, manifest_worker) = server(
        std::collections::HashMap::from([("/manifest".into(), manifest.clone())]),
        1,
    );
    let release = Release {
        bundle_id: package.bundle_id.clone(),
        locale: "KOR".into(),
        game_build: package.game_build.clone(),
        manifest_url: format!("{base}/manifest"),
        manifest_sha256: digest(&manifest),
        notes: String::new(),
    };
    // Use a distinct cache: the import fixture must not satisfy this download.
    let download_cache = f.home.join("downloads-test");
    let downloaded = download(&release, &download_cache, &|_| {}).unwrap();
    worker.join().unwrap();
    manifest_worker.join().unwrap();
    assert_eq!(
        fs::read(downloaded.directory.join("Lang_KOR.dat")).unwrap(),
        bytes
    );
    download(&release, &download_cache, &|_| {}).unwrap(); // Server closed: only verified cache can succeed.
    let bad_path = f.home.join("bad-download");
    let (url, worker) = server(
        std::collections::HashMap::from([("/bad".into(), b"bad".to_vec())]),
        1,
    );
    assert!(
        fetch(
            &format!("{url}/bad"),
            &bad_path,
            &digest(b"expected"),
            Some(3),
            &|_| {}
        )
        .is_err()
    );
    worker.join().unwrap();
    assert!(!bad_path.exists());
}
