use std::{collections::BTreeMap, path::Path};

use labello_domain::*;

use super::*;
use crate::{DatasetRepository, ImportService, import as importing};

fn task(id: &str, classes: &[&str], names: Option<&[&str]>) -> TaskDefinition {
    TaskDefinition {
        task_id: id.into(),
        name: id.into(),
        annotation_type: if names.is_some() {
            AnnotationType::Skeleton
        } else {
            AnnotationType::BoundingBox
        },
        class_ids: classes.iter().map(|id| ClassId::from(*id)).collect(),
        instructions: TutorialContent {
            title: id.into(),
            example_text: String::new(),
            example_images: vec![],
        },
        skeleton: names.map(|names| SkeletonSpec {
            keypoints: names
                .iter()
                .map(|name| KeypointSpec {
                    name: (*name).into(),
                    required: false,
                })
                .collect(),
            edges: vec![],
            allow_hidden: true,
            allow_absent: true,
        }),
        review: ReviewConfig {
            workflow: ReviewWorkflow::None,
            ..ReviewConfig::default()
        },
        prelabel_config_ids: vec![],
        manual_box_guide_migration: None,
        enabled: true,
    }
}

fn annotation(
    id: &str,
    task: &str,
    class: &str,
    geometry: AnnotationGeometry,
    group: Option<&str>,
) -> AnnotationVersion {
    let mut annotation = AnnotationVersion::native(
        id.into(),
        task.into(),
        class.into(),
        match geometry {
            AnnotationGeometry::BoundingBox(_) => AnnotationType::BoundingBox,
            AnnotationGeometry::Skeleton(_) => AnnotationType::Skeleton,
        },
        geometry,
        "native-author".into(),
        now(),
    );
    annotation.object_group_id = group.map(ObjectGroupId::from);
    annotation
}

fn pose(names: &[&str], points: &[Option<(f32, f32)>]) -> AnnotationGeometry {
    AnnotationGeometry::Skeleton(SkeletonGeometry {
        keypoints: names
            .iter()
            .zip(points)
            .enumerate()
            .map(|(i, (name, point))| KeypointAnnotation {
                name: (*name).into(),
                state: if point.is_none() {
                    KeypointState::Absent
                } else if i == 1 {
                    KeypointState::Hidden
                } else {
                    KeypointState::Visible
                },
                point: point.map(|(x, y)| NormalizedPoint { x, y }),
            })
            .collect(),
    })
}

async fn source(root: &Path) -> (DatasetRepository, Vec<AnnotationVersion>) {
    let repository = DatasetRepository::new(root.join("native"));
    let mut dataset = DatasetMetadata::new("native".into(), "Native", now());
    for id in ["class-a", "class-b"] {
        dataset.label_classes.push(LabelClass {
            class_id: id.into(),
            name: "Same display name".into(),
            color: "#ffffff".into(),
            description: None,
        });
    }
    let a = ["nose", "tail", "ear"];
    let b = ["tip", "base", "joint"];
    dataset.tasks = vec![
        task("boxes-a", &["class-a"], None),
        task("boxes-b", &["class-b"], None),
        task("pose-a", &["class-a"], Some(&a)),
        task("pose-b", &["class-b"], Some(&b)),
    ];
    repository.initialize(dataset.clone()).await.unwrap();
    let box_a = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.0,
        y: 0.1,
        width: 1.0,
        height: 0.8,
    });
    let box_b = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.9,
        y: 0.9,
        width: 0.1,
        height: 0.1,
    });
    let absent_box = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.1,
        y: 0.2,
        width: 0.3,
        height: 0.4,
    });
    let annotations = vec![
        annotation("box-a", "boxes-a", "class-a", box_a, Some("paired-a")),
        annotation("box-b", "boxes-b", "class-b", box_b, None),
        annotation(
            "box-absent",
            "boxes-a",
            "class-a",
            absent_box,
            Some("paired-absent"),
        ),
        annotation(
            "pose-a",
            "pose-a",
            "class-a",
            pose(&a, &[Some((0.2, 0.3)), Some((0.8, 0.8)), None]),
            Some("paired-a"),
        ),
        annotation(
            "pose-absent",
            "pose-a",
            "class-a",
            pose(&a, &[None, None, None]),
            Some("paired-absent"),
        ),
        annotation(
            "pose-b",
            "pose-b",
            "class-b",
            pose(&b, &[Some((0.9, 1.0)), Some((1.0, 1.0)), None]),
            None,
        ),
    ];
    let mut index = ImagesIndex::default();
    for (i, (id, split)) in [
        ("objects", "train"),
        ("empty", "val"),
        ("test-empty", "test"),
        ("pending", "train"),
    ]
    .into_iter()
    .enumerate()
    {
        let relative = format!("images/{id}.png");
        ::image::RgbImage::from_pixel(100, 80, ::image::Rgb([30 + i as u8, 60, 90]))
            .save(repository.root().join(&relative))
            .unwrap();
        let bytes = std::fs::read(repository.root().join(&relative)).unwrap();
        let hash = blake3::hash(&bytes).to_hex().to_string();
        index.images_by_hash.insert(
            hash.clone(),
            ImageRecord {
                image_id: id.into(),
                blake3: hash,
                canonical_path: relative.clone(),
                known_paths: vec![relative],
                duplicate_paths: vec![],
                file_name: "same-original-name.png".into(),
                byte_size: bytes.len() as u64,
                width: 100,
                height: 80,
                media_type: "image/png".into(),
                source_memberships: Some(vec![split.into()]),
            },
        );
        if id == "pending" {
            continue;
        }
        let mut payloads = if id == "objects" {
            annotations
                .iter()
                .map(|annotation| EventPayload::AnnotationVersionCreated {
                    annotation: annotation.clone(),
                    previous_version: None,
                    reason: None,
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        for task in &dataset.tasks {
            payloads.push(EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task.task_id.clone(),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some("native-author".into()),
                    completed_at: Some(now()),
                    updated_at: now(),
                },
            });
        }
        let image = ImageId::from(id);
        let events = payloads
            .into_iter()
            .enumerate()
            .map(|(i, payload)| {
                EventLogEntry::new(
                    i as u64 + 1,
                    image.clone(),
                    "native-author".into(),
                    DatasetRole::Annotator,
                    now(),
                    payload,
                )
            })
            .collect::<Vec<_>>();
        rebuild_state(image.clone(), &events).unwrap();
        std::fs::create_dir_all(repository.annotations_dir(&image)).unwrap();
        std::fs::write(
            repository.events_path(&image),
            events
                .iter()
                .map(|event| serde_json::to_string(event).unwrap() + "\n")
                .collect::<String>(),
        )
        .unwrap();
    }
    repository.save_images_index(&index).await.unwrap();
    (repository, annotations)
}

