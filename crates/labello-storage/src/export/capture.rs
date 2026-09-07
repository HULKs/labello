use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use labello_domain::{
    AnnotationGeometry, AnnotationId, AnnotationOrigin, DatasetId, ExportClassSelection,
    ExportOptions, ExportSplit, ImageId, ImageRecord, ObjectGroupId, RevisionSource, Timestamp,
};
use serde::Serialize;

use crate::DatasetRepository;

use super::{
    ExportBlocker, ExportFailure, ExportLimits, ExportOmittedImage, ExportSummary,
    archive::{self, CapturedFile},
    encoding::{self, Row, RowTrace},
    image,
    source::Source,
};

pub(super) struct Capture {
    pub repository: DatasetRepository,
    source: Arc<Source>,
    originals: Vec<ImageRecord>,
    pub files: Vec<CapturedFile>,
    pub summary: ExportSummary,
}

impl Capture {
    pub fn dataset_id(&self) -> &DatasetId {
        &self.source.metadata.dataset_id
    }
    pub fn verify_source(
        &self,
        limits: &ExportLimits,
        cancel: &AtomicBool,
    ) -> Result<(), ExportFailure> {
        self.source.verify_configuration(limits)?;
        for image in &self.originals {
            self.source.verify_original(image, limits, cancel)?;
        }
        self.source.verify_configuration(limits)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format_version: u32,
    job_id: String,
    captured_at: Timestamp,
    dataset_id: DatasetId,
    configuration_blake3: String,
    index_blake3: String,
    options: ExportOptions,
    summary: ExportSummary,
    images: Vec<ManifestImage>,
    omitted: Vec<ExportOmittedImage>,
    /// Payload checksums. checksums.json additionally covers this manifest.
    files: Vec<CapturedFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestImage {
    image_id: ImageId,
    original_blake3: String,
    width: u32,
    height: u32,
    image_path: String,
    label_path: String,
    split: ExportSplit,
    source_memberships: Vec<String>,
    event_sequence: u64,
    rows: Vec<RowTrace>,
    provenance: Vec<Provenance>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Provenance {
    annotation_id: AnnotationId,
    object_group_id: Option<ObjectGroupId>,
    origin: AnnotationOrigin,
    revision_source: RevisionSource,
    linked_box: Option<(AnnotationId, u32)>,
}

struct PayloadWriter {
    root: PathBuf,
    files: Vec<CapturedFile>,
    metadata_bytes: u64,
    manifest_bytes: u64,
}

struct JsonOutput<'a> {
    writer: BufWriter<File>,
    bytes: u64,
    maximum: u64,
    cancel: &'a AtomicBool,
    failure: Option<ExportFailure>,
    digest: blake3::Hasher,
}

impl Write for JsonOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let failure = if self.cancel.load(Ordering::Acquire) {
            Some(ExportFailure::Cancelled)
        } else if self
            .bytes
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > self.maximum)
        {
            Some(ExportFailure::Limit)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(std::io::Error::other("export metadata write stopped"));
        }
        match self.writer.write(bytes) {
            Ok(count) => {
                self.bytes += count as u64;
                self.digest.update(&bytes[..count]);
                Ok(count)
            }
            Err(error) => {
                self.failure = Some(ExportFailure::Storage);
                Err(error)
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl PayloadWriter {
    fn metadata(
        &mut self,
        name: &str,
        bytes: &[u8],
        limits: &ExportLimits,
    ) -> Result<(), ExportFailure> {
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(ExportFailure::Limit)?;
        if self.metadata_bytes > limits.max_metadata_bytes.unwrap_or(u64::MAX)
            || bytes.len() as u64 > limits.max_file_bytes.unwrap_or(u64::MAX)
        {
            return Err(ExportFailure::Limit);
        }
        let mut file = private_file(&self.root, name)?;
        file.write_all(bytes).map_err(|_| ExportFailure::Storage)?;
        self.record(
            CapturedFile {
                path: name.into(),
                bytes: bytes.len() as u64,
                blake3: blake3::hash(bytes).to_hex().to_string(),
            },
            limits,
        )
    }

    fn json(
        &mut self,
        name: &str,
        value: &impl Serialize,
        limits: &ExportLimits,
        cancel: &AtomicBool,
    ) -> Result<(), ExportFailure> {
        let maximum = limits
            .max_metadata_bytes
            .unwrap_or(u64::MAX)
            .saturating_sub(self.metadata_bytes)
            .min(limits.max_file_bytes.unwrap_or(u64::MAX));
        let mut output = JsonOutput {
            writer: BufWriter::new(private_file(&self.root, name)?),
            bytes: 0,
            maximum,
            cancel,
            failure: None,
            digest: blake3::Hasher::new(),
        };
        if serde_json::to_writer(&mut output, value).is_err() {
            return Err(output.failure.unwrap_or(ExportFailure::InvalidInput));
        }
        output.writer.flush().map_err(|_| ExportFailure::Storage)?;
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(output.bytes)
            .ok_or(ExportFailure::Limit)?;
        self.record(
            CapturedFile {
                path: name.into(),
                bytes: output.bytes,
                blake3: output.digest.finalize().to_hex().to_string(),
            },
            limits,
        )
    }

    fn record(&mut self, file: CapturedFile, limits: &ExportLimits) -> Result<(), ExportFailure> {
        if self.files.len() >= limits.max_files.unwrap_or(usize::MAX) {
            return Err(ExportFailure::Limit);
        }
        self.files.push(file);
        Ok(())
    }

    fn account_manifest(
        &mut self,
        value: &impl Serialize,
        limits: &ExportLimits,
    ) -> Result<(), ExportFailure> {
        if limits.max_metadata_bytes.is_none() {
            return Ok(());
        }
        self.manifest_bytes = self
            .manifest_bytes
            .checked_add(
                serde_json::to_vec(value)
                    .map_err(|_| ExportFailure::InvalidInput)?
                    .len() as u64,
            )
            .ok_or(ExportFailure::Limit)?;
        if self.manifest_bytes > limits.max_metadata_bytes.unwrap_or(u64::MAX) {
            return Err(ExportFailure::Limit);
        }
        Ok(())
    }
}

/// The service supplies a newly created private directory, never a source dataset path.
pub(super) async fn prepare(
    repository: DatasetRepository,
    root: PathBuf,
    job_id: String,
    options: ExportOptions,
    limits: ExportLimits,
    cancel: Arc<AtomicBool>,
) -> Result<Capture, ExportFailure> {
    let source_path = repository.root().to_path_buf();
    let read_limits = limits.clone();
    let source = Arc::new(
        tokio::task::spawn_blocking(move || Source::open(&source_path, &read_limits))
            .await
            .map_err(|_| ExportFailure::Storage)??,
    );
    let mapping = options
        .class_mapping(&source.metadata)
        .map_err(ExportFailure::Policy)?;
    if options.split_choices.len() > limits.max_images.unwrap_or(usize::MAX)
        || options
            .split_choices
            .keys()
            .any(|id| !source.metadata.images.contains_key(id))
    {
        return Err(ExportFailure::InvalidInput);
    }
    let tasks = options
        .classes
        .iter()
        .map(|selection| &selection.task_id)
        .collect::<BTreeSet<_>>();
    let mut writer = PayloadWriter {
        root,
        files: vec![],
        metadata_bytes: 0,
        manifest_bytes: 0,
    };
    let mut summary = ExportSummary {
        classes: mapping.clone(),
        ..ExportSummary::default()
    };
    let mut omitted = Vec::new();
    let mut images = Vec::new();
    let mut originals = Vec::new();
    let mut attempted_source_bytes = 0_u64;
    let mut split_lists = BTreeMap::<ExportSplit, String>::new();
    for record in source.metadata.images.values() {
        if cancel.load(Ordering::Acquire) {
            return Err(ExportFailure::Cancelled);
        }
        let split = match options.image_split(
            &record.image_id,
            record.source_memberships.as_deref().unwrap_or_default(),
        ) {
            Ok(split) => split,
            Err(reason) => {
                block(
                    &mut summary,
                    &record.image_id,
                    ExportFailure::Policy(reason),
                );
                continue;
            }
        };
        if !options.splits.contains(&split) {
            let reason = labello_domain::ExportOmissionReason::UnselectedSplit;
            let value = ExportOmittedImage {
                image_id: record.image_id.clone(),
                reason,
            };
            writer.account_manifest(&value, &limits)?;
            summary.omitted_images += 1;
            *summary.omission_counts.entry(reason).or_default() += 1;
            if summary.omitted_samples.len() < 100 {
                summary.omitted_samples.push(value.clone());
            }
            omitted.push(value);
            continue;
        }
        // Check directory identity per image without rereading the entire index.
        // Full configuration/index hashes are checked after capture and at publication.
        source.verify_directory()?;
        let (state, events) = repository
            .export_image_cut(
                &record.image_id,
                limits.max_metadata_bytes.unwrap_or(u64::MAX),
            )
            .await?;
        if let Some(reason) = tasks.iter().find_map(|task_id| {
            state.export_task_omission(
                source.metadata.task(task_id).expect("validated task"),
                &events,
            )
        }) {
            let value = ExportOmittedImage {
                image_id: record.image_id.clone(),
                reason,
            };
            writer.account_manifest(&value, &limits)?;
            summary.omitted_images += 1;
            *summary.omission_counts.entry(reason).or_default() += 1;
            if summary.omitted_samples.len() < 100 {
                summary.omitted_samples.push(value.clone());
            }
            omitted.push(value);
            continue;
        }
        let selected = state
            .active_annotations()
            .filter(|annotation| tasks.contains(&annotation.task_id))
            .collect::<Vec<_>>();
        let projection = (|| {
            let mut rows = Vec::new();
            let mut provenance = Vec::new();
            for annotation in selected {
                let selection = ExportClassSelection {
                    task_id: annotation.task_id.clone(),
                    class_id: annotation.class_id.clone(),
                };
                // Filtering a known object would turn selected coverage into false negatives.
                let class = mapping
                    .iter()
                    .find(|class| class.selection == selection)
                    .ok_or(ExportFailure::UnmappedObjects)?;
                annotation
                    .validate_for_task(
                        source
                            .metadata
                            .task(&annotation.task_id)
                            .expect("selected task"),
                        record.dimensions(),
                    )
                    .map_err(|_| ExportFailure::InvalidInput)?;
                let (bbox, derived_box, linked_box) = match annotation.geometry {
                    AnnotationGeometry::BoundingBox(bounds) => (bounds, false, None),
                    AnnotationGeometry::Skeleton(_) => {
                        let result = state
                            .export_pose_box(annotation, record.dimensions())
                            .map_err(ExportFailure::Policy)?;
                        (result.bounds, result.derived, result.linked_annotation)
                    }
                };
                rows.push(Row {
                    annotation,
                    class_index: class.index,
                    bbox,
                    derived_box,
                });
                provenance.push(Provenance {
                    annotation_id: annotation.annotation_id.clone(),
                    object_group_id: annotation.object_group_id.clone(),
                    origin: annotation.origin.clone(),
                    revision_source: annotation.revision_source.clone(),
                    linked_box,
                });
            }
            let (labels, traces) = encoding::labels(options.profile, &mut rows)?;
            Ok::<_, ExportFailure>((split, labels, traces, provenance))
        })();
        let (split, labels, traces, provenance) = match projection {
            Ok(value) => value,
            Err(reason) => {
                block(&mut summary, &record.image_id, reason);
                continue;
            }
        };
        let next_bytes = summary
            .source_bytes
            .checked_add(record.byte_size)
            .ok_or(ExportFailure::Limit)?;
        attempted_source_bytes = attempted_source_bytes
            .checked_add(record.byte_size)
            .ok_or(ExportFailure::Limit)?;
        if attempted_source_bytes > limits.max_source_bytes.unwrap_or(u64::MAX) {
            return Err(ExportFailure::Limit);
        }
        let temporary_name = format!("original-{}.tmp", record.blake3);
        let temporary = writer.root.join(&temporary_name);
        let mut output = private_file(&writer.root, &temporary_name)?;
        let copy_source = Arc::clone(&source);
        let copy_record = record.clone();
        let copy_limits = limits.clone();
        let copy_cancel = Arc::clone(&cancel);
        let validated = tokio::task::spawn_blocking(move || {
            copy_source.copy_original(&copy_record, &mut output, &copy_limits, &copy_cancel)?;
            image::validate(&mut output, &copy_record, &copy_limits)
        })
        .await
        .map_err(|_| ExportFailure::Storage)?;
        let extension = match validated {
            Ok(extension) => extension,
            Err(ExportFailure::UnsupportedImage) => {
                std::fs::remove_file(temporary).map_err(|_| ExportFailure::Storage)?;
                block(
                    &mut summary,
                    &record.image_id,
                    ExportFailure::UnsupportedImage,
                );
                continue;
            }
            Err(reason) => return Err(reason),
        };
        let (image_path, label_path) = encoding::image_paths(split, &record.blake3, extension)?;
        let destination = writer.root.join(&image_path);
        std::fs::create_dir_all(destination.parent().expect("image directory"))
            .map_err(|_| ExportFailure::Storage)?;
        std::fs::rename(temporary, &destination).map_err(|_| ExportFailure::Storage)?;
        writer.record(
            CapturedFile {
                path: image_path.clone(),
                bytes: record.byte_size,
                blake3: record.blake3.clone(),
            },
            &limits,
        )?;
        writer.metadata(&label_path, &labels, &limits)?;
        split_lists
            .entry(split)
            .or_default()
            .push_str(&format!("./{image_path}\n"));
        let entry = ManifestImage {
            image_id: record.image_id.clone(),
            original_blake3: record.blake3.clone(),
            width: record.width,
            height: record.height,
            image_path,
            label_path,
            split,
            source_memberships: record.source_memberships.clone().unwrap_or_default(),
            event_sequence: state.current_sequence,
            rows: traces,
            provenance,
        };
        writer.account_manifest(&entry, &limits)?;
        summary.included_images += 1;
        summary.empty_images += usize::from(entry.rows.is_empty());
        summary.objects += entry.rows.len();
        summary.source_bytes = next_bytes;
        images.push(entry);
        originals.push(record.clone());
    }
    for split in [ExportSplit::Train, ExportSplit::Val, ExportSplit::Test] {
        writer.metadata(
            &format!("{}.txt", split.as_str()),
            split_lists
                .get(&split)
                .map_or("\n", String::as_str)
                .as_bytes(),
            &limits,
        )?;
    }
    writer.metadata(
        "data.yaml",
        &encoding::descriptor(options.profile, &mapping)?,
        &limits,
    )?;
    let manifest = Manifest {
        format_version: 1,
        job_id,
        captured_at: labello_domain::now(),
        dataset_id: source.metadata.dataset_id.clone(),
        configuration_blake3: source.configuration_digest.clone(),
        index_blake3: source.index_digest.clone(),
        options,
        summary: summary.clone(),
        images,
        omitted,
        files: writer.files.clone(),
    };
    writer.json("labello-export.json", &manifest, &limits, &cancel)?;
    let files = writer.files.clone();
    writer.json("checksums.json", &files, &limits, &cancel)?;
    let capture = Capture {
        repository,
        source,
        originals,
        files: writer.files,
        summary,
    };
    let check_limits = limits.clone();
    tokio::task::spawn_blocking(move || {
        capture.verify_source(&check_limits, &cancel)?;
        Ok(capture)
    })
    .await
    .map_err(|_| ExportFailure::Storage)?
}

fn block(summary: &mut ExportSummary, image_id: &ImageId, reason: ExportFailure) {
    summary.blocking_images += 1;
    if summary.blockers.len() < 100 {
        summary.blockers.push(ExportBlocker {
            image_id: image_id.clone(),
            reason,
        });
    }
}

fn private_file(root: &Path, relative: &str) -> Result<File, ExportFailure> {
    archive::check_entry(relative)?;
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().ok_or(ExportFailure::InvalidInput)?)
        .map_err(|_| ExportFailure::Storage)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| ExportFailure::Storage)
}

#[cfg(test)]
pub(super) mod tests;
