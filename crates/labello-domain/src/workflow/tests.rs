use crate::*;

struct Fixture {
    state: ImageState,
    events: Vec<EventLogEntry>,
    task: TaskId,
    annotation: AnnotationVersion,
    user: UserId,
    timestamp: Timestamp,
}

impl Fixture {
    fn new() -> Self {
        let timestamp = now();
        let user = UserId::from("partial_author");
        let task = TaskId::from("boxes");
        let annotation = AnnotationVersion::native(
            AnnotationId::from("object_a"),
            task.clone(),
            ClassId::from("ball"),
            AnnotationType::BoundingBox,
            AnnotationGeometry::BoundingBox(BoundingBox {
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.2,
            }),
            user.clone(),
            timestamp,
        );
        let mut fixture = Self {
            state: ImageState::new(ImageId::from("image")),
            events: vec![],
            task,
            annotation,
            user,
            timestamp,
        };
        fixture
            .append(EventPayload::AnnotationVersionCreated {
                annotation: fixture.annotation.clone(),
                previous_version: None,
                reason: None,
            })
            .unwrap();
        fixture
            .workflow(WorkflowEvent::Prepared {
                task_id: fixture.task.clone(),
                preparation: WorkflowPreparation {
                    objects: vec![fixture.object()],
                    ..Default::default()
                },
            })
            .unwrap();
        fixture
    }

    fn object(&self) -> WorkflowObject {
        WorkflowObject::Annotation {
            annotation_id: self.annotation.annotation_id.clone(),
        }
    }

    fn append(&mut self, payload: EventPayload) -> DomainResult<()> {
        let event = EventLogEntry::new(
            self.state.current_sequence + 1,
            self.state.image_id.clone(),
            self.user.clone(),
            DatasetRole::Annotator,
            self.timestamp,
            payload,
        );
        self.state.apply_event(&event)?;
        self.events.push(event);
        // Every published prefix must reproduce the complete derived state.
        assert_eq!(
            self.state,
            rebuild_state(self.state.image_id.clone(), &self.events).unwrap()
        );
        Ok(())
    }

    fn workflow(&mut self, event: WorkflowEvent) -> DomainResult<()> {
        self.append(EventPayload::Workflow {
            event: Box::new(event),
        })
    }

    fn open(&mut self) -> AssignmentId {
        let assignment_id = AssignmentId::generate();
        self.workflow(WorkflowEvent::AssignmentOpened {
            assignment: Assignment {
                assignment_id: assignment_id.clone(),
                image_id: self.state.image_id.clone(),
                task_id: self.task.clone(),
                assigned_to: self.user.clone(),
                kind: AssignmentKind::Annotation,
                status: AssignmentStatus::Active,
                expires_at: Some(self.timestamp + std::time::Duration::from_secs(1800)),
                created_at: self.timestamp,
                updated_at: self.timestamp,
            },
            context: WorkflowAssignmentContext {
                task_fingerprint: "fixture".into(),
                overview_fingerprint: None,
                item: WorkflowItem::Object {
                    object: self.object(),
                },
                review_target: None,
                review_exception: false,
                source_assignment_id: None,
            },
        })
        .unwrap();
        assignment_id
    }

    fn confirm(&mut self, id: &AssignmentId) -> DomainResult<()> {
        self.workflow(WorkflowEvent::ItemConfirmed {
            confirmation: WorkflowConfirmation {
                assignment_id: id.clone(),
                task_id: self.task.clone(),
                annotation: Some(WorkflowAnnotation {
                    annotation_id: self.annotation.annotation_id.clone(),
                    version: 1,
                }),
                review: None,
                reviewed_targets: vec![],
            },
        })
    }
}

#[test]
fn confirmation_requires_display_and_only_finishes_the_object() {
    let mut f = Fixture::new();
    let id = f.open();
    let before = f.state.clone();
    assert!(f.confirm(&id).is_err());
    assert_eq!(f.state, before);
    f.workflow(WorkflowEvent::ItemSeen {
        task_id: f.task.clone(),
        assignment_id: id.clone(),
    })
    .unwrap();
    f.confirm(&id).unwrap();
    assert!(f.state.workflow_pending_objects(&f.task).is_empty());
    assert!(!f.state.task_states.contains_key(&f.task));
    assert!(f.confirm(&id).is_err());
}

