use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Result, bail};
use tempfile::TempDir;

use super::*;

#[derive(Clone)]
struct FakePlatform {
    state: Arc<Mutex<FakeState>>,
    data: PathBuf,
}

#[derive(Default)]
struct FakeState {
    actions: Vec<String>,
    fail_action: Option<String>,
    mutate_on_wait: bool,
    mutate_on_stop: Option<Vec<u8>>,
}

impl FakePlatform {
    fn new(data: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState::default())),
            data,
        }
    }

    fn fail(&self, action: &str) {
        self.state.lock().unwrap().fail_action = Some(action.to_string());
    }

    fn mutate_on_wait(&self) {
        self.state.lock().unwrap().mutate_on_wait = true;
    }

    fn mutate_on_stop(&self, contents: &[u8]) {
        self.state.lock().unwrap().mutate_on_stop = Some(contents.to_vec());
    }

    fn actions(&self) -> Vec<String> {
        self.state.lock().unwrap().actions.clone()
    }

    fn action(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.actions.push(name.to_string());
        let occurrence = state
            .actions
            .iter()
            .filter(|action| *action == name)
            .count();
        if state.fail_action.as_deref() == Some(name)
            || state.fail_action.as_deref() == Some(&format!("{name}:{occurrence}"))
        {
            state.fail_action = None;
            bail!("injected failure");
        }
        Ok(())
    }
}

impl Platform for FakePlatform {
    fn reload_caddy(&self) -> Result<()> {
        self.action("reload_caddy")
    }

    fn restart_server(&self) -> Result<()> {
        self.action("restart_server")
    }

    fn stop_server(&self) -> Result<()> {
        self.action("stop_server")?;
        if let Some(contents) = self.state.lock().unwrap().mutate_on_stop.take() {
            fs::write(self.data.join("authoritative.json"), contents)?;
        }
        Ok(())
    }

    fn wait_until_ready(&self, _release_tag: &str, _source_commit: &str) -> Result<()> {
        let mutate = self.state.lock().unwrap().mutate_on_wait;
        if mutate {
            fs::write(self.data.join("authoritative.json"), b"candidate")?;
        }
        self.action("wait_until_ready")
    }

    fn start_worker(&self, _request_id: &str) -> Result<()> {
        self.action("start_worker")
    }
}

struct Setup {
    _temp: TempDir,
    root: PathBuf,
    platform: FakePlatform,
}

impl Setup {
    fn new(previous: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("deployment");
        fs::create_dir_all(root.join("caddy/live")).unwrap();
        fs::create_dir_all(root.join("caddy/maintenance")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("config/browser")).unwrap();
        fs::write(root.join("config/labello.server.toml"), b"configuration").unwrap();
        fs::write(root.join("data/authoritative.json"), b"original").unwrap();
        if previous {
            fs::create_dir_all(root.join("releases/v0.1.0")).unwrap();
            fs::create_dir_all(root.join("configurations/v0.1.0")).unwrap();
            fs::write(
                root.join("configurations/v0.1.0/labello.server.toml"),
                b"old configuration",
            )
            .unwrap();
            symlink("v0.1.0", root.join("releases/current")).unwrap();
            symlink("v0.1.0", root.join("configurations/current")).unwrap();
        }
        let platform = FakePlatform::new(root.join("data"));
        Self {
            _temp: temp,
            root,
            platform,
        }
    }

    fn manager(&self) -> DeploymentManager<FakePlatform> {
        DeploymentManager::new(self.root.clone(), self.platform.clone())
    }

    fn receive(&self, id: &str) {
        self.manager()
            .receive(
                id,
                Cursor::new(candidate_archive("v1.2.3", COMMIT)),
                ReceiveOptions {
                    start_worker: false,
                },
            )
            .unwrap();
    }
}

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn complete_transaction_admits_candidate() {
    let setup = Setup::new(true);
    setup.receive("request-1");

    let status = setup.manager().run("request-1").unwrap();

    assert_eq!(status.phase, Phase::Complete);
    assert!(status.candidate_data_access_started);
    assert!(status.admission_started);
    assert_eq!(
        fs::read_link(setup.root.join("releases/current")).unwrap(),
        Path::new("v1.2.3")
    );
    assert_eq!(
        fs::read_link(setup.root.join("configurations/current")).unwrap(),
        Path::new("v1.2.3")
    );
    assert_eq!(
        fs::read_link(setup.root.join("caddy/current")).unwrap(),
        Path::new("live")
    );
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"original"
    );
}

