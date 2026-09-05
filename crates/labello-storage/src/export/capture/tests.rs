use super::*;
use labello_domain::{
    AnnotationType, AnnotationVersion, BoundingBox, ClassId, DatasetMetadata, DatasetRole,
    EventLogEntry, EventPayload, ExportProfile, ImagesIndex, LabelClass, ReviewConfig,
    ReviewWorkflow, TaskDefinition, TaskOutcome, TaskState, TaskStatus, TutorialContent, UserId,
    now,
};

pub(crate) async fn fixture() -> (tempfile::TempDir, DatasetRepository, ExportOptions) {
    let dir = tempfile::tempdir().unwrap();
    let repository = DatasetRepository::new(dir.path());
    let mut metadata = DatasetMetadata::new(DatasetId::from("export"), "Export", now());
    metadata.label_classes.push(LabelClass {
        class_id: "person".into(),
        name: "Person".into(),
        color: "#ffffff".into(),
        description: None,
    });
    metadata.tasks.push(TaskDefinition {
        task_id: "boxes".into(),
        name: "Boxes".into(),
        annotation_type: AnnotationType::BoundingBox,
        class_ids: vec![ClassId::from("person"), ClassId::from("other")],
        instructions: TutorialContent {
            title: String::new(),
            example_text: String::new(),
            example_images: vec![],
        },
        skeleton: None,
        review: ReviewConfig {
            workflow: ReviewWorkflow::None,
            ..ReviewConfig::default()
        },
        prelabel_config_ids: vec![],
        manual_box_guide_migration: None,
        enabled: true,
    });
    repository.initialize(metadata).await.unwrap();
    let mut index = ImagesIndex::default();
    for (i, name) in ["object", "empty", "pending"].into_iter().enumerate() {
        let path = format!("images/{name}.png");
        ::image::RgbImage::from_pixel(20, 20, ::image::Rgb([i as u8, 100, 200]))
            .save(dir.path().join(&path))
            .unwrap();
        let bytes = std::fs::read(dir.path().join(&path)).unwrap();
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let record = ImageRecord {
            image_id: name.into(),
            blake3: digest.clone(),
            canonical_path: path.clone(),
            known_paths: vec![path],
            duplicate_paths: vec![],
            file_name: format!("{name}.png"),
            byte_size: bytes.len() as u64,
            width: 20,
            height: 20,
            media_type: "image/png".into(),
            source_memberships: Some(vec!["val".into()]),
        };
        index.images_by_hash.insert(digest, record);
        if name != "pending" {
            let mut payloads = Vec::new();
            if name == "object" {
                payloads.push(EventPayload::AnnotationVersionCreated {
                    annotation: AnnotationVersion::native(
                        "annotation".into(),
                        "boxes".into(),
                        "person".into(),
                        AnnotationType::BoundingBox,
                        AnnotationGeometry::BoundingBox(BoundingBox {
                            x: 0.0,
                            y: 0.25,
                            width: 0.5,
                            height: 0.75,
                        }),
                        "author".into(),
                        now(),
                    ),
                    previous_version: None,
                    reason: None,
                });
            }
            payloads.push(EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: "boxes".into(),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some("author".into()),
                    completed_at: Some(now()),
                    updated_at: now(),
                },
            });
            write_events(&repository, name, payloads);
        }
    }
    repository.save_images_index(&index).await.unwrap();
    let options = ExportOptions {
        profile: ExportProfile::UltralyticsYoloDetectV1,
        classes: BTreeSet::from([ExportClassSelection {
            task_id: "boxes".into(),
            class_id: "person".into(),
        }]),
        fallback_split: ExportSplit::Train,
        split_choices: BTreeMap::new(),
    };
    (dir, repository, options)
}

fn write_events(repository: &DatasetRepository, image: &str, payloads: Vec<EventPayload>) {
    let image_id = ImageId::from(image);
    std::fs::create_dir_all(repository.annotations_dir(&image_id)).unwrap();
    let text = payloads
        .into_iter()
        .enumerate()
        .map(|(i, payload)| {
            serde_json::to_string(&EventLogEntry::new(
                i as u64 + 1,
                image_id.clone(),
                UserId::from("author"),
                DatasetRole::Annotator,
                now(),
                payload,
            ))
            .unwrap()
                + "\n"
        })
        .collect::<String>();
    std::fs::write(repository.events_path(&image_id), text).unwrap();
}