#[test]
fn skipped_partial_work_stays_pending_and_final_author_gets_review_exclusion() {
    let mut f = Fixture::new();
    let first = f.open();
    f.workflow(WorkflowEvent::ItemSeen {
        task_id: f.task.clone(),
        assignment_id: first,
    })
    .unwrap();
    let mut cancelled = f.state.assignments[0].clone();
    cancelled.status = AssignmentStatus::Cancelled;
    f.append(EventPayload::AssignmentUpdated {
        assignment: cancelled,
    })
    .unwrap();
    assert_eq!(f.state.workflow_pending_objects(&f.task), vec![f.object()]);
    f.user = UserId::from("finisher");
    let second = f.open();
    f.workflow(WorkflowEvent::ItemSeen {
        task_id: f.task.clone(),
        assignment_id: second.clone(),
    })
    .unwrap();
    f.confirm(&second).unwrap();
    let item = WorkflowItem::Object { object: f.object() };
    let target = ReviewTarget::AnnotationVersion {
        annotation_id: f.annotation.annotation_id.clone(),
        version: 1,
    };
    assert!(
        f.state
            .workflow_independent_reviewer(&item, &target, &UserId::from("partial_author"))
    );
    assert!(
        !f.state
            .workflow_independent_reviewer(&item, &target, &f.user)
    );
    assert!(!f.state.workflow_independent_reviewer(
        &WorkflowItem::Overview,
        &target,
        &UserId::from("partial_author")
    ));
    assert!(
        !f.state
            .workflow_independent_reviewer(&WorkflowItem::Overview, &target, &f.user)
    );
    let scores = |f: &Fixture| {
        let mut projection = stats::scoring::ScoringProjection::default();
        projection.record_image(&f.state, &f.events);
        let mut contributors = std::collections::BTreeMap::new();
        projection.finish(&mut contributors, &[]);
        contributors
    };
    let contributors = scores(&f);
    assert!(!contributors.contains_key(&UserId::from("partial_author")));
    assert_eq!(contributors[&f.user].history[0].score.labels, 1);
    assert_eq!(contributors[&f.user].history[0].score.labeling, 2200);
    // The later image submission cannot pay for an already confirmed object again.
    f.append(EventPayload::TaskStateChanged {
        task_state: TaskState {
            task_id: f.task.clone(),
            status: TaskStatus::Submitted,
            outcome: None,
            assigned_to: None,
            completed_by: Some(f.user.clone()),
            completed_at: Some(f.timestamp),
            updated_at: f.timestamp,
        },
    })
    .unwrap();
    assert_eq!(contributors, scores(&f));
}

#[test]
fn seen_objects_cannot_be_removed_and_unseen_claims_can_be_refreshed() {
    let mut f = Fixture::new();
    let id = f.open();
    f.workflow(WorkflowEvent::Prepared {
        task_id: f.task.clone(),
        preparation: WorkflowPreparation::default(),
    })
    .unwrap();
    f.workflow(WorkflowEvent::Prepared {
        task_id: f.task.clone(),
        preparation: WorkflowPreparation {
            objects: vec![f.object()],
            ..Default::default()
        },
    })
    .unwrap();
    f.workflow(WorkflowEvent::ItemSeen {
        task_id: f.task.clone(),
        assignment_id: id,
    })
    .unwrap();
    let before = f.state.clone();
    assert!(
        f.workflow(WorkflowEvent::Prepared {
            task_id: f.task.clone(),
            preparation: WorkflowPreparation::default()
        })
        .is_err()
    );
    assert_eq!(before, f.state);
}

#[test]
fn queue_events_round_trip_in_v3_and_cannot_be_written_as_v2() {
    let f = Fixture::new();
    let event = f.events.last().unwrap();
    let encoded = serde_json::to_string(event).unwrap();
    assert_eq!(
        serde_json::from_str::<EventLogEntry>(&encoded).unwrap(),
        *event
    );
    let mut legacy = event.clone();
    legacy.schema_version = LEGACY_SCHEMA_VERSION;
    assert!(legacy.validate_shape().is_err());
    assert!(serde_json::to_string(&legacy).is_err());
}

#[test]
fn queue_configuration_defaults_to_five_history_items_and_no_overview_cap() {
    let config: WorkflowQueueConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(config.history_depth, 5);
    assert_eq!(config.max_pending_overviews, None);
    config.validate().unwrap();
    assert!(
        WorkflowQueueConfig {
            history_depth: 101,
            ..config.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        WorkflowQueueConfig {
            max_pending_overviews: Some(0),
            ..config
        }
        .validate()
        .is_err()
    );
}

#[test]
fn revising_a_confirmed_object_outside_overview_requires_confirmation() {
    let mut f = Fixture::new();
    let id = f.open();
    f.workflow(WorkflowEvent::ItemSeen {
        task_id: f.task.clone(),
        assignment_id: id.clone(),
    })
    .unwrap();
    f.confirm(&id).unwrap();
    let mut annotation = f.annotation.clone();
    annotation.version = 2;
    if let AnnotationGeometry::BoundingBox(bbox) = &mut annotation.geometry {
        bbox.x += 0.1;
    }
    f.append(EventPayload::AnnotationVersionCreated {
        annotation,
        previous_version: Some(1),
        reason: None,
    })
    .unwrap();
    assert_eq!(f.state.workflow_pending_objects(&f.task), vec![f.object()]);
    assert!(f.state.workflow_overview_versions.is_empty());
}
