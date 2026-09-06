//! Private, synthetic data for real-browser verification. Never opens existing data.
use std::{collections::BTreeSet, error::Error, path::PathBuf};

use image::{ImageBuffer, Rgba};
use labello_domain::*;
use labello_storage::DatasetRepository;

#[tokio::main]
async fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.as_slice() {
        [destination, scenario] => create(destination, scenario).await,
        _ => Err("expected new destination and scenario".into()),
    };
    if result.is_err() {
        eprintln!("browser_fixture.failed");
        std::process::exit(1);
    }
}

async fn create(destination: &str, scenario: &str) -> Result<(), Box<dyn Error>> {
    if !matches!(scenario, "boxes" | "skeletons" | "review" | "migration") {
        return Err("unknown fixture scenario".into());
    }
    let root = PathBuf::from(destination);
    // create_dir, not create_dir_all: existing roots and symlinks are refused.
    std::fs::create_dir(&root)?;
    let dataset_id = DatasetId::from("browser-fixture");
    let user = UserId::from("synthetic-admin");
    let author = UserId::from("synthetic-author");
    let now = labello_domain::now();
    let class_id = ClassId::from("synthetic-shape");
    let skeleton = matches!(scenario, "skeletons" | "migration");
    let task_id = TaskId::from(if skeleton {
        "skeleton:synthetic"
    } else {
        "bounding_box:synthetic"
    });
    let guide_id = TaskId::from("bounding_box:guide");
    let mut metadata = DatasetMetadata::new(dataset_id.clone(), "Synthetic browser fixture", now);
    metadata.image_roots = vec!["images".to_owned()];
    metadata.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Synthetic shape".to_owned(),
        color: "#5eead4".to_owned(),
        description: Some("Repository-owned artificial verification fixture".to_owned()),
    });
    let task = TaskDefinition {
        task_id: task_id.clone(),
        name: if skeleton {
            "Synthetic skeletons"
        } else {
            "Synthetic boxes"
        }
        .to_owned(),
        annotation_type: if skeleton {
            AnnotationType::Skeleton
        } else {
            AnnotationType::BoundingBox
        },
        class_ids: vec![class_id.clone()],
        instructions: TutorialContent {
            title: "Synthetic fixture instructions".to_owned(),
            example_text: "Label the artificial shape. This is test data.".to_owned(),
            example_images: vec![],
        },
        skeleton: skeleton.then(|| SkeletonSpec {
            keypoints: ["top", "center", "bottom"]
                .into_iter()
                .map(|name| KeypointSpec {
                    name: name.to_owned(),
                    required: false,
                })
                .collect(),
            edges: vec![
                SkeletonEdge {
                    from: "top".to_owned(),
                    to: "center".to_owned(),
                },
                SkeletonEdge {
                    from: "center".to_owned(),
                    to: "bottom".to_owned(),
                },
            ],
            allow_hidden: true,
            allow_absent: true,
        }),
        review: ReviewConfig {
            required_reviews: 1,
            workflow: ReviewWorkflow::Approval,
            allow_reviewer_corrections: scenario != "migration",
            agreement_threshold: None,
        },
        prelabel_config_ids: vec![],
        manual_box_guide_migration: (scenario == "migration").then(|| ManualBoxGuideMigration {
            guide_task_id: guide_id.clone(),
            cardinality: MigrationCardinality::ExactlyOne,
            allow_exclusion: true,
            sequence: MigrationSequence::ImportedSpatialOrderV1,
        }),
        enabled: true,
    };
    if scenario == "migration" {
        let mut guide = task.clone();
        guide.task_id = guide_id.clone();
        guide.name = "Synthetic imported guides".to_owned();
        guide.annotation_type = AnnotationType::BoundingBox;
        guide.skeleton = None;
        guide.manual_box_guide_migration = None;
        guide.review = ReviewConfig::default();
        metadata.tasks.push(guide);
    }
    metadata.tasks.push(task);
    for identity in [&user, &author] {
        metadata.role_assignments.push(DatasetRoleAssignment {
            dataset_id: dataset_id.clone(),
            user_id: identity.clone(),
            roles: BTreeSet::from([
                DatasetRole::DataAdmin,
                DatasetRole::Annotator,
                DatasetRole::Reviewer,
            ]),
            assigned_at: now,
            assigned_by: None,
        });
    }
    let repo = DatasetRepository::new(root.join(dataset_id.as_str()));
    repo.initialize(metadata).await?;
    let image_dir = root.join(dataset_id.as_str()).join("images");
    std::fs::create_dir_all(&image_dir)?;
    for index in 0..4_u8 {
        let pixels = ImageBuffer::from_fn(800, 600, |x, y| {
            let checker = (x / 50 + y / 50) % 2 == 0;
            let level = if checker { 220 } else { 35 };
            Rgba([
                level,
                level,
                if x < 400 { level } else { 100 + index * 20 },
                255_u8,
            ])
        });
        pixels.save(image_dir.join(format!("synthetic-pattern-{index}.png")))?;
    }
    repo.ingest_images().await?;
    let images = repo.load_images_index().await?;
    for record in images.images_by_hash.values() {
        if scenario == "migration" {
            let import_id = ImportId::from("synthetic-import");
            let target = MigrationTarget {
                object_group_id: ObjectGroupId::from("synthetic-group"),
                guide_annotation_id: AnnotationId::from("synthetic-guide"),
                reserved_skeleton_annotation_id: AnnotationId::from("synthetic-skeleton"),
                sequence_index: 0,
            };
            let mut annotation = annotation(&guide_id, &class_id, &author, now);
            annotation.annotation_id = target.guide_annotation_id.clone();
            annotation.object_group_id = Some(target.object_group_id.clone());
            annotation.origin = AnnotationOrigin::Imported {
                imported: ImportedOrigin {
                    import_id: import_id.clone(),
                    source_profile: SourceProfile {
                        profile_id: "synthetic_browser_fixture_v1".to_owned(),
                        profile_version: 1,
                    },
                    source_namespace: "repository-fixture".to_owned(),
                    source_object_key: "shape".to_owned(),
                    geometry_provenance: ImportGeometryProvenance::Direct,
                },
            };
            annotation.revision_source = RevisionSource::Import {
                import_id: import_id.clone(),
            };
            let targets = vec![target];
            let hash = migration_target_set_hash(
                &MigrationHashContext {
                    dataset_id: &dataset_id,
                    image_id: &record.image_id,
                    guide_task_id: &guide_id,
                    target_task_id: &task_id,
                },
                &targets,
            )?;
            repo.append_payload(
                &record.image_id,
                &Actor {
                    user_id: author.clone(),
                    role: DatasetRole::DataAdmin,
                },
                EventPayload::ImportInitialized {
                    import_id,
                    annotations: vec![annotation],
                    task_initializations: vec![
                        ImportTaskInitialization {
                            task_id: guide_id.clone(),
                            coverage: ImportCoverage::Complete,
                            initial_state: TaskState {
                                task_id: guide_id.clone(),
                                status: TaskStatus::Completed,
                                outcome: Some(TaskOutcome::ImportedGroundTruth),
                                assigned_to: None,
                                completed_by: Some(author.clone()),
                                completed_at: Some(now),
                                updated_at: now,
                            },
                        },
                        ImportTaskInitialization {
                            task_id: task_id.clone(),
                            coverage: ImportCoverage::Incomplete,
                            initial_state: TaskState::new(task_id.clone(), now),
                        },
                    ],
                    migration_target_sets: vec![MigrationTargetSetInitialization {
                        dataset_id: dataset_id.clone(),
                        guide_task_id: guide_id.clone(),
                        target_task_id: task_id.clone(),
                        target_set_hash: hash,
                        targets,
                    }],
                },
            )
            .await?;
        } else if scenario == "review" {
            let actor = Actor {
                user_id: author.clone(),
                role: DatasetRole::DataAdmin,
            };
            repo.append_payload(
                &record.image_id,
                &actor,
                EventPayload::AnnotationVersionCreated {
                    annotation: annotation(&task_id, &class_id, &author, now),
                    previous_version: None,
                    reason: None,
                },
            )
            .await?;
            repo.append_payload(
                &record.image_id,
                &actor,
                EventPayload::TaskStateChanged {
                    task_state: TaskState {
                        task_id: task_id.clone(),
                        status: TaskStatus::Submitted,
                        outcome: None,
                        assigned_to: None,
                        completed_by: Some(author.clone()),
                        completed_at: Some(now),
                        updated_at: now,
                    },
                },
            )
            .await?;
        }
    }
    std::fs::write(
        root.join("synthetic-fixture.json"),
        serde_json::to_vec(&serde_json::json!({
            "fixture": "labello-browser-v1", "scenario": scenario, "images": images.image_count,
            "sourceDigest": blake3::hash(include_bytes!("browser_fixture.rs")).to_hex().to_string()
        }))?,
    )?;
    Ok(())
}

