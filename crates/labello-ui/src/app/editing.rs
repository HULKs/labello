impl LabelloApp {
    pub(crate) fn replenish_demo_queue(&mut self) {
        while self.work.queue.len() < self.work.queue.queue_size() {
            let image = demo_image(self.work.next_demo_image_index);
            self.work.queue.push_if_room(image);
            self.work.next_demo_image_index += 1;
        }
        self.work.queue.set_loading(false);
    }

    pub(crate) fn create_bbox(&mut self, bbox: BoundingBox) {
        if let Some(annotation_id) = self.work.annotations.iter().find(|annotation| {
            Some(&annotation.annotation_id) == self.work.selected_annotation.as_ref()
                && self.companion_needs_box(annotation)
        }).map(|annotation| annotation.annotation_id.clone()) {
            self.edit_bbox(crate::canvas::BoundingBoxEdit { annotation_id, bounding_box: bbox });
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        let Some(class_id) = self.selected_class_id() else {
            return;
        };
        let task_id = task.task_id.clone();
        let class_id = class_id.clone();
        let user_id = self.config.user_id.clone();
        let timestamp = labello_domain::now();
        let annotation_id = AnnotationId::generate();
        self.record_edit();
        self.work
            .annotations
            .push(labello_domain::AnnotationVersion {
                annotation_id: annotation_id.clone(),
                version: 1,
                object_group_id: None,
                origin: AnnotationOrigin::native(),
                task_id,
                class_id,
                annotation_type: AnnotationType::BoundingBox,
                revision_source: RevisionSource::Human {
                    action: HumanRevisionKind::Authored,
                },
                geometry: AnnotationGeometry::BoundingBox(bbox),
                author_user_id: user_id,
                created_at: timestamp,
                updated_at: timestamp,
                deleted: false,
            });
        self.work.selected_annotation = Some(annotation_id);
        self.mark_edited();
    }

    pub(crate) fn edit_bbox(&mut self, edit: crate::canvas::BoundingBoxEdit) {
        if self.edit_prelabel_geometry(&edit.annotation_id, AnnotationGeometry::BoundingBox(edit.bounding_box)) { return; }
        let annotation_id = edit.annotation_id;
        let persisted = self.work.persisted_annotations.contains(&annotation_id);
        let persisted_version = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| state.current_annotation(&annotation_id))
            .map(|annotation| annotation.version);
        let Some(index) = self.work.annotations.iter().position(|annotation| {
            annotation.annotation_id == annotation_id && !annotation.deleted
        }) else {
            return;
        };
        let AnnotationGeometry::BoundingBox(current) = &self.work.annotations[index].geometry
        else {
            return;
        };
        if *current == edit.bounding_box && !self.companion_needs_box(&self.work.annotations[index]) {
            return;
        }
        let user_id = self.config.user_id.clone();
        self.record_edit();
        let annotation = &mut self.work.annotations[index];
        let AnnotationGeometry::BoundingBox(current) = &mut annotation.geometry else {
            return;
        };
        *current = edit.bounding_box;
        annotation.updated_at = labello_domain::now();
        if persisted {
            annotation.version = persisted_version.unwrap_or(annotation.version) + 1;
            annotation.revision_source = RevisionSource::Human {
                action: HumanRevisionKind::Edited,
            };
            annotation.author_user_id = user_id;
            self.work.modified_annotations.insert(annotation_id);
        }
        self.mark_edited();
    }

    pub(crate) fn editing_keypoint(&self) -> Option<(crate::canvas::KeypointSelection, KeypointAnnotation)> {
        let selection = self.work.canvas.selected_keypoint()?.clone();
        let geometry = if self.view == AppView::Review {
            if !self.work.canvas.is_dragging() { return None; }
            let draft = self.work.correction_draft.as_ref()?;
            if draft.annotation_id != selection.annotation_id { return None; }
            draft.edited_geometry.clone()
        } else if self.manual_migration_active() {
            if self.editable_migration_draft_annotation_id().as_ref() != Some(&selection.annotation_id)
                || self.work.migration.inspected_group_id.is_some() { return None; }
            AnnotationGeometry::Skeleton(self.work.migration.draft.clone()?)
        } else {
            if self.work.selected_annotation.as_ref() != Some(&selection.annotation_id) { return None; }
            self.annotation_objects().into_iter().find(|annotation| annotation.annotation_id == selection.annotation_id)?.geometry
        };
        let AnnotationGeometry::Skeleton(skeleton) = geometry else { return None; };
        let keypoint = skeleton.keypoints.get(selection.keypoint_index)?.clone();
        keypoint.point?;
        Some((selection, keypoint))
    }

    pub(crate) fn keypoint_visibility_editable(&self) -> bool {
        matches!(self.view, AppView::Annotate | AppView::Review)
            && (self.work.assignment.is_some() || self.runtime.api.is_none())
            && !self.saving_blocks_interaction() && !self.loading.image && !self.work.migration.busy
            && self.work.pending_transition.is_none()
            && self.work.review_corrections.submission.is_none()
            && self.selected_task().and_then(|task| task.skeleton.as_ref()).is_some_and(|spec| spec.allow_hidden)
    }

    pub(crate) fn set_editing_keypoint_visibility(&mut self, state: KeypointState) {
        if !self.keypoint_visibility_editable() { return; }
        let Some((selection, _)) = self.editing_keypoint() else { return; };
        if self.view == AppView::Review {
            self.select_correction_keypoint(selection.keypoint_index);
            self.set_correction_keypoint_state(state);
        } else if self.manual_migration_active() {
            self.set_migration_keypoint_visibility(selection.keypoint_index, state);
        } else {
            self.set_annotation_keypoint_visibility(selection.annotation_id, selection.keypoint_index, state);
        }
    }

    pub(crate) fn edit_keypoint(&mut self, edit: crate::canvas::KeypointEdit) {
        self.update_annotation_keypoint(edit.annotation_id, edit.keypoint_index, Some(edit.point), None);
    }

    pub(crate) fn set_annotation_keypoint_visibility(&mut self, annotation_id: AnnotationId, index: usize, state: KeypointState) {
        if self.view != AppView::Annotate || self.saving_blocks_interaction() || self.loading.image
            || self.work.pending_transition.is_some()
            || !matches!(state, KeypointState::Visible | KeypointState::Hidden)
            || !self.selected_task().and_then(|task| task.skeleton.as_ref()).is_some_and(|spec| spec.allow_hidden)
        { return; }
        self.update_annotation_keypoint(annotation_id, index, None, Some(state));
    }

    fn update_annotation_keypoint(&mut self, annotation_id: AnnotationId, keypoint_index: usize, point: Option<NormalizedPoint>, state: Option<KeypointState>) {
        if let Some(item) = self.work.prelabel_review.objects.iter().find(|item| item.annotation.annotation_id == annotation_id) {
            let mut geometry = item.annotation.geometry.clone();
            if let AnnotationGeometry::Skeleton(skeleton) = &mut geometry
                && let Some(keypoint) = skeleton.keypoints.get_mut(keypoint_index) {
                if point.is_none() && keypoint.point.is_none() { return; }
                if let Some(point) = point { keypoint.point = Some(point); }
                if let Some(state) = state { keypoint.state = state; }
                self.edit_prelabel_geometry(&annotation_id, geometry);
            }
            return;
        }
        let persisted = self.work.persisted_annotations.contains(&annotation_id);
        let persisted_version = self
            .work
            .current_state
            .as_ref()
            .and_then(|state| state.current_annotation(&annotation_id))
            .map(|annotation| annotation.version);
        let Some(index) = self.work.annotations.iter().position(|annotation| {
            annotation.annotation_id == annotation_id && !annotation.deleted
        }) else {
            return;
        };
        let AnnotationGeometry::Skeleton(skeleton) = &self.work.annotations[index].geometry else {
            return;
        };
        let Some(keypoint) = skeleton.keypoints.get(keypoint_index) else {
            return;
        };
        if keypoint.point.is_none() || (point.is_none_or(|point| keypoint.point == Some(point)) && state.as_ref().is_none_or(|state| &keypoint.state == state)) {
            return;
        }

        let user_id = self.config.user_id.clone();
        self.record_edit();
        let annotation = &mut self.work.annotations[index];
        let AnnotationGeometry::Skeleton(skeleton) = &mut annotation.geometry else {
            return;
        };
        let Some(keypoint) = skeleton.keypoints.get_mut(keypoint_index) else {
            return;
        };
        if let Some(point) = point { keypoint.point = Some(point); }
        if let Some(state) = state { keypoint.state = state; }
        annotation.updated_at = labello_domain::now();
        if persisted {
            annotation.version = persisted_version.unwrap_or(annotation.version) + 1;
            annotation.revision_source = RevisionSource::Human {
                action: HumanRevisionKind::Edited,
            };
            annotation.author_user_id = user_id;
            self.work.modified_annotations.insert(annotation_id);
        }
        self.mark_edited();
    }

    pub(crate) fn place_keypoint(&mut self, point: NormalizedPoint) {
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let Some(spec) = task.skeleton.clone() else {
            self.runtime.error = Some(
                "This skeleton workflow has no keypoint specification. Ask a data admin to configure it."
                    .to_string(),
            );
            return;
        };
        if let Some(active_id) = self.work.active_skeleton.clone() {
            let persisted_version = self
                .work
                .current_state
                .as_ref()
                .and_then(|state| state.current_annotation(&active_id))
                .map(|annotation| annotation.version);
            self.record_edit();
            let keypoint_index = self.work.skeleton_keypoint_index;
            let hidden = self.work.next_keypoint_hidden;
            let Some(annotation) =
                self.work.annotations.iter_mut().find(|annotation| {
                    annotation.annotation_id == active_id && !annotation.deleted
                })
            else {
                self.work.active_skeleton = None;
                return;
            };
            let AnnotationGeometry::Skeleton(skeleton) = &mut annotation.geometry else {
                return;
            };
            if let Some(keypoint) = skeleton.keypoints.get_mut(keypoint_index) {
                keypoint.state = if hidden {
                    KeypointState::Hidden
                } else {
                    KeypointState::Visible
                };
                keypoint.point = Some(point);
                annotation.updated_at = labello_domain::now();
                let completed = keypoint_index + 1 >= skeleton.keypoints.len();
                // Autosave may have committed the earlier keypoints while this
                // skeleton remained active. Continuing it is now a revision.
                if let Some(version) = persisted_version {
                    annotation.version = version + 1;
                    annotation.revision_source = RevisionSource::Human {
                        action: HumanRevisionKind::Edited,
                    };
                    annotation.author_user_id = self.config.user_id.clone();
                    self.work.modified_annotations.insert(active_id);
                }
                self.work.skeleton_keypoint_index = keypoint_index + 1;
                self.work.next_keypoint_hidden = false;
                self.work.canvas.select_keypoint(Some(crate::canvas::KeypointSelection {
                    annotation_id: annotation.annotation_id.clone(),
                    keypoint_index,
                }));
                if completed {
                    self.work.active_skeleton = None;
                    self.work.skeleton_keypoint_index = 0;
                }
                self.mark_edited();
            }
            return;
        }

        let Some(class_id) = self.selected_class_id().cloned() else {
            return;
        };
        let timestamp = labello_domain::now();
        let author_user_id = self.config.user_id.clone();
        let keypoint_count = spec.keypoints.len();
        let mut keypoints = spec
            .keypoints
            .into_iter()
            .map(|keypoint| KeypointAnnotation {
                name: keypoint.name,
                state: KeypointState::Absent,
                point: None,
            })
            .collect::<Vec<_>>();
        let Some(first) = keypoints.first_mut() else {
            self.runtime.error =
                Some("Skeleton workflows require at least one keypoint".to_string());
            return;
        };
        first.state = if self.work.next_keypoint_hidden {
            KeypointState::Hidden
        } else {
            KeypointState::Visible
        };
        first.point = Some(point);
        let annotation_id = AnnotationId::generate();
        self.record_edit();
        self.work
            .annotations
            .push(labello_domain::AnnotationVersion {
                annotation_id: annotation_id.clone(),
                version: 1,
                object_group_id: None,
                origin: AnnotationOrigin::native(),
                task_id: task.task_id,
                class_id,
                annotation_type: AnnotationType::Skeleton,
                revision_source: RevisionSource::Human {
                    action: HumanRevisionKind::Authored,
                },
                geometry: AnnotationGeometry::Skeleton(SkeletonGeometry { keypoints }),
                author_user_id,
                created_at: timestamp,
                updated_at: timestamp,
                deleted: false,
            });
        self.work.selected_annotation = Some(annotation_id.clone());
        self.work.canvas.select_keypoint(Some(crate::canvas::KeypointSelection {
            annotation_id: annotation_id.clone(),
            keypoint_index: 0,
        }));
        if keypoint_count > 1 {
            self.work.active_skeleton = Some(annotation_id);
            self.work.skeleton_keypoint_index = 1;
        }
        self.work.next_keypoint_hidden = false;
        self.mark_edited();
    }

    pub(crate) fn skip_keypoint(&mut self) {
        let Some((allow_absent, keypoint_count, required)) = self
            .selected_task()
            .and_then(|task| task.skeleton.as_ref())
            .map(|spec| {
                (
                    spec.allow_absent,
                    spec.keypoints.len(),
                    spec.keypoints
                        .get(self.work.skeleton_keypoint_index)
                        .is_some_and(|keypoint| keypoint.required),
                )
            })
        else {
            return;
        };
        if !allow_absent || self.work.active_skeleton.is_none() {
            return;
        }
        if required {
            self.runtime.error =
                Some("This keypoint is required and cannot be marked absent.".to_string());
            return;
        }
        self.record_edit();
        self.work.skeleton_keypoint_index += 1;
        self.work.next_keypoint_hidden = false;
        if self.work.skeleton_keypoint_index >= keypoint_count {
            self.work.active_skeleton = None;
            self.work.skeleton_keypoint_index = 0;
        }
        self.mark_edited();
    }

    pub(crate) fn delete_selected(&mut self) {
        if self.delete_prelabel_object() { return; }
        if let Some(selected) = self.work.selected_annotation.clone() {
            let persisted = self.work.persisted_annotations.contains(&selected);
            let persisted_version = self
                .work
                .current_state
                .as_ref()
                .and_then(|state| state.current_annotation(&selected))
                .map(|annotation| annotation.version);
            if let Some(index) = self
                .work
                .annotations
                .iter()
                .position(|annotation| annotation.annotation_id == selected)
            {
                if self.work.annotations[index].deleted {
                    self.work.selected_annotation = None;
                    return;
                }
                self.record_edit();
                let annotation = &mut self.work.annotations[index];
                annotation.deleted = true;
                annotation.updated_at = labello_domain::now();
                if persisted {
                    if let Some(version) = persisted_version {
                        annotation.version = version;
                    }
                    self.work.modified_annotations.remove(&selected);
                }
                if self.work.active_skeleton.as_ref() == Some(&selected) {
                    self.work.active_skeleton = None;
                    self.work.skeleton_keypoint_index = 0;
                    self.work.next_keypoint_hidden = false;
                }
                self.work.selected_annotation = None;
                self.mark_edited();
            } else {
                self.work.selected_annotation = None;
            }
        }
    }

    fn snapshot(&self) -> EditSnapshot {
        let approx_bytes = serde_json::to_vec(&self.work.annotations)
            .map(|value| value.len())
            .unwrap_or_else(|_| {
                self.work.annotations.len()
                    * std::mem::size_of::<labello_domain::AnnotationVersion>()
            })
            + self
                .work
                .accepted_prelabels
                .iter()
                .map(|value| value.len())
                .sum::<usize>()
            + serde_json::to_vec(&self.work.prelabel_evidence).map_or(0, |value| value.len())
            + serde_json::to_vec(&self.work.prelabel_review).map_or(0, |value| value.len())
            + 256;
        EditSnapshot {
            prelabel_review: self.work.prelabel_review.clone(),
            annotations: self.work.annotations.clone(),
            accepted_prelabels: self.work.accepted_prelabels.clone(),
            prelabel_evidence: self.work.prelabel_evidence.clone(),
            selected_annotation: self.work.selected_annotation.clone(),
            active_skeleton: self.work.active_skeleton.clone(),
            skeleton_keypoint_index: self.work.skeleton_keypoint_index,
            next_keypoint_hidden: self.work.next_keypoint_hidden,
            approx_bytes,
        }
    }

    pub(crate) fn record_edit(&mut self) {
        let snapshot = self.snapshot();
        push_history(&mut self.work.undo_stack, snapshot);
        self.work.redo_stack.clear();
    }

    fn restore_snapshot(&mut self, snapshot: EditSnapshot) {
        self.work.prelabel_review = snapshot.prelabel_review;
        self.work.annotations = snapshot.annotations;
        if let Some(state) = self.work.current_state.as_ref() {
            let persisted_annotations = state.active_annotations().cloned().collect::<Vec<_>>();
            for persisted in persisted_annotations {
                if self
                    .work
                    .annotations
                    .iter()
                    .all(|annotation| annotation.annotation_id != persisted.annotation_id)
                {
                    let mut deleted = persisted;
                    deleted.deleted = true;
                    deleted.updated_at = labello_domain::now();
                    self.work.annotations.push(deleted);
                }
            }
        }
        self.work.accepted_prelabels = snapshot.accepted_prelabels;
        self.work.prelabel_evidence = snapshot.prelabel_evidence;
        self.work.selected_annotation = snapshot.selected_annotation;
        self.work.active_skeleton = snapshot.active_skeleton;
        self.work.skeleton_keypoint_index = snapshot.skeleton_keypoint_index;
        self.work.next_keypoint_hidden = snapshot.next_keypoint_hidden;
        self.recompute_modified_annotations();
        self.mark_edited();
    }

    pub(crate) fn undo(&mut self) {
        if let Some(snapshot) = self.work.undo_stack.pop() {
            let current = self.snapshot();
            push_history(&mut self.work.redo_stack, current);
            self.restore_snapshot(snapshot);
        }
    }

    pub(crate) fn redo(&mut self) {
        if let Some(snapshot) = self.work.redo_stack.pop() {
            let current = self.snapshot();
            push_history(&mut self.work.undo_stack, current);
            self.restore_snapshot(snapshot);
        }
    }

    pub(crate) fn recompute_modified_annotations(&mut self) {
        let persisted_annotations = self.work.persisted_annotations.clone();
        let current_state = self.work.current_state.clone();
        for annotation in &mut self.work.annotations {
            if persisted_annotations.contains(&annotation.annotation_id)
                && let Some(persisted) = current_state
                    .as_ref()
                    .and_then(|state| state.current_annotation(&annotation.annotation_id))
            {
                // Saves can establish provenance after this draft or history snapshot
                // was created. Retain edits, but always use the server's identity.
                annotation.origin = persisted.origin.clone();
                annotation.object_group_id = persisted.object_group_id.clone();
                annotation.created_at = persisted.created_at;
                annotation.task_id = persisted.task_id.clone();
                annotation.annotation_type = persisted.annotation_type.clone();
            }
        }
        self.work.modified_annotations = self
            .work
            .annotations
            .iter()
            .filter(|annotation| {
                persisted_annotations.contains(&annotation.annotation_id)
                    && current_state
                        .as_ref()
                        .and_then(|state| state.current_annotation(&annotation.annotation_id))
                        != Some(annotation)
                    && !annotation.deleted
            })
            .map(|annotation| annotation.annotation_id.clone())
            .collect();
        for annotation in &mut self.work.annotations {
            if persisted_annotations.contains(&annotation.annotation_id)
                && let Some(persisted) = current_state
                    .as_ref()
                    .and_then(|state| state.current_annotation(&annotation.annotation_id))
            {
                annotation.version = if annotation.deleted {
                    persisted.version
                } else if annotation != persisted {
                    persisted.version + 1
                } else {
                    persisted.version
                };
            }
        }
    }

    pub(crate) fn mark_edited(&mut self) {
        self.work.assignment_touched = true;
        self.work.edit_generation = self.work.edit_generation.wrapping_add(1);
        self.work.save_status = SaveStatus::Dirty;
        self.work.last_edit_at = Some(Instant::now());
    }
}