#[test]
fn duplicate_request_is_rejected_without_replacing_the_first() {
    let setup = Setup::new(false);
    setup.receive("request-1");
    let second = setup.manager().receive(
        "request-1",
        Cursor::new(candidate_archive("v1.2.3", COMMIT)),
        ReceiveOptions {
            start_worker: false,
        },
    );
    assert!(second.is_err());
    assert_eq!(
        setup.manager().status("request-1").unwrap().phase,
        Phase::Received
    );
}

#[test]
fn worker_start_failure_preserves_the_durable_request() {
    let setup = Setup::new(false);
    setup.platform.fail("start_worker");

    let result = setup.manager().receive(
        "request-1",
        Cursor::new(candidate_archive("v1.2.3", COMMIT)),
        ReceiveOptions { start_worker: true },
    );

    assert!(result.is_err());
    assert_eq!(
        setup.manager().status("request-1").unwrap().phase,
        Phase::Received
    );
}

#[test]
fn candidate_archive_size_is_bounded_even_after_the_tar_terminator() {
    let temp = tempfile::tempdir().unwrap();
    let archive = candidate_archive("v1.2.3", COMMIT);
    let limit = archive.len() as u64;
    let mut padded = archive;
    padded.push(0);

    assert!(unpack_candidate_bounded(Cursor::new(padded), temp.path(), limit).is_err());
}

#[test]
fn readiness_adapter_accepts_the_api_schema_version_field() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request).unwrap();
        let body = serde_json::to_vec(&serde_json::json!({
            "ok": true,
            "service": "labello",
            "releaseTag": "v1.2.3",
            "sourceCommit": COMMIT,
            "schemaVersion": 3,
            "persistence": "ok",
            "authentication": "ok"
        }))
        .unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        std::io::Write::write_all(&mut stream, &body).unwrap();
    });
    let platform = RealPlatform {
        api_address: address,
        systemctl: PathBuf::from("/not-used"),
        readiness_attempts: 1,
    };

    platform.wait_until_ready("v1.2.3", COMMIT).unwrap();
    server.join().unwrap();
}

#[test]
fn receive_rejects_duplicate_archive_paths() {
    let setup = Setup::new(false);
    let result = setup.manager().receive(
        "request-1",
        Cursor::new(candidate_archive_with_duplicate("v1.2.3", COMMIT)),
        ReceiveOptions {
            start_worker: false,
        },
    );

    assert!(result.is_err());
    assert!(!setup.root.join("requests/request-1").exists());
}

#[test]
fn an_existing_release_must_match_the_complete_candidate_manifest() {
    let setup = Setup::new(false);
    fs::create_dir_all(setup.root.join("releases/v1.2.3/server")).unwrap();
    fs::create_dir_all(setup.root.join("releases/v1.2.3/browser")).unwrap();
    let existing = candidate_archive("v1.2.3", COMMIT);
    unpack_candidate(Cursor::new(existing), &setup.root.join("releases/v1.2.3")).unwrap();
    fs::write(
        setup.root.join("releases/v1.2.3/browser/index.html"),
        b"different",
    )
    .unwrap();
    setup.receive("request-1");

    assert!(setup.manager().run("request-1").is_err());
    assert!(
        !setup
            .manager()
            .status("request-1")
            .unwrap()
            .candidate_data_access_started
    );
}

#[test]
fn receive_rejects_uninventoried_files() {
    let setup = Setup::new(false);
    let archive = candidate_archive_with_extra("v1.2.3", COMMIT);
    let result = setup.manager().receive(
        "request-1",
        Cursor::new(archive),
        ReceiveOptions {
            start_worker: false,
        },
    );
    assert!(result.is_err());
    assert!(!setup.root.join("requests/request-1").exists());
}

#[test]
fn receive_rejects_noncanonical_release_tags() {
    let setup = Setup::new(false);
    let result = setup.manager().receive(
        "request-1",
        Cursor::new(candidate_archive("v01.2.3", COMMIT)),
        ReceiveOptions {
            start_worker: false,
        },
    );
    assert!(result.is_err());
}