fn annotation(
    task: &TaskId,
    class: &ClassId,
    author: &UserId,
    timestamp: Timestamp,
) -> AnnotationVersion {
    AnnotationVersion {
        annotation_id: AnnotationId::from("synthetic-annotation"),
        version: 1,
        object_group_id: None,
        origin: AnnotationOrigin::native(),
        task_id: task.clone(),
        class_id: class.clone(),
        annotation_type: AnnotationType::BoundingBox,
        revision_source: RevisionSource::Human {
            action: HumanRevisionKind::Authored,
        },
        geometry: AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.25,
            y: 0.2,
            width: 0.4,
            height: 0.6,
        }),
        author_user_id: author.clone(),
        created_at: timestamp,
        updated_at: timestamp,
        deleted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labello_storage::assignment::{AssignmentContext, MigrationTargetExpectation};

    #[tokio::test]
    async fn synthetic_migration_supports_the_real_save_transaction() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("datasets");
        create(root.to_str().unwrap(), "migration").await.unwrap();
        let repo = DatasetRepository::new(root.join("browser-fixture"));
        let user = UserId::from("synthetic-admin");
        let task = TaskId::from("skeleton:synthetic");
        let assignment = repo
            .assign_next_image(&user, &task, AssignmentKind::Annotation)
            .await
            .unwrap()
            .unwrap();
        let skeleton = SkeletonGeometry {
            keypoints: vec![
                KeypointAnnotation {
                    name: "top".into(),
                    state: KeypointState::Visible,
                    point: Some(NormalizedPoint { x: 0.4, y: 0.3 }),
                },
                KeypointAnnotation {
                    name: "center".into(),
                    state: KeypointState::Hidden,
                    point: Some(NormalizedPoint { x: 0.5, y: 0.5 }),
                },
                KeypointAnnotation {
                    name: "bottom".into(),
                    state: KeypointState::Absent,
                    point: None,
                },
            ],
        };
        let result = repo
            .save_migration_skeleton(
                &user,
                AssignmentContext {
                    assignment_id: &assignment.assignment_id,
                    image_id: &assignment.image_id,
                    task_id: &task,
                    kind: AssignmentKind::Annotation,
                },
                None,
                &MigrationTargetExpectation {
                    object_group_id: ObjectGroupId::from("synthetic-group"),
                    expected_guide_annotation_version: 1,
                    expected_guide_deleted: false,
                    expected_disposition_version: 1,
                    expected_skeleton_version: None,
                },
                skeleton,
                "synthetic-command",
            )
            .await
            .unwrap();
        assert_eq!(result.progress.annotated, 1);
        assert_eq!(result.cursor, MigrationCursor::FullImage);
    }
}