async fn settle(service: &ExportService, id: &str) -> ExportJob {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let job = service.job(&DatasetId::from("native"), id).await.unwrap();
            if !job.phase.is_active() {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

async fn reimport(
    root: &Path,
    extracted: &Path,
    profile: importing::ImportProfile,
    name: &str,
) -> (DatasetRepository, importing::ImportPlan) {
    let service = ImportService::new(
        root,
        importing::ImportConfig {
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let owner = UserId::from("admin");
    let job = service
        .create_job(
            owner.clone(),
            importing::CreateImportRequest {
                destination_dataset_id: name.into(),
                destination_name: name.into(),
                profile,
                transport: importing::ImportTransport::Browser,
            },
        )
        .await
        .unwrap();
    // Extraction precedes the production import flow; archive import is not supported.
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(extracted) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            files.insert(
                entry
                    .path()
                    .strip_prefix(extracted)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
                std::fs::read(entry.path()).unwrap(),
            );
        }
    }
    let registered = service
        .register_browser_files(
            &job.import_id,
            &owner,
            files
                .iter()
                .map(|(path, bytes)| importing::BrowserFileRegistration {
                    relative_path: path.clone(),
                    byte_size: bytes.len() as u64,
                    blake3: blake3::hash(bytes).to_hex().to_string(),
                })
                .collect(),
        )
        .await
        .unwrap();
    for file in registered {
        let bytes = &files[&file.relative_path];
        // Exported zero-row labels contain a newline and use the ordinary upload protocol.
        service
            .upload_chunk(
                &job.import_id,
                &owner,
                &file.file_id,
                0,
                bytes,
                blake3::hash(bytes).to_hex().as_str(),
            )
            .await
            .unwrap();
    }
    service.seal(&job.import_id, &owner).await.unwrap();
    let request = importing::PreflightRequest {
        descriptor_paths: vec!["data.yaml".into()],
        selected_splits: vec!["train".into(), "val".into(), "test".into()],
        coco_descriptors: vec![],
        ground_truth_attested: true,
        exhaustive_attested: true,
        source_namespace: "round-trip".into(),
        source_release: "v1".into(),
        coverage_scope: vec![],
        attestation_provenance: "synthetic export fixture".into(),
        intent: importing::ImportIntent::AuthoritativeGroundTruth,
        policies: importing::CompatibilityPolicies {
            yolo_zero_keypoints: importing::YoloZeroKeypointPolicy::PreserveAbsent,
            ..Default::default()
        },
        output: importing::OutputPolicy::defaults_for(profile),
        acknowledged_warning_codes: vec![],
        category_mappings: vec![],
        task_mappings: vec![],
        geometry_mappings: vec![],
    };
    let mut plan = service
        .preflight(&job.import_id, &owner, request)
        .await
        .unwrap();
    let warnings = plan
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.requires_acknowledgement)
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<Vec<_>>();
    // The importer discovers new source categories. Native class IDs recorded in
    // the manifest are not imported identities or exhaustive-coverage attestations.
    plan.request.coverage_scope = plan.source_categories.keys().cloned().collect();
    plan.request.acknowledged_warning_codes = warnings;
    plan = service
        .preflight(&job.import_id, &owner, plan.request)
        .await
        .unwrap();
    assert!(
        plan.committable(),
        "synthetic round-trip diagnostics: {:?}",
        plan.diagnostics
    );
    let result = service
        .commit(&job.import_id, &owner, &plan.plan_hash)
        .await
        .unwrap();
    (DatasetRepository::new(result.dataset_path), plan)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-6
}

fn geometry_matches(a: &AnnotationGeometry, b: &AnnotationGeometry) -> bool {
    match (a, b) {
        (AnnotationGeometry::BoundingBox(a), AnnotationGeometry::BoundingBox(b)) => {
            close(a.x, b.x)
                && close(a.y, b.y)
                && close(a.width, b.width)
                && close(a.height, b.height)
        }
        (AnnotationGeometry::Skeleton(a), AnnotationGeometry::Skeleton(b)) => {
            a.keypoints.len() == b.keypoints.len()
                && a.keypoints.iter().zip(&b.keypoints).all(|(a, b)| {
                    a.name == b.name
                        && a.state == b.state
                        && match (a.point, b.point) {
                            (None, None) => true,
                            (Some(a), Some(b)) => close(a.x, b.x) && close(a.y, b.y),
                            _ => false,
                        }
                })
        }
        _ => false,
    }
}

#[tokio::test]
async fn both_profiles_round_trip_through_production_import_with_explicit_losses() {
    let root = tempfile::tempdir().unwrap();
    let (source, annotations) = source(root.path()).await;
    if let Some(destination) = std::env::var_os("LABELLO_EXPORT_NATIVE_FIXTURE") {
        let destination = std::path::PathBuf::from(destination);
        std::fs::create_dir(&destination).unwrap();
        for entry in walkdir::WalkDir::new(source.root()).min_depth(1) {
            let entry = entry.unwrap();
            let target = destination.join(entry.path().strip_prefix(source.root()).unwrap());
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(target).unwrap();
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let original_index = source.load_images_index().await.unwrap();
    let export = ExportService::new(root.path(), ExportLimits::default())
        .await
        .unwrap();
    for (profile, import_profile, name) in [
        (
            ExportProfile::UltralyticsYoloDetectV1,
            importing::ImportProfile::UltralyticsYoloDetectV1,
            "detect",
        ),
        (
            ExportProfile::UltralyticsYoloPoseV1,
            importing::ImportProfile::UltralyticsYoloPoseV1,
            "pose",
        ),
    ] {
        let classes = if name == "detect" {
            vec![("boxes-a", "class-a"), ("boxes-b", "class-b")]
        } else {
            vec![("pose-a", "class-a"), ("pose-b", "class-b")]
        };
        let options = ExportOptions {
            profile,
            classes: classes
                .into_iter()
                .map(|(task, class)| ExportClassSelection {
                    task_id: task.into(),
                    class_id: class.into(),
                })
                .collect(),
            fallback_split: ExportSplit::Train,
            split_choices: BTreeMap::new(),
        };
        let job = export
            .preflight(&DatasetId::from("native"), source.clone(), options)
            .await
            .unwrap();
        let ready = settle(&export, &job.job_id).await;
        assert_eq!(ready.phase, ExportPhase::Ready, "{:?}", ready.summary);
        assert_eq!(ready.summary.as_ref().unwrap().omitted_images, 1);
        export
            .start(&DatasetId::from("native"), &job.job_id)
            .await
            .unwrap();
        assert_eq!(
            settle(&export, &job.job_id).await.phase,
            ExportPhase::Succeeded
        );
        let (file, completed, _permit) = export
            .download(&DatasetId::from("native"), &job.job_id)
            .await
            .unwrap();
        let extracted = tempfile::tempdir().unwrap();
        zip::ZipArchive::new(file)
            .unwrap()
            .extract(extracted.path())
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(extracted.path().join("labello-export.json")).unwrap(),
        )
        .unwrap();
        let (imported, plan) = reimport(root.path(), extracted.path(), import_profile, name).await;
        let metadata = imported.load_dataset().await.unwrap();
        assert_eq!(metadata.label_classes.len(), 2);
        assert!(
            metadata
                .label_classes
                .iter()
                .all(|class| class.name == "Same display name")
        );
        assert_ne!(
            metadata.label_classes[0].class_id,
            metadata.label_classes[1].class_id
        );
        let index = imported.load_images_index().await.unwrap();
        assert_eq!(index.images_by_hash.len(), 3);
        for (hash, record) in &index.images_by_hash {
            let original = &original_index.images_by_hash[hash];
            assert_eq!(record.source_memberships, original.source_memberships);
            assert_eq!(record.dimensions(), original.dimensions());
            assert_eq!(
                blake3::hash(&std::fs::read(imported.root().join(&record.canonical_path)).unwrap())
                    .to_hex()
                    .as_str(),
                hash
            );
            let state = imported.load_image_state(&record.image_id).await.unwrap();
            assert!(state.reviews.is_empty());
            assert!(state.reviewer_corrections.is_empty());
            let events = imported.load_events(&record.image_id).await.unwrap();
            assert_eq!(events.len(), 1);
            assert!(matches!(
                events[0].payload,
                EventPayload::ImportInitialized { .. }
            ));
            if original.image_id.as_str() != "objects" {
                assert_eq!(state.active_annotations().count(), 0);
                assert!(
                    state
                        .import_coverage
                        .values()
                        .all(|coverage| *coverage == ImportCoverage::VerifiedEmpty)
                );
                continue;
            }
            let mut actual = state
                .active_annotations()
                .filter(|annotation| annotation.annotation_type == profile.annotation_type())
                .collect::<Vec<_>>();
            assert_eq!(actual.len(), 3);
            for expected in annotations
                .iter()
                .filter(|annotation| annotation.annotation_type == profile.annotation_type())
            {
                let class = ready
                    .summary
                    .as_ref()
                    .unwrap()
                    .classes
                    .iter()
                    .find(|class| class.selection.class_id == expected.class_id)
                    .unwrap();
                let category_key = plan
                    .source_categories
                    .iter()
                    .find(|(_, category)| category.source_category_id == class.index.to_string())
                    .unwrap()
                    .0;
                let class_id = ClassId::from(plan.class_ids[category_key].clone());
                let position = actual
                    .iter()
                    .position(|annotation| {
                        annotation.class_id == class_id
                            && geometry_matches(&annotation.geometry, &expected.geometry)
                    })
                    .expect("geometry/class/state round-trip within 1e-6");
                let annotation = actual.remove(position);
                assert_ne!(annotation.annotation_id, expected.annotation_id);
                assert!(matches!(
                    annotation.origin,
                    AnnotationOrigin::Imported { .. }
                ));
            }
            assert!(actual.is_empty());
            if name == "pose" {
                // The importer materializes each YOLO pose row's source box as another
                // native annotation/task. These are new identities, including derived boxes.
                assert_eq!(
                    state
                        .active_annotations()
                        .filter(
                            |annotation| annotation.annotation_type == AnnotationType::BoundingBox
                        )
                        .count(),
                    3
                );
                let source_state = source.load_image_state(&original.image_id).await.unwrap();
                for expected in annotations
                    .iter()
                    .filter(|annotation| annotation.annotation_type == AnnotationType::Skeleton)
                {
                    let bounds = source_state
                        .export_pose_box(expected, original.dimensions())
                        .unwrap();
                    assert!(
                        state
                            .active_annotations()
                            .any(|annotation| geometry_matches(
                                &annotation.geometry,
                                &AnnotationGeometry::BoundingBox(bounds.bounds)
                            ))
                    );
                }
                let rows = manifest["images"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|image| image["imageId"] == "objects")
                    .unwrap()["rows"]
                    .as_array()
                    .unwrap();
                assert_eq!(
                    rows.iter().filter(|row| row["derivedBox"] == true).count(),
                    1
                );
            }
        }
        if let Some(evidence) = std::env::var_os("LABELLO_EXPORT_ROUND_TRIP_ARTIFACTS") {
            let destination = std::path::PathBuf::from(evidence).join(name);
            std::fs::create_dir(&destination).unwrap();
            for entry in walkdir::WalkDir::new(extracted.path()) {
                let entry = entry.unwrap();
                if !entry.file_type().is_file() {
                    continue;
                }
                let target = destination.join(entry.path().strip_prefix(extracted.path()).unwrap());
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::copy(entry.path(), target).unwrap();
            }
            std::fs::write(destination.join("round-trip-evidence.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "profile": profile, "productionExport": true, "productionReimport": true, "images": 3, "objects": 3,
                "verifiedEmptyImages": 2, "normalizedTolerance": 0.000001, "archiveBlake3": completed.archive_blake3,
                "nativeIdentityRestored": false, "nativeHistoryRestored": false
            })).unwrap()).unwrap();
        }
    }
}
