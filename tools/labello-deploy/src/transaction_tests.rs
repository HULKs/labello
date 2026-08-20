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
        self.action("stop_server")
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
    build_archive(tag, commit, false)
}

fn candidate_archive_with_extra(tag: &str, commit: &str) -> Vec<u8> {
    build_archive(tag, commit, true)
}

fn build_archive(tag: &str, commit: &str, extra: bool) -> Vec<u8> {
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
        archive.finish().unwrap();
    }
    output
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
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
