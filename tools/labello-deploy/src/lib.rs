use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const JOURNAL_SCHEMA: u32 = 2;
const MANIFEST_SCHEMA: u32 = 1;
const RELEASE_METADATA_SCHEMA: u32 = 1;
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MANIFEST_NAME: &str = "release-manifest.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveOptions {
    pub start_worker: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub release_tag: String,
    pub source_commit: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseMetadata {
    schema_version: u32,
    release_tag: String,
    source_commit: String,
    payloads: Vec<ReleasePayload>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReleasePayload {
    name: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackupEntry {
    path: String,
    kind: BackupEntryKind,
    mode: u32,
    sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum BackupEntryKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Received,
    Maintenance,
    BackupVerified,
    CandidatePublished,
    CandidateDataAccessStarted,
    CandidateReady,
    AdmissionStarted,
    Complete,
    RolledBack,
    FirstInstallFailed,
    ManualRecovery,
}

impl Phase {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Complete | Self::RolledBack | Self::FirstInstallFailed | Self::ManualRecovery
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    Admission,
    Backup,
    Candidate,
    Input,
    Recovery,
    Service,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    request_id: String,
    release_tag: String,
    source_commit: String,
    previous_release: Option<String>,
    previous_release_captured: bool,
    phase: Phase,
    candidate_data_access_started: bool,
    admission_started: bool,
    backup_created: bool,
    failure: Option<FailureCategory>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentStatus {
    pub schema_version: u32,
    pub request_id: String,
    pub release_tag: String,
    pub source_commit: String,
    pub phase: Phase,
    pub candidate_data_access_started: bool,
    pub admission_started: bool,
    pub failure: Option<FailureCategory>,
}

impl From<Journal> for DeploymentStatus {
    fn from(journal: Journal) -> Self {
        Self {
            schema_version: journal.schema_version,
            request_id: journal.request_id,
            release_tag: journal.release_tag,
            source_commit: journal.source_commit,
            phase: journal.phase,
            candidate_data_access_started: journal.candidate_data_access_started,
            admission_started: journal.admission_started,
            failure: journal.failure,
        }
    }
}

pub trait Platform {
    fn reload_caddy(&self) -> Result<()>;
    fn restart_server(&self) -> Result<()>;
    fn stop_server(&self) -> Result<()>;
    fn wait_until_ready(&self, release_tag: &str, source_commit: &str) -> Result<()>;
    fn start_worker(&self, request_id: &str) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct RealPlatform {
    api_address: SocketAddr,
    systemctl: PathBuf,
    readiness_attempts: usize,
}

impl RealPlatform {
    pub fn from_environment() -> Result<Self> {
        let api_address = std::env::var("LABELLO_DEPLOY_API_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
            .parse()
            .context("invalid deployment API address")?;
        let systemctl = std::env::var_os("LABELLO_DEPLOY_SYSTEMCTL")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/systemctl"));
        Ok(Self {
            api_address,
            systemctl,
            readiness_attempts: 60,
        })
    }

    fn user_service(&self, operation: &str, unit: &str) -> Result<()> {
        let status = Command::new(&self.systemctl)
            .args(["--user", operation, unit])
            .status()
            .context("cannot execute user service manager")?;
        ensure!(status.success(), "user service operation failed");
        Ok(())
    }

    fn readiness(&self) -> Result<ReadinessResponse> {
        let mut stream = TcpStream::connect_timeout(&self.api_address, Duration::from_secs(2))
            .context("readiness connection failed")?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(
            b"GET /deployment/readiness HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )?;
        let mut response = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut response)?;
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .context("invalid readiness response")?;
        let headers = std::str::from_utf8(&response[..split])?;
        ensure!(
            headers
                .lines()
                .next()
                .is_some_and(|line| line.contains(" 200 ")),
            "readiness returned a failure status"
        );
        serde_json::from_slice(&response[split + 4..]).context("invalid readiness body")
    }
}

impl Platform for RealPlatform {
    fn reload_caddy(&self) -> Result<()> {
        self.user_service("reload-or-restart", "labello-caddy.service")
    }

    fn restart_server(&self) -> Result<()> {
        self.user_service("restart", "labello-server.service")
    }

    fn stop_server(&self) -> Result<()> {
        self.user_service("stop", "labello-server.service")
    }

    fn wait_until_ready(&self, release_tag: &str, source_commit: &str) -> Result<()> {
        for _ in 0..self.readiness_attempts {
            if let Ok(readiness) = self.readiness()
                && readiness.ok
                && readiness.service == "labello"
                && readiness.release_tag == release_tag
                && readiness.source_commit == source_commit
                && readiness.schema_version > 0
                && readiness.persistence == "ok"
                && readiness.authentication == "ok"
            {
                return Ok(());
            }
            thread::sleep(Duration::from_secs(1));
        }
        bail!("candidate did not become ready")
    }

    fn start_worker(&self, request_id: &str) -> Result<()> {
        self.user_service("start", &format!("labello-deploy@{request_id}.service"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadinessResponse {
    ok: bool,
    service: String,
    release_tag: String,
    source_commit: String,
    schema_version: u32,
    persistence: String,
    authentication: String,
}

pub struct DeploymentManager<P> {
    root: PathBuf,
    platform: P,
}

pub fn create_release_manifest(
    root: impl AsRef<Path>,
    release_tag: &str,
    source_commit: &str,
) -> Result<()> {
    let root = root.as_ref();
    validate_release_tag(release_tag)?;
    validate_source_commit(source_commit)?;
    ensure!(
        !root.join(MANIFEST_NAME).exists(),
        "release manifest already exists"
    );
    let manifest = ReleaseManifest {
        schema_version: MANIFEST_SCHEMA,
        release_tag: release_tag.to_string(),
        source_commit: source_commit.to_string(),
        files: inventory(root)?,
    };
    write_json(&root.join(MANIFEST_NAME), &manifest)?;
    validate_candidate(root)?;
    Ok(())
}

pub fn verify_release_assets(
    root: impl AsRef<Path>,
    release_tag: &str,
    source_commit: &str,
) -> Result<()> {
    let root = root.as_ref();
    validate_release_tag(release_tag)?;
    validate_source_commit(source_commit)?;
    ensure!(root.is_dir(), "release asset directory is unavailable");

    let server = format!("labello-server-x86_64-linux-{release_tag}.tar.gz");
    let browser = format!("labello-browser-{release_tag}.tar.gz");
    let deployment = format!("labello-deployment-{release_tag}.tar.gz");
    let metadata_name = format!("release-metadata-{release_tag}.json");
    let payload_names = BTreeSet::from([server, browser, deployment]);
    let checksummed_names = payload_names
        .iter()
        .cloned()
        .chain(std::iter::once(metadata_name.clone()))
        .collect::<BTreeSet<_>>();
    let expected_files = checksummed_names
        .iter()
        .cloned()
        .chain(std::iter::once("SHA256SUMS".to_string()))
        .collect::<BTreeSet<_>>();

    let mut actual_files = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "release asset is not a regular file"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("release asset name is not UTF-8"))?;
        ensure!(actual_files.insert(name), "duplicate release asset path");
    }
    ensure!(
        actual_files == expected_files,
        "release asset set is incomplete"
    );

    let checksum_text = fs::read_to_string(root.join("SHA256SUMS"))?;
    let mut checksums = BTreeMap::new();
    for line in checksum_text.lines() {
        let (digest, name) = line
            .split_once("  ")
            .context("release checksum line is invalid")?;
        validate_sha256(digest)?;
        ensure!(
            checksummed_names.contains(name),
            "release checksum path is unexpected"
        );
        ensure!(
            checksums
                .insert(name.to_string(), digest.to_string())
                .is_none(),
            "release checksum path is duplicated"
        );
    }
    ensure!(
        checksums.keys().cloned().collect::<BTreeSet<_>>() == checksummed_names,
        "release checksum inventory is incomplete"
    );
    for (name, digest) in &checksums {
        ensure!(
            hash_file(&root.join(name))? == *digest,
            "release asset checksum does not match"
        );
    }

    let metadata: ReleaseMetadata = serde_json::from_slice(&fs::read(root.join(&metadata_name))?)?;
    ensure!(
        metadata.schema_version == RELEASE_METADATA_SCHEMA,
        "unsupported release metadata schema"
    );
    ensure!(
        metadata.release_tag == release_tag && metadata.source_commit == source_commit,
        "release metadata identity does not match"
    );
    let mut payloads = BTreeMap::new();
    for payload in metadata.payloads {
        validate_sha256(&payload.sha256)?;
        ensure!(
            payload_names.contains(&payload.name),
            "release metadata payload is unexpected"
        );
        ensure!(
            payloads.insert(payload.name, payload.sha256).is_none(),
            "release metadata payload is duplicated"
        );
    }
    let expected_payloads = payload_names
        .into_iter()
        .map(|name| {
            let digest = checksums
                .get(&name)
                .expect("payload checksum was validated")
                .clone();
            (name, digest)
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        payloads == expected_payloads,
        "release metadata payload inventory does not match checksums"
    );
    Ok(())
}

impl<P: Platform> DeploymentManager<P> {
    pub fn new(root: PathBuf, platform: P) -> Self {
        Self { root, platform }
    }

    pub fn receive(
        &self,
        request_id: &str,
        reader: impl Read,
        options: ReceiveOptions,
    ) -> Result<DeploymentStatus> {
        validate_request_id(request_id)?;
        self.initialize_layout()?;
        let request = self.request_path(request_id);
        fs::create_dir(&request).context("deployment request already exists")?;
        sync_directory(request.parent().expect("request directory has a parent"))?;
        let candidate = request.join("candidate");
        fs::create_dir(&candidate)?;

        let mut durable = false;
        let result = (|| {
            unpack_candidate(reader, &candidate)?;
            let manifest = validate_candidate(&candidate)?;
            sync_tree(&candidate)?;
            let journal = Journal {
                schema_version: JOURNAL_SCHEMA,
                request_id: request_id.to_string(),
                release_tag: manifest.release_tag,
                source_commit: manifest.source_commit,
                previous_release: None,
                previous_release_captured: false,
                phase: Phase::Received,
                candidate_data_access_started: false,
                admission_started: false,
                backup_created: false,
                failure: None,
            };
            self.save_journal(&journal)?;
            durable = true;
            if options.start_worker {
                self.platform.start_worker(request_id)?;
            }
            Ok(DeploymentStatus::from(journal))
        })();

        if result.is_err() && !durable {
            let _ = remove_tree(&request);
        }
        result
    }

    pub fn status(&self, request_id: &str) -> Result<DeploymentStatus> {
        validate_request_id(request_id)?;
        Ok(self.load_journal(request_id)?.into())
    }

    pub fn run(&self, request_id: &str) -> Result<DeploymentStatus> {
        validate_request_id(request_id)?;
        self.initialize_layout()?;
        let _lock = self.lock()?;
        let mut journal = self.load_journal(request_id)?;
        validate_journal(&journal, request_id)?;
        if journal.phase.terminal() {
            return Ok(journal.into());
        }
        if journal.admission_started {
            self.mark_manual_recovery(&mut journal, FailureCategory::Recovery)?;
            return Ok(journal.into());
        }
        if journal.candidate_data_access_started {
            self.rollback(&mut journal, FailureCategory::Recovery)?;
            return Ok(journal.into());
        }

        if let Err(error) = self.execute(&mut journal) {
            let category = category_for_phase(journal.phase);
            if let Err(recovery_error) = self.handle_failure(&mut journal, category) {
                self.mark_manual_recovery(&mut journal, FailureCategory::Recovery)?;
                return Err(error.context(recovery_error));
            }
            return Err(error);
        }
        Ok(journal.into())
    }

    pub fn boot_recover(&self) -> Result<()> {
        self.initialize_layout()?;
        let _lock = self.lock()?;
        let requests = self.root.join("requests");
        let mut ids = Vec::new();
        for entry in fs::read_dir(requests)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Some(id) = entry.file_name().to_str()
                && validate_request_id(id).is_ok()
            {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        let mut failed = false;
        for id in ids {
            let mut journal = self.load_journal(&id)?;
            validate_journal(&journal, &id)?;
            if journal.phase.terminal() {
                continue;
            }

            if !journal.previous_release_captured {
                journal.previous_release = self.current_release()?;
                journal.previous_release_captured = true;
                self.save_journal(&journal)?;
            }

            // `execute` persists `Maintenance` before its first service or
            // symlink mutation. A durable `Received` request therefore has
            // nothing to undo and must not disturb the live deployment.
            if journal.phase == Phase::Received {
                journal.phase = if journal.previous_release.is_some() {
                    Phase::RolledBack
                } else {
                    Phase::FirstInstallFailed
                };
                journal.failure = Some(FailureCategory::Recovery);
                self.save_journal(&journal)?;
                continue;
            }

            self.switch_caddy("maintenance")?;
            if journal.admission_started {
                self.mark_manual_recovery(&mut journal, FailureCategory::Recovery)?;
                failed = true;
            } else if journal.candidate_data_access_started {
                if !journal.backup_created || self.restore_data(&journal.request_id).is_err() {
                    self.mark_manual_recovery(&mut journal, FailureCategory::Recovery)?;
                    failed = true;
                    continue;
                }
                if let Some(previous) = &journal.previous_release {
                    self.switch_release(previous)?;
                    journal.phase = Phase::RolledBack;
                } else {
                    journal.phase = Phase::FirstInstallFailed;
                }
                journal.failure = Some(FailureCategory::Recovery);
                self.save_journal(&journal)?;
            } else {
                if let Some(previous) = &journal.previous_release {
                    self.switch_release(previous)?;
                    journal.phase = Phase::RolledBack;
                } else {
                    journal.phase = Phase::FirstInstallFailed;
                }
                journal.failure = Some(FailureCategory::Recovery);
                self.save_journal(&journal)?;
            }
        }
        ensure!(!failed, "one or more deployments require manual recovery");
        Ok(())
    }

    fn execute(&self, journal: &mut Journal) -> Result<()> {
        if !journal.previous_release_captured {
            journal.previous_release = self.current_release()?;
            journal.previous_release_captured = true;
            self.save_journal(journal)?;
        }

        journal.phase = Phase::Maintenance;
        self.save_journal(journal)?;
        self.switch_caddy("maintenance")?;
        self.platform.reload_caddy()?;
        self.platform.stop_server()?;

        self.back_up_data(&journal.request_id)?;
        journal.backup_created = true;
        journal.phase = Phase::BackupVerified;
        self.save_journal(journal)?;

        self.publish_candidate(journal)?;
        journal.phase = Phase::CandidatePublished;
        self.save_journal(journal)?;

        journal.candidate_data_access_started = true;
        journal.phase = Phase::CandidateDataAccessStarted;
        self.save_journal(journal)?;
        self.switch_release(&journal.release_tag)?;
        self.platform.restart_server()?;
        self.platform
            .wait_until_ready(&journal.release_tag, &journal.source_commit)?;
        journal.phase = Phase::CandidateReady;
        self.save_journal(journal)?;

        journal.admission_started = true;
        journal.phase = Phase::AdmissionStarted;
        self.save_journal(journal)?;
        self.switch_caddy("live")?;
        self.platform.reload_caddy()?;

        journal.phase = Phase::Complete;
        journal.failure = None;
        self.save_journal(journal)?;
        Ok(())
    }

    fn handle_failure(&self, journal: &mut Journal, category: FailureCategory) -> Result<()> {
        if journal.admission_started {
            return self.mark_manual_recovery(journal, FailureCategory::Admission);
        }
        if journal.candidate_data_access_started {
            return self.rollback(journal, category);
        }

        self.switch_caddy("maintenance")?;
        self.platform.reload_caddy()?;
        if let Some(previous) = &journal.previous_release {
            self.switch_release(previous)?;
            self.platform.restart_server()?;
            journal.phase = Phase::RolledBack;
        } else {
            let _ = self.platform.stop_server();
            journal.phase = Phase::FirstInstallFailed;
        }
        journal.failure = Some(category);
        self.save_journal(journal)
    }

    fn rollback(&self, journal: &mut Journal, category: FailureCategory) -> Result<()> {
        ensure!(journal.backup_created, "verified backup is unavailable");
        self.switch_caddy("maintenance")?;
        self.platform.reload_caddy()?;
        self.platform.stop_server()?;
        self.restore_data(&journal.request_id)?;
        if let Some(previous) = &journal.previous_release {
            self.switch_release(previous)?;
            self.platform.restart_server()?;
            journal.phase = Phase::RolledBack;
        } else {
            journal.phase = Phase::FirstInstallFailed;
        }
        journal.failure = Some(category);
        self.save_journal(journal)
    }

    fn mark_manual_recovery(&self, journal: &mut Journal, category: FailureCategory) -> Result<()> {
        journal.phase = Phase::ManualRecovery;
        journal.failure = Some(category);
        self.save_journal(journal)
    }

    fn initialize_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        for directory in [
            "backups",
            "caddy",
            "config",
            "configurations",
            "releases",
            "requests",
            "state",
            "data",
        ] {
            fs::create_dir_all(self.root.join(directory))?;
        }
        sync_directory(&self.root)
    }

    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("state/deploy.lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn request_path(&self, request_id: &str) -> PathBuf {
        self.root.join("requests").join(request_id)
    }

    fn journal_path(&self, request_id: &str) -> PathBuf {
        self.request_path(request_id).join("journal.json")
    }

    fn load_journal(&self, request_id: &str) -> Result<Journal> {
        let bytes =
            fs::read(self.journal_path(request_id)).context("deployment journal missing")?;
        serde_json::from_slice(&bytes).context("deployment journal is invalid")
    }

    fn save_journal(&self, journal: &Journal) -> Result<()> {
        write_json(&self.journal_path(&journal.request_id), journal)
    }

    fn current_release(&self) -> Result<Option<String>> {
        let path = self.root.join("releases/current");
        let target = match fs::read_link(path) {
            Ok(target) => target,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let name = target
            .file_name()
            .and_then(OsStr::to_str)
            .context("current release link is invalid")?;
        validate_release_tag(name)?;
        Ok(Some(name.to_string()))
    }

    fn publish_candidate(&self, journal: &Journal) -> Result<()> {
        let source = self.request_path(&journal.request_id).join("candidate");
        let destination = self.root.join("releases").join(&journal.release_tag);
        let requested = validate_candidate(&source)?;
        if destination.exists() {
            let existing = validate_candidate(&destination)?;
            ensure!(
                existing == requested,
                "release generation conflicts with the request"
            );
        } else {
            fs::rename(&source, &destination)?;
            sync_directory(destination.parent().expect("release has a parent"))?;
        }
        self.publish_configuration(journal)
    }

    fn publish_configuration(&self, journal: &Journal) -> Result<()> {
        let source = self.root.join("config");
        ensure!(source.is_dir(), "deployment configuration is unavailable");
        let destination = self.root.join("configurations").join(&journal.release_tag);
        let expected = backup_inventory(&source)?;
        if destination.exists() {
            ensure!(
                expected == backup_inventory(&destination)?,
                "configuration generation conflicts with the release"
            );
            return Ok(());
        }
        let staged = self.request_path(&journal.request_id).join("configuration");
        copy_tree(&source, &staged)?;
        ensure!(
            expected == backup_inventory(&staged)?,
            "configuration verification failed"
        );
        fs::rename(&staged, &destination)?;
        sync_directory(destination.parent().expect("configuration has a parent"))
    }

    fn switch_release(&self, release: &str) -> Result<()> {
        validate_release_tag(release)?;
        ensure!(
            self.root.join("releases").join(release).is_dir(),
            "release generation is unavailable"
        );
        ensure!(
            self.root.join("configurations").join(release).is_dir(),
            "configuration generation is unavailable"
        );
        replace_symlink(
            Path::new(release),
            &self.root.join("configurations/current"),
        )?;
        replace_symlink(Path::new(release), &self.root.join("releases/current"))
    }

    fn switch_caddy(&self, mode: &str) -> Result<()> {
        ensure!(matches!(mode, "live" | "maintenance"), "invalid Caddy mode");
        ensure!(
            self.root.join("caddy").join(mode).is_dir(),
            "Caddy mode is unavailable"
        );
        replace_symlink(Path::new(mode), &self.root.join("caddy/current"))
    }

    fn back_up_data(&self, request_id: &str) -> Result<()> {
        let backup = self.root.join("backups").join(request_id);
        ensure!(!backup.exists(), "backup generation already exists");
        fs::create_dir(&backup)?;
        let expected = copy_tree_verified(&self.root.join("data"), &backup.join("data"))?;
        write_json(&backup.join("inventory.json"), &expected)?;
        sync_tree(&backup)?;
        sync_directory(backup.parent().expect("backup has a parent"))
    }

    fn restore_data(&self, request_id: &str) -> Result<()> {
        let backup = self.root.join("backups").join(request_id);
        let expected: Vec<BackupEntry> =
            serde_json::from_slice(&fs::read(backup.join("inventory.json"))?)?;
        ensure!(
            expected == backup_inventory(&backup.join("data"))?,
            "backup is no longer valid"
        );
        let data = self.root.join("data");
        clear_directory(&data)?;
        let backup_data = backup.join("data");
        fs::set_permissions(&data, fs::metadata(&backup_data)?.permissions())?;
        copy_tree_contents(&backup_data, &data)?;
        ensure!(
            expected == backup_inventory(&data)?,
            "restored data verification failed"
        );
        sync_tree(&data)
    }
}

fn validate_journal(journal: &Journal, request_id: &str) -> Result<()> {
    ensure!(
        journal.schema_version == JOURNAL_SCHEMA,
        "unsupported journal schema"
    );
    ensure!(journal.request_id == request_id, "journal request mismatch");
    validate_request_id(&journal.request_id)?;
    validate_release_tag(&journal.release_tag)?;
    validate_source_commit(&journal.source_commit)?;
    if let Some(previous) = &journal.previous_release {
        ensure!(
            journal.previous_release_captured,
            "journal previous release was not captured"
        );
        validate_release_tag(previous)?;
    }
    ensure!(
        journal.previous_release_captured || journal.phase == Phase::Received,
        "journal phase precedes previous release capture"
    );
    ensure!(
        !journal.admission_started || journal.candidate_data_access_started,
        "journal barriers are inconsistent"
    );
    Ok(())
}

fn category_for_phase(phase: Phase) -> FailureCategory {
    match phase {
        Phase::Received | Phase::Maintenance => FailureCategory::Backup,
        Phase::BackupVerified | Phase::CandidatePublished => FailureCategory::Candidate,
        Phase::CandidateDataAccessStarted | Phase::CandidateReady => FailureCategory::Service,
        Phase::AdmissionStarted => FailureCategory::Admission,
        Phase::Complete | Phase::RolledBack | Phase::FirstInstallFailed | Phase::ManualRecovery => {
            FailureCategory::Recovery
        }
    }
}

fn unpack_candidate(reader: impl Read, destination: &Path) -> Result<()> {
    unpack_candidate_bounded(reader, destination, MAX_ARCHIVE_BYTES)
}

fn unpack_candidate_bounded(reader: impl Read, destination: &Path, max_bytes: u64) -> Result<()> {
    let mut limited = reader.take(max_bytes + 1);
    {
        let mut archive = tar::Archive::new(&mut limited);
        let mut count = 0usize;
        let mut extracted_bytes = 0u64;
        let mut extracted_paths = BTreeSet::new();
        for entry in archive.entries()? {
            let mut entry = entry?;
            count += 1;
            ensure!(
                count <= MAX_ARCHIVE_ENTRIES,
                "candidate archive contains too many entries"
            );
            let path = entry.path()?.into_owned();
            let normalized_path = normalized_archive_path(&path)?;
            ensure!(
                extracted_paths.insert(normalized_path),
                "candidate archive contains a duplicate path"
            );
            let kind = entry.header().entry_type();
            ensure!(
                kind.is_file() || kind.is_dir(),
                "candidate archive contains an unsupported entry"
            );
            if kind.is_file() {
                extracted_bytes = extracted_bytes
                    .checked_add(entry.size())
                    .context("candidate file sizes overflowed")?;
                ensure!(
                    extracted_bytes <= max_bytes,
                    "candidate files exceed their size limit"
                );
            } else {
                ensure!(entry.size() == 0, "candidate directory has file content");
            }
            ensure!(
                entry.unpack_in(destination)?,
                "candidate archive path escaped destination"
            );
        }
    }
    io::copy(&mut limited, &mut io::sink())?;
    ensure!(
        limited.limit() > 0,
        "candidate archive exceeds its size limit"
    );
    Ok(())
}

fn validate_candidate(root: &Path) -> Result<ReleaseManifest> {
    let manifest: ReleaseManifest = serde_json::from_slice(&fs::read(root.join(MANIFEST_NAME))?)?;
    ensure!(
        manifest.schema_version == MANIFEST_SCHEMA,
        "unsupported release manifest schema"
    );
    validate_release_tag(&manifest.release_tag)?;
    validate_source_commit(&manifest.source_commit)?;
    ensure!(!manifest.files.is_empty(), "release manifest is empty");

    let mut expected = BTreeMap::new();
    for file in &manifest.files {
        let path = Path::new(&file.path);
        validate_relative_path(path)?;
        validate_sha256(&file.sha256)?;
        ensure!(
            file.path != MANIFEST_NAME,
            "release manifest cannot inventory itself"
        );
        ensure!(
            expected
                .insert(file.path.clone(), file.sha256.clone())
                .is_none(),
            "release manifest contains a duplicate path"
        );
    }
    for required in [
        "server/labello-server",
        "browser/index.html",
        "browser/release.json",
        "browser/MANIFEST.sha256",
    ] {
        ensure!(
            expected.contains_key(required),
            "release manifest is missing a required file"
        );
    }

    let actual = inventory_without(root, MANIFEST_NAME)?
        .into_iter()
        .map(|file| (file.path, file.sha256))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        expected == actual,
        "release file inventory does not match the manifest"
    );

    let browser: BrowserInventory =
        serde_json::from_slice(&fs::read(root.join("browser/release.json"))?)?;
    ensure!(
        browser.release_tag == manifest.release_tag
            && browser.source_commit == manifest.source_commit,
        "browser inventory does not match the release"
    );
    validate_browser_manifest(&root.join("browser"))?;
    let mode = fs::metadata(root.join("server/labello-server"))?
        .permissions()
        .mode();
    ensure!(mode & 0o111 != 0, "server executable is not executable");
    Ok(manifest)
}

fn validate_browser_manifest(browser_root: &Path) -> Result<()> {
    let text = fs::read_to_string(browser_root.join("MANIFEST.sha256"))?;
    let mut declared = BTreeMap::new();
    for line in text.lines() {
        let (digest, path) = line
            .split_once("  ")
            .context("browser manifest line is invalid")?;
        validate_sha256(digest)?;
        let path = path
            .strip_prefix("./")
            .context("browser manifest path is invalid")?;
        validate_relative_path(Path::new(path))?;
        ensure!(
            path != "MANIFEST.sha256",
            "browser manifest cannot inventory itself"
        );
        ensure!(
            declared
                .insert(path.to_string(), digest.to_string())
                .is_none(),
            "browser manifest contains a duplicate path"
        );
    }
    let actual = inventory_without(browser_root, "MANIFEST.sha256")?
        .into_iter()
        .map(|file| (file.path, file.sha256))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        declared == actual,
        "browser files do not match their manifest"
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrowserInventory {
    release_tag: String,
    source_commit: String,
}

fn inventory(root: &Path) -> Result<Vec<ManifestFile>> {
    inventory_without(root, "")
}

fn backup_inventory(root: &Path) -> Result<Vec<BackupEntry>> {
    let metadata = fs::symlink_metadata(root)?;
    ensure!(metadata.is_dir(), "managed root is not a directory");
    let mut entries = vec![BackupEntry {
        path: ".".to_string(),
        kind: BackupEntryKind::Directory,
        mode: metadata.permissions().mode() & 0o7777,
        sha256: None,
    }];
    collect_backup_entries(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn collect_backup_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<BackupEntry>,
) -> Result<()> {
    let mut children = fs::read_dir(current)?.collect::<std::io::Result<Vec<_>>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let file_type = child.file_type()?;
        ensure!(
            !file_type.is_symlink(),
            "symbolic links are not allowed in managed data"
        );
        let path = child.path();
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("managed path is not UTF-8")?
            .replace('\\', "/");
        let mode = child.metadata()?.permissions().mode() & 0o7777;
        if file_type.is_dir() {
            entries.push(BackupEntry {
                path: relative,
                kind: BackupEntryKind::Directory,
                mode,
                sha256: None,
            });
            collect_backup_entries(root, &path, entries)?;
        } else if file_type.is_file() {
            entries.push(BackupEntry {
                path: relative,
                kind: BackupEntryKind::File,
                mode,
                sha256: Some(hash_file(&path)?),
            });
        } else {
            bail!("managed data contains an unsupported file type");
        }
    }
    Ok(())
}

fn inventory_without(root: &Path, excluded: &str) -> Result<Vec<ManifestFile>> {
    let mut files = Vec::new();
    collect_files(root, root, excluded, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_files(
    root: &Path,
    current: &Path,
    excluded: &str,
    files: &mut Vec<ManifestFile>,
) -> Result<()> {
    let mut entries = fs::read_dir(current)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        ensure!(
            !file_type.is_symlink(),
            "symbolic links are not allowed in managed trees"
        );
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(root, &path, excluded, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root)?;
            let relative = relative
                .to_str()
                .context("managed path is not UTF-8")?
                .replace('\\', "/");
            if relative != excluded {
                files.push(ManifestFile {
                    path: relative,
                    sha256: hash_file(&path)?,
                });
            }
        } else {
            bail!("managed tree contains an unsupported file type");
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    ensure!(source.is_dir(), "source tree is unavailable");
    fs::create_dir(destination)?;
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    copy_tree_contents(source, destination)
}

fn copy_tree_verified(source: &Path, destination: &Path) -> Result<Vec<BackupEntry>> {
    copy_tree_verified_with(source, destination, copy_tree)
}

fn copy_tree_verified_with(
    source: &Path,
    destination: &Path,
    copy: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<Vec<BackupEntry>> {
    let expected = backup_inventory(source)?;
    copy(source, destination)?;
    ensure!(
        expected == backup_inventory(source)?,
        "managed data changed while the backup was copied"
    );
    ensure!(
        expected == backup_inventory(destination)?,
        "backup verification failed"
    );
    Ok(expected)
}

fn copy_tree_contents(source: &Path, destination: &Path) -> Result<()> {
    let mut entries = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "symbolic links are not allowed in managed data"
        );
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
            fs::set_permissions(&target, entry.metadata()?.permissions())?;
            File::open(&target)?.sync_all()?;
        } else {
            bail!("managed data contains an unsupported file type");
        }
    }
    sync_directory(destination)
}

fn clear_directory(directory: &Path) -> Result<()> {
    ensure!(directory.is_dir(), "data root is unavailable");
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "symbolic links are not allowed in managed data"
        );
        if kind.is_dir() {
            remove_tree(&entry.path())?;
        } else if kind.is_file() {
            fs::remove_file(entry.path())?;
        } else {
            bail!("managed data contains an unsupported file type");
        }
    }
    sync_directory(directory)
}

fn remove_tree(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "refusing to remove a symbolic link"
    );
    fs::remove_dir_all(path)?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn sync_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        } else if entry.file_type()?.is_file() {
            File::open(entry.path())?.sync_all()?;
        }
    }
    sync_directory(path)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("managed file has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_directory(parent)
}

fn replace_symlink(target: &Path, link: &Path) -> Result<()> {
    let parent = link.parent().context("managed link has no parent")?;
    let temporary = link.with_extension("next");
    if fs::symlink_metadata(&temporary).is_ok() {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    fs::rename(&temporary, link)?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn validate_request_id(value: &str) -> Result<()> {
    ensure!((1..=128).contains(&value.len()), "invalid request ID");
    ensure!(value != "." && value != "..", "invalid request ID");
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "invalid request ID"
    );
    Ok(())
}

fn validate_release_tag(value: &str) -> Result<()> {
    let version = value.strip_prefix('v').context("invalid release tag")?;
    let parts = version.split('.').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "invalid release tag");
    for part in parts {
        ensure!(
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid release tag"
        );
        ensure!(part == "0" || !part.starts_with('0'), "invalid release tag");
    }
    Ok(())
}

fn validate_source_commit(value: &str) -> Result<()> {
    ensure!(
        value.len() == 40
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "invalid source commit"
    );
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "invalid SHA-256 digest"
    );
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "empty managed path");
    for component in path.components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "unsafe managed path"
        );
    }
    Ok(())
}

fn validate_archive_path(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "empty archive path");
    for component in path.components() {
        ensure!(
            matches!(component, Component::CurDir | Component::Normal(_)),
            "unsafe archive path"
        );
    }
    Ok(())
}

fn normalized_archive_path(path: &Path) -> Result<PathBuf> {
    validate_archive_path(path)?;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if let Component::Normal(component) = component {
            normalized.push(component);
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    Ok(normalized)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn identifiers_are_bounded() {
        assert!(validate_request_id("run-123_abc.1").is_ok());
        for value in ["", ".", "..", "a/b", "a b"] {
            assert!(validate_request_id(value).is_err(), "{value}");
        }
    }

    #[test]
    fn archive_paths_accept_posix_root_but_not_parent_traversal() {
        assert!(validate_archive_path(Path::new(".")).is_ok());
        assert!(validate_archive_path(Path::new("./server/labello-server")).is_ok());
        assert!(validate_archive_path(Path::new("../outside")).is_err());
        assert!(validate_archive_path(Path::new("/absolute")).is_err());
    }

    #[test]
    fn release_tags_are_canonical() {
        assert!(validate_release_tag("v0.1.2").is_ok());
        for value in ["1.2.3", "v1.2", "v01.2.3", "v1.2.3-rc1"] {
            assert!(validate_release_tag(value).is_err(), "{value}");
        }
    }
}

#[cfg(test)]
mod transaction_tests;
