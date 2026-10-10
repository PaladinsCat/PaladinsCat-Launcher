use super::*;
use std::{collections::HashMap, net::TcpListener, thread};

struct Fixture {
    home: PathBuf,
    game: PathBuf,
    cache: PathBuf,
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn image(tag: u8) -> Vec<u8> {
    let mut bytes = vec![0; 2048];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[70..72].copy_from_slice(&1u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&240u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    let set =
        |bytes: &mut [u8], at: usize, v: u32| bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    set(&mut bytes, 200, 4096);
    set(&mut bytes, 204, 1024);
    set(&mut bytes, 340, 4096);
    set(&mut bytes, 344, 1536);
    set(&mut bytes, 348, 512);
    set(&mut bytes, 364, 0xe0000040);
    set(&mut bytes, 532, 9);
    set(&mut bytes, 536, 9);
    set(&mut bytes, 540, 4096 + 88);
    set(&mut bytes, 544, 4096 + 128);
    set(&mut bytes, 548, 4096 + 168);
    let mut at = 720;
    for (i, name) in pe::EXPORTS.iter().enumerate() {
        set(
            &mut bytes,
            600 + i * 4,
            if i == 5 { 4096 + 1024 } else { 4096 + 1408 },
        );
        set(&mut bytes, 640 + i * 4, 4096 + (at - 512) as u32);
        bytes[680 + i * 2..682 + i * 2].copy_from_slice(&(i as u16).to_le_bytes());
        bytes[at..at + name.len()].copy_from_slice(name.as_bytes());
        at += name.len() + 1;
    }
    for (i, v) in [4u64, 9, 7, 1].iter().enumerate() {
        bytes[1536 + i * 8..1544 + i * 8].copy_from_slice(&v.to_le_bytes());
    }
    bytes[2047] = tag;
    bytes
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "paladinscat-ime-fixture-{}-{}",
            std::process::id(),
            unique()
        ));
        let game = home.join("game");
        let cache = home.join("cache");
        fs::create_dir_all(game.join("ChaosGame")).unwrap();
        fs::create_dir_all(game.join("Binaries/Win64")).unwrap();
        fs::write(game.join("Binaries/Win64/Paladins.exe"), b"fixture-game").unwrap();
        Self { home, game, cache }
    }
    fn package(&self, version: &str, tag: u8) -> Cached {
        let source = self.home.join(format!("source-{version}"));
        let dll = self.home.join(format!("{version}.dll"));
        fs::write(&dll, image(tag)).unwrap();
        let mut p = prepare(&dll, &source, version).unwrap();
        p.game_fingerprint.sha256 = sha(b"fixture-game");
        save(&source.join("ime-package.json"), &p).unwrap();
        import(&source.join("ime-package.json"), &self.cache).unwrap()
    }
    fn run(&self, action: Action, c: Option<Cached>) -> Result<String> {
        operate_with(action, &self.game, c, &|_| {}, &|| false)
    }
}
#[test]
fn prepared_package_and_rejected_enable_remain_disabled_until_verified_install() {
    let f = Fixture::new();
    let cached = f.package("0.1.0", 7);
    assert_eq!(installed_package(&f.game).unwrap(), None);
    assert!(
        operate_with(
            Action::Apply,
            &f.game,
            Some(cached.clone()),
            &|_| {},
            &|| true
        )
        .is_err()
    );
    assert_eq!(installed_package(&f.game).unwrap(), None);
    f.run(Action::Apply, Some(cached.clone())).unwrap();
    assert_eq!(installed_package(&f.game).unwrap(), Some(cached.package));
    f.run(Action::Remove, None).unwrap();
    assert_eq!(installed_package(&f.game).unwrap(), None);
    assert!(cached.directory.join(DLL).exists());
}