#[test]
fn release_manifest_generation_inventories_candidate_files() {
    let temp = tempfile::tempdir().unwrap();
    let candidate = temp.path();
    fs::create_dir_all(candidate.join("server")).unwrap();
    fs::create_dir_all(candidate.join("browser")).unwrap();
    fs::write(candidate.join("server/labello-server"), b"server").unwrap();
    let mut permissions = fs::metadata(candidate.join("server/labello-server"))
        .unwrap()
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(candidate.join("server/labello-server"), permissions).unwrap();
    fs::write(candidate.join("browser/index.html"), b"index").unwrap();
    fs::write(
        candidate.join("browser/release.json"),
        serde_json::to_vec(&serde_json::json!({
            "releaseTag": "v1.2.3",
            "sourceCommit": COMMIT,
        }))
        .unwrap(),
    )
    .unwrap();
    write_browser_manifest(candidate.join("browser").as_path());

    create_release_manifest(candidate, "v1.2.3", COMMIT).unwrap();

    let manifest: ReleaseManifest =
        serde_json::from_slice(&fs::read(candidate.join(MANIFEST_NAME)).unwrap()).unwrap();
    assert_eq!(manifest.files.len(), 4);
    assert!(validate_candidate(candidate).is_ok());
}

#[test]
fn release_asset_verification_cross_checks_metadata_and_checksums() {
    let temp = tempfile::tempdir().unwrap();
    write_release_assets(temp.path(), "v1.2.3", COMMIT);

    verify_release_assets(temp.path(), "v1.2.3", COMMIT).unwrap();
}

#[test]
fn release_asset_verification_rejects_an_untrusted_checksum_path() {
    let temp = tempfile::tempdir().unwrap();
    write_release_assets(temp.path(), "v1.2.3", COMMIT);
    fs::write(
        temp.path().join("SHA256SUMS"),
        format!("{}  /etc/passwd\n", "0".repeat(64)),
    )
    .unwrap();

    assert!(verify_release_assets(temp.path(), "v1.2.3", COMMIT).is_err());
}

#[test]
fn release_asset_verification_rejects_metadata_payload_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    write_release_assets(temp.path(), "v1.2.3", COMMIT);
    let metadata_name = "release-metadata-v1.2.3.json";
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.path().join(metadata_name)).unwrap()).unwrap();
    metadata["payloads"][0]["sha256"] = serde_json::Value::String("0".repeat(64));
    fs::write(
        temp.path().join(metadata_name),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    write_release_checksums(temp.path(), "v1.2.3");

    assert!(verify_release_assets(temp.path(), "v1.2.3", COMMIT).is_err());
}

#[test]
fn candidate_rejects_a_browser_manifest_that_omits_a_file() {
    let temp = tempfile::tempdir().unwrap();
    let archive = candidate_archive("v1.2.3", COMMIT);
    let candidate = temp.path().join("candidate");
    fs::create_dir(&candidate).unwrap();
    unpack_candidate(Cursor::new(archive), &candidate).unwrap();
    fs::write(candidate.join("browser/MANIFEST.sha256"), b"").unwrap();

    assert!(validate_candidate(&candidate).is_err());
}

#[test]
fn maintenance_failure_leaves_data_untouched() {
    let setup = Setup::new(true);
    setup.receive("request-1");
    setup.platform.fail("reload_caddy");

    assert!(setup.manager().run("request-1").is_err());
    let status = setup.manager().status("request-1").unwrap();
    assert_eq!(status.phase, Phase::RolledBack);
    assert!(!status.candidate_data_access_started);
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"original"
    );
}

#[test]
fn backup_waits_for_server_quiescence_and_matches_its_source() {
    let setup = Setup::new(true);
    setup.receive("request-1");
    setup.platform.mutate_on_stop(b"settled");

    setup.manager().run("request-1").unwrap();

    let backup_data = setup.root.join("backups/request-1/data");
    assert_eq!(
        fs::read(backup_data.join("authoritative.json")).unwrap(),
        b"settled"
    );
    let recorded: Vec<BackupEntry> = serde_json::from_slice(
        &fs::read(setup.root.join("backups/request-1/inventory.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        recorded,
        backup_inventory(&setup.root.join("data")).unwrap()
    );
    assert_eq!(recorded, backup_inventory(&backup_data).unwrap());
    let actions = setup.platform.actions();
    let stop = actions
        .iter()
        .position(|action| action == "stop_server")
        .expect("server was stopped");
    let readiness = actions
        .iter()
        .position(|action| action == "wait_until_ready")
        .expect("candidate readiness was checked");
    assert!(stop < readiness);
}

#[test]
fn verified_backup_copy_rejects_a_source_change_during_copy() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("authoritative.json"), b"before").unwrap();

    let result = copy_tree_verified_with(&source, &destination, |source, destination| {
        copy_tree(source, destination)?;
        fs::write(source.join("authoritative.json"), b"after")?;
        Ok(())
    });

    assert!(result.is_err());
}