#[tokio::test]
async fn immutable_capture_preserves_verified_empty_split_hash_and_exact_event_cut() {
    let (_source, repository, options) = fixture().await;
    let spool = tempfile::tempdir().unwrap();
    let limits = ExportLimits::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let capture = prepare(
        repository.clone(),
        spool.path().into(),
        "job".into(),
        options,
        limits.clone(),
        Arc::clone(&cancel),
    )
    .await
    .unwrap();
    assert!(capture.summary.can_start());
    assert_eq!(
        (
            capture.summary.included_images,
            capture.summary.empty_images,
            capture.summary.objects,
            capture.summary.omitted_images
        ),
        (2, 1, 1, 1)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(spool.path().join("labello-export.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["omitted"][0]["reason"], "unfinished");
    for entry in manifest["images"].as_array().unwrap() {
        assert_eq!(entry["split"], "val");
        assert_eq!(
            entry["eventSequence"],
            if entry["imageId"] == "object" { 2 } else { 1 }
        );
        let bytes = std::fs::read(spool.path().join(entry["imagePath"].as_str().unwrap())).unwrap();
        assert_eq!(
            blake3::hash(&bytes).to_hex().as_str(),
            entry["originalBlake3"].as_str().unwrap()
        );
    }
    // Source annotations can advance after capture without changing the spool.
    let event_path = repository.events_path(&ImageId::from("object"));
    let mut events = repository
        .export_image_cut(&ImageId::from("object"), limits.max_metadata_bytes)
        .await
        .unwrap()
        .1;
    let mut annotation = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::AnnotationVersionCreated { annotation, .. } => Some(annotation.clone()),
            _ => None,
        })
        .unwrap();
    annotation.version = 2;
    annotation.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.2,
        y: 0.2,
        width: 0.2,
        height: 0.2,
    });
    events.push(EventLogEntry::new(
        3,
        "object".into(),
        "author".into(),
        DatasetRole::Annotator,
        now(),
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: Some(1),
            reason: None,
        },
    ));
    std::fs::write(
        event_path,
        events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    capture.verify_source(&limits, &cancel).unwrap();
    let archive = archive::build(
        spool.path(),
        tempfile::tempfile().unwrap(),
        &capture.files,
        &limits,
        &cancel,
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(archive).unwrap();
    let object = manifest["images"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["imageId"] == "object")
        .unwrap();
    let mut text = String::new();
    use std::io::Read;
    zip.by_name(object["labelPath"].as_str().unwrap())
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "0 0.250000000 0.625000000 0.500000000 0.750000000\n");
    assert!(!repository.state_path(&ImageId::from("object")).exists());
    let mut changed = repository.load_dataset().await.unwrap();
    changed.name = "Changed".into();
    repository.save_dataset(&changed).await.unwrap();
    assert_eq!(
        capture.verify_source(&limits, &cancel),
        Err(ExportFailure::SourceChanged)
    );
}

#[tokio::test]
async fn split_conflicts_and_unmapped_known_objects_block_the_entire_artifact() {
    let (_source, repository, options) = fixture().await;
    let mut index = repository.load_images_index().await.unwrap();
    for record in index.images_by_hash.values_mut() {
        if record.image_id.as_str() == "object" {
            record.source_memberships = Some(vec!["train".into(), "val".into()]);
        }
    }
    repository.save_images_index(&index).await.unwrap();
    let spool = tempfile::tempdir().unwrap();
    let capture = prepare(
        repository.clone(),
        spool.path().into(),
        "job".into(),
        options.clone(),
        ExportLimits::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert!(!capture.summary.can_start());
    assert_eq!(
        capture.summary.blockers[0].reason,
        ExportFailure::Policy(labello_domain::ExportPolicyError::SplitConflict)
    );
    let mut options = options;
    options
        .split_choices
        .insert("object".into(), ExportSplit::Test);
    let spool = tempfile::tempdir().unwrap();
    assert!(
        prepare(
            repository.clone(),
            spool.path().into(),
            "job".into(),
            options.clone(),
            ExportLimits::default(),
            Arc::new(AtomicBool::new(false))
        )
        .await
        .unwrap()
        .summary
        .can_start()
    );
    let mut events = repository
        .export_image_cut(&ImageId::from("object"), 4096)
        .await
        .unwrap()
        .1;
    if let EventPayload::AnnotationVersionCreated { annotation, .. } = &mut events[0].payload {
        annotation.class_id = "other".into();
    }
    write_events(
        &repository,
        "object",
        events.into_iter().map(|event| event.payload).collect(),
    );
    let spool = tempfile::tempdir().unwrap();
    let capture = prepare(
        repository,
        spool.path().into(),
        "job".into(),
        options,
        ExportLimits::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert_eq!(
        capture.summary.blockers[0].reason,
        ExportFailure::UnmappedObjects
    );
    assert!(!capture.summary.can_start());
}