#[test]
fn configured_detection_refuses_unknown_or_changed_mod_files() {
    let f = Fixture::new();
    let cached = f.package("0.1.0", 7);
    f.run(Action::Apply, Some(cached)).unwrap();
    fs::write(f.game.join(TARGET).join("dlls").join(DLL), image(8)).unwrap();
    assert!(installed_package(&f.game).is_err());
    save(
        &f.game.join(STATE).join("ime-active.json"),
        &Option::<Package>::None,
    )
    .unwrap();
    assert!(
        installed_package(&f.game)
            .unwrap_err()
            .contains("without a recognized receipt")
    );
}

#[test]
fn restoring_cached_selection_does_not_copy_dll_onto_itself() {
    let f = Fixture::new();
    let cached = f.package("0.1.0", 7);
    let restored = import(&cached.directory.join("ime-package.json"), &f.cache).unwrap();
    assert_eq!(restored.directory, cached.directory);
    assert_eq!(fs::read(restored.directory.join(DLL)).unwrap(), image(7));
    fs::write(cached.directory.join(DLL), image(8)).unwrap();
    assert!(import(&cached.directory.join("ime-package.json"), &f.cache).is_err());
}

#[test]
fn independent_install_update_disable_preserves_locale_and_other_mods() {
    let f = Fixture::new();
    let locale = f.game.join("ChaosGame/keep.dat");
    fs::write(&locale, b"localization").unwrap();
    let other = f.game.join(".tempest/v2/mods/OtherMod");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("keep"), b"other").unwrap();
    f.run(Action::Apply, Some(f.package("0.1.0", 1))).unwrap();
    assert!(installed(&f.game).unwrap().starts_with("0.1.0"));
    f.run(Action::Apply, Some(f.package("0.2.0", 2))).unwrap();
    assert_eq!(
        fs::read(f.game.join(TARGET).join("dlls").join(DLL)).unwrap(),
        image(2)
    );
    f.run(Action::Remove, None).unwrap();
    assert_eq!(installed(&f.game).unwrap(), "Not installed");
    assert_eq!(fs::read(locale).unwrap(), b"localization");
    assert_eq!(fs::read(other.join("keep")).unwrap(), b"other");
    assert!(
        fs::read_dir(f.game.join(STATE).join("ime-archive"))
            .unwrap()
            .count()
            >= 2
    );
}
#[test]
fn running_game_build_mismatch_tamper_and_foreign_mod_are_refused() {
    let f = Fixture::new();
    let c = f.package("0.1.0", 1);
    assert!(operate_with(Action::Apply, &f.game, Some(c.clone()), &|_| {}, &|| true).is_err());
    assert!(!f.game.join(STATE).exists());
    fs::write(f.game.join("Binaries/Win64/Paladins.exe"), b"changed-game").unwrap();
    assert!(f.run(Action::Apply, Some(c.clone())).is_err());
    assert!(!f.game.join(TARGET).exists());
    fs::write(f.game.join("Binaries/Win64/Paladins.exe"), b"fixture-game").unwrap();
    fs::write(c.directory.join(DLL), image(9)).unwrap();
    assert!(f.run(Action::Apply, Some(c)).is_err());
    let target = f.game.join(TARGET);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("foreign"), b"keep").unwrap();
    assert!(f.run(Action::Apply, Some(f.package("0.2.0", 2))).is_err());
    assert_eq!(fs::read(target.join("foreign")).unwrap(), b"keep");
}
#[test]
fn publishing_identical_draft_updates_receipt_without_replacing_mod() {
    let f = Fixture::new();
    let mut c = f.package("0.1.0", 1);
    f.run(Action::Apply, Some(c.clone())).unwrap();
    let archive_count = fs::read_dir(f.game.join(STATE).join("ime-archive"))
        .unwrap()
        .count();
    c.package.draft = false;
    c.package.source_commit = "a".repeat(40);
    save(&c.directory.join("ime-package.json"), &c.package).unwrap();
    assert!(
        f.run(Action::Apply, Some(c))
            .unwrap()
            .contains("already matches")
    );
    assert_eq!(
        fs::read_dir(f.game.join(STATE).join("ime-archive"))
            .unwrap()
            .count(),
        archive_count
    );
    assert!(!recovery_pending(&f.game));
    assert!(
        !active(&f.game.canonicalize().unwrap())
            .unwrap()
            .unwrap()
            .draft
    );
}
#[test]
fn interrupted_update_after_new_mod_is_installed_restores_previous_mod() {
    let f = Fixture::new();
    let old = f.package("0.1.0", 1);
    let next = f.package("0.2.0", 2);
    f.run(Action::Apply, Some(old.clone())).unwrap();
    let root = f.game.canonicalize().unwrap();
    let archive = ".watchcat-localization/ime-archive/interrupted-after-install";
    fs::rename(root.join(TARGET), root.join(archive)).unwrap();
    fs::create_dir_all(root.join(TARGET).join("dlls")).unwrap();
    fs::copy(
        next.directory.join(DLL),
        root.join(TARGET).join("dlls").join(DLL),
    )
    .unwrap();
    fs::write(
        root.join(TARGET).join("manifest.toml"),
        manifest(&next.package),
    )
    .unwrap();
    save(
        &root.join(STATE).join("ime-active.json"),
        &Some(next.package.clone()),
    )
    .unwrap();
    save(
        &root.join(STATE).join("ime-journal.json"),
        &Journal {
            previous: Some(old.package),
            next: Some(next.package),
            archive: archive.into(),
            staging: None,
        },
    )
    .unwrap();
    f.run(Action::Recover, None).unwrap();
    assert!(installed(&f.game).unwrap().starts_with("0.1.0"));
    assert_eq!(
        fs::read(root.join(TARGET).join("dlls").join(DLL)).unwrap(),
        image(1)
    );
    assert!(!recovery_pending(&f.game));
}
#[test]
fn interrupted_directory_swap_restores_exact_previous_package() {
    let f = Fixture::new();
    let old = f.package("0.1.0", 1);
    f.run(Action::Apply, Some(old.clone())).unwrap();
    let next = f.package("0.2.0", 2);
    let root = f.game.canonicalize().unwrap();
    let archive = ".watchcat-localization/ime-archive/interrupted";
    fs::rename(root.join(TARGET), root.join(archive)).unwrap();
    save(
        &root.join(STATE).join("ime-journal.json"),
        &Journal {
            previous: Some(old.package),
            next: Some(next.package),
            archive: archive.into(),
            staging: None,
        },
    )
    .unwrap();
    f.run(Action::Recover, None).unwrap();
    assert!(installed(&f.game).unwrap().starts_with("0.1.0"));
    assert!(!recovery_pending(&f.game));
}
#[test]
fn changed_installed_files_and_unsafe_journal_do_not_get_overwritten() {
    let f = Fixture::new();
    let c = f.package("0.1.0", 1);
    f.run(Action::Apply, Some(c.clone())).unwrap();
    fs::write(f.game.join(TARGET).join("dlls").join(DLL), image(8)).unwrap();
    assert!(f.run(Action::Remove, None).is_err());
    let root = f.game.canonicalize().unwrap();
    save(
        &root.join(STATE).join("ime-journal.json"),
        &Journal {
            previous: Some(c.package),
            next: None,
            archive: "../escape".into(),
            staging: None,
        },
    )
    .unwrap();
    assert!(f.run(Action::Recover, None).is_err());
    assert_eq!(
        fs::read(root.join(TARGET).join("dlls").join(DLL)).unwrap(),
        image(8)
    );
}
#[test]
fn localization_and_ime_operations_share_the_same_installation_lock() {
    let f = Fixture::new();
    let c = f.package("0.1.0", 1);
    let (_root, _lock) = engine::acquire(&f.game, &|| false).unwrap();
    assert!(
        f.run(Action::Apply, Some(c))
            .unwrap_err()
            .contains("Another patch operation")
    );
    assert!(!f.game.join(TARGET).exists());
}
fn server(routes: HashMap<String, Vec<u8>>, count: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        for _ in 0..count {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let n = socket.read(&mut request).unwrap();
            let text = String::from_utf8_lossy(&request[..n]);
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
    (base, worker)
}
#[test]
fn remote_payload_is_verified_cached_and_bound_to_catalog_identity() {
    let f = Fixture::new();
    let mut p = f.package("0.1.0", 1).package;
    p.draft = false;
    p.source_commit = "a".repeat(40);
    let bytes = image(1);
    let (url, payload_worker) = server(HashMap::from([("/dll".into(), bytes.clone())]), 1);
    p.download = Some(Download {
        url: format!("{url}/dll"),
        sha256: sha(&bytes),
        bytes: bytes.len() as u64,
        encoding: "raw".into(),
    });
    let manifest = serde_json::to_vec(&p).unwrap();
    let (base, manifest_worker) =
        server(HashMap::from([("/manifest".into(), manifest.clone())]), 1);
    let r = Release {
        package_id: p.package_id.clone(),
        version: p.version.clone(),
        game_build: p.game_build.clone(),
        manifest_url: format!("{base}/manifest"),
        manifest_sha256: sha(&manifest),
        notes: String::new(),
    };
    let c = download(&r, &f.home.join("remote-cache"), &|_| {}).unwrap();
    payload_worker.join().unwrap();
    manifest_worker.join().unwrap();
    assert_eq!(fs::read(c.directory.join(DLL)).unwrap(), bytes);
    download(&r, &f.home.join("remote-cache"), &|_| {}).unwrap();
    let mut mismatched = r;
    mismatched.package_id = "another-package".into();
    assert!(download(&mismatched, &f.home.join("remote-cache"), &|_| {}).is_err());
}
#[test]
fn malformed_executable_and_remote_drafts_are_not_release_material() {
    let f = Fixture::new();
    let dll = f.home.join("bad.dll");
    fs::write(&dll, b"not-a-dll").unwrap();
    assert!(prepare(&dll, &f.home.join("bad-output"), "0.1.0").is_err());
    let p = f.package("0.1.0", 1).package;
    let bytes = serde_json::to_vec(&p).unwrap();
    let (base, worker) = server(HashMap::from([("/manifest".into(), bytes.clone())]), 1);
    let r = Release {
        package_id: p.package_id,
        version: p.version,
        game_build: p.game_build,
        manifest_url: format!("{base}/manifest"),
        manifest_sha256: sha(&bytes),
        notes: String::new(),
    };
    assert!(
        download(&r, &f.home.join("draft-cache"), &|_| {})
            .unwrap_err()
            .contains("reviewed source commit")
    );
    worker.join().unwrap();
}
#[test]
fn dll_exports_and_actual_abi_are_checked_without_execution() {
    let mut bytes = image(1);
    pe(&bytes).unwrap();
    bytes[1536] = 3;
    assert!(pe(&bytes).unwrap_err().contains("ABI"));
    let mut bytes = image(1);
    bytes[720] = b'X';
    assert!(pe(&bytes).is_err());
    for size in [0, 1, 63, 128, 511, 1537] {
        assert!(pe(&image(1)[..size]).is_err());
    }
}
#[test]
fn zero_initialized_data_exports_can_be_virtual_but_functions_cannot() {
    let mut bytes = image(1);
    bytes[336..340].copy_from_slice(&2048u32.to_le_bytes());
    bytes[604..608].copy_from_slice(&(4096u32 + 1600).to_le_bytes());
    pe(&bytes).unwrap(); // WatchCatImeState is legitimate zero-initialized DLL data.
    bytes[364..368].copy_from_slice(&0xc0000040u32.to_le_bytes());
    assert!(pe(&bytes).unwrap_err().contains("not executable"));
}