#[test]
fn failed_candidate_restores_verified_backup_and_previous_release() {
    let setup = Setup::new(true);
    let original_mode = fs::metadata(setup.root.join("data"))
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    fs::create_dir(setup.root.join("data/empty-authoritative-directory")).unwrap();
    setup.receive("request-1");
    setup.platform.mutate_on_wait();
    setup.platform.fail("wait_until_ready");

    assert!(setup.manager().run("request-1").is_err());
    let status = setup.manager().status("request-1").unwrap();
    assert_eq!(status.phase, Phase::RolledBack);
    assert!(status.candidate_data_access_started);
    assert!(!status.admission_started);
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"original"
    );
    assert!(
        setup
            .root
            .join("data/empty-authoritative-directory")
            .is_dir()
    );
    assert_eq!(
        fs::metadata(setup.root.join("data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        original_mode
    );
    assert_eq!(
        fs::read_link(setup.root.join("releases/current")).unwrap(),
        Path::new("v0.1.0")
    );
}

#[test]
fn failed_first_install_restores_data_and_stays_in_maintenance() {
    let setup = Setup::new(false);
    setup.receive("request-1");
    setup.platform.mutate_on_wait();
    setup.platform.fail("wait_until_ready");

    assert!(setup.manager().run("request-1").is_err());
    let status = setup.manager().status("request-1").unwrap();
    assert_eq!(status.phase, Phase::FirstInstallFailed);
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"original"
    );
    assert_eq!(
        fs::read_link(setup.root.join("caddy/current")).unwrap(),
        Path::new("maintenance")
    );
}

#[test]
fn failure_after_admission_barrier_never_restores_data() {
    let setup = Setup::new(true);
    setup.receive("request-1");
    setup.platform.mutate_on_wait();
    setup.platform.fail("reload_caddy:2");

    assert!(setup.manager().run("request-1").is_err());
    let status = setup.manager().status("request-1").unwrap();
    assert_eq!(status.phase, Phase::ManualRecovery);
    assert!(status.admission_started);
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"candidate"
    );
}

#[test]
fn boot_recovery_rolls_back_an_uncertain_candidate() {
    let setup = Setup::new(true);
    setup.receive("request-1");
    let manager = setup.manager();
    let mut journal = manager.load_journal("request-1").unwrap();
    journal.previous_release = Some("v0.1.0".to_string());
    journal.previous_release_captured = true;
    manager.back_up_data("request-1").unwrap();
    journal.backup_created = true;
    journal.candidate_data_access_started = true;
    journal.phase = Phase::CandidateDataAccessStarted;
    manager.save_journal(&journal).unwrap();
    fs::write(setup.root.join("data/authoritative.json"), b"candidate").unwrap();

    manager.boot_recover().unwrap();

    assert_eq!(
        manager.status("request-1").unwrap().phase,
        Phase::RolledBack
    );
    assert_eq!(
        fs::read(setup.root.join("data/authoritative.json")).unwrap(),
        b"original"
    );
}

#[test]
fn boot_recovery_marks_post_admission_crash_for_manual_recovery() {
    let setup = Setup::new(true);
    setup.receive("request-1");
    let manager = setup.manager();
    let mut journal = manager.load_journal("request-1").unwrap();
    journal.previous_release = Some("v0.1.0".to_string());
    journal.previous_release_captured = true;
    journal.candidate_data_access_started = true;
    journal.admission_started = true;
    journal.phase = Phase::AdmissionStarted;
    manager.save_journal(&journal).unwrap();

    assert!(manager.boot_recover().is_err());

    assert_eq!(
        manager.status("request-1").unwrap().phase,
        Phase::ManualRecovery
    );
}

#[test]
fn boot_recovery_of_received_request_preserves_live_previous_release() {
    let setup = Setup::new(true);
    symlink("live", setup.root.join("caddy/current")).unwrap();
    setup.receive("request-1");

    setup.manager().boot_recover().unwrap();

    assert_eq!(
        setup.manager().status("request-1").unwrap().phase,
        Phase::RolledBack
    );
    assert_eq!(
        fs::read_link(setup.root.join("releases/current")).unwrap(),
        Path::new("v0.1.0")
    );
    assert_eq!(
        fs::read_link(setup.root.join("caddy/current")).unwrap(),
        Path::new("live")
    );
    assert!(setup.platform.actions().is_empty());
}

#[test]
fn managed_data_rejects_symbolic_links_before_backup() {
    let setup = Setup::new(true);
    symlink("authoritative.json", setup.root.join("data/alias")).unwrap();
    setup.receive("request-1");

    assert!(setup.manager().run("request-1").is_err());
    let status = setup.manager().status("request-1").unwrap();
    assert_eq!(status.phase, Phase::RolledBack);
    assert!(!status.candidate_data_access_started);
}

fn candidate_archive(tag: &str, commit: &str) -> Vec<u8> {
    build_archive(tag, commit, false, false)
}

fn candidate_archive_with_extra(tag: &str, commit: &str) -> Vec<u8> {
    build_archive(tag, commit, true, false)
}

fn candidate_archive_with_duplicate(tag: &str, commit: &str) -> Vec<u8> {
    build_archive(tag, commit, false, true)
}

fn build_archive(tag: &str, commit: &str, extra: bool, duplicate: bool) -> Vec<u8> {
    let mut files = BTreeMap::from([
        ("server/labello-server".to_string(), b"server".to_vec()),
        ("browser/index.html".to_string(), b"index".to_vec()),
        (
            "browser/release.json".to_string(),
            serde_json::to_vec(&serde_json::json!({
                "releaseTag": tag,
                "sourceCommit": commit,
            }))
            .unwrap(),
        ),
    ]);
    let browser_manifest = files
        .iter()
        .filter(|(path, _)| path.starts_with("browser/"))
        .map(|(path, bytes)| {
            format!(
                "{}  ./{}\n",
                hash_bytes(bytes),
                path.strip_prefix("browser/").unwrap()
            )
        })
        .collect::<String>();
    files.insert(
        "browser/MANIFEST.sha256".to_string(),
        browser_manifest.into_bytes(),
    );
    let manifest = ReleaseManifest {
        schema_version: MANIFEST_SCHEMA,
        release_tag: tag.to_string(),
        source_commit: commit.to_string(),
        files: files
            .iter()
            .map(|(path, bytes)| ManifestFile {
                path: path.clone(),
                sha256: hash_bytes(bytes),
            })
            .collect(),
    };
    files.insert(
        MANIFEST_NAME.to_string(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    if extra {
        files.insert("extra".to_string(), b"not inventoried".to_vec());
    }

    let mut output = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut output);
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(if path == "server/labello-server" {
                0o755
            } else {
                0o644
            });
            header.set_cksum();
            archive
                .append_data(&mut header, path, Cursor::new(bytes))
                .unwrap();
        }
        if duplicate {
            let bytes = b"index";
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, "./browser/index.html", Cursor::new(bytes))
                .unwrap();
        }
        archive.finish().unwrap();
    }
    output
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn write_release_assets(root: &Path, tag: &str, commit: &str) {
    let server = format!("labello-server-x86_64-linux-{tag}.tar.gz");
    let browser = format!("labello-browser-{tag}.tar.gz");
    let deployment = format!("labello-deployment-{tag}.tar.gz");
    let metadata_name = format!("release-metadata-{tag}.json");
    let payloads = [
        (server, b"server".as_slice()),
        (browser, b"browser".as_slice()),
        (deployment, b"deployment".as_slice()),
    ];
    for (name, contents) in &payloads {
        fs::write(root.join(name), contents).unwrap();
    }
    let metadata = serde_json::json!({
        "schemaVersion": RELEASE_METADATA_SCHEMA,
        "releaseTag": tag,
        "sourceCommit": commit,
        "payloads": payloads
            .iter()
            .map(|(name, contents)| serde_json::json!({
                "name": name,
                "sha256": hash_bytes(contents),
            }))
            .collect::<Vec<_>>(),
    });
    fs::write(
        root.join(metadata_name),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    write_release_checksums(root, tag);
}

fn write_release_checksums(root: &Path, tag: &str) {
    let names = [
        format!("labello-server-x86_64-linux-{tag}.tar.gz"),
        format!("labello-browser-{tag}.tar.gz"),
        format!("labello-deployment-{tag}.tar.gz"),
        format!("release-metadata-{tag}.json"),
    ];
    let checksums = names
        .iter()
        .map(|name| {
            format!(
                "{}  {name}\n",
                hash_bytes(&fs::read(root.join(name)).unwrap())
            )
        })
        .collect::<String>();
    fs::write(root.join("SHA256SUMS"), checksums).unwrap();
}

fn write_browser_manifest(root: &Path) {
    let mut entries = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    let manifest = entries
        .into_iter()
        .map(|entry| {
            format!(
                "{}  ./{}\n",
                hash_bytes(&fs::read(entry.path()).unwrap()),
                entry.file_name().to_str().unwrap()
            )
        })
        .collect::<String>();
    fs::write(root.join("MANIFEST.sha256"), manifest).unwrap();
}
