pub(crate) fn action_label(action: &labello_domain::UserAction) -> &'static str {
    use labello_domain::UserAction;
    match action {
        UserAction::NextImage => crate::glossary::CONFIRM_SUBMIT,
        UserAction::UndoEdit => crate::glossary::UNDO,
        UserAction::RedoEdit => crate::glossary::REDO,
        UserAction::SkipAssignment => crate::glossary::SKIP,
        UserAction::ToggleWorkflowPanel => crate::glossary::WORKFLOW_PANEL,
        UserAction::ToggleInspectorPanel => crate::glossary::INSPECTOR_PANEL,
        UserAction::OpenSettings => crate::glossary::SETTINGS,
        UserAction::SelectPreviousWorkflow => crate::glossary::PREVIOUS_WORKFLOW,
        UserAction::SelectNextWorkflow => crate::glossary::NEXT_WORKFLOW,
        UserAction::SelectPreviousObject => crate::glossary::PREVIOUS_OBJECT,
        UserAction::SelectNextObject => crate::glossary::NEXT_OBJECT,
        UserAction::SelectPreviousPrelabel => crate::glossary::PREVIOUS_PRELABEL,
        UserAction::SelectNextPrelabel => crate::glossary::NEXT_PRELABEL,
        UserAction::AcceptPrelabel => crate::glossary::CONFIRM_SELECTED_PRELABEL,
        UserAction::DiscardPrelabel => crate::glossary::DELETE_SELECTED_PRELABEL,
        UserAction::ToggleKeypointHidden => crate::glossary::VISIBLE_OCCLUDED,
        UserAction::MarkKeypointAbsent => crate::glossary::NOT_PRESENT,
        UserAction::AddMissingObject => crate::glossary::ADD_OR_CANCEL_MISSING_MIGRATION_OBJECT,
        UserAction::RetryImageLoad => crate::glossary::RETRY_IMAGE_LOAD,
        UserAction::TogglePanMode => crate::glossary::PAN,
        UserAction::ZoomIn => crate::glossary::ZOOM_IN,
        UserAction::ZoomOut => crate::glossary::ZOOM_OUT,
        UserAction::FitImage => crate::glossary::FIT,
        UserAction::RefocusObject => crate::glossary::REFOCUS,
        UserAction::PreviousImage => crate::glossary::PREVIOUS,
        UserAction::SaveAnnotations => crate::glossary::SAVE,
        UserAction::DeleteAnnotation => crate::glossary::DELETE,
        UserAction::SelectBoundingBoxTool => crate::glossary::BOUNDING_BOX_TOOL,
        UserAction::SelectKeypointTool => crate::glossary::KEYPOINT_TOOL,
        UserAction::AcceptReviewObject => crate::glossary::APPROVE_DIRECTLY,
        UserAction::RejectReviewObject => crate::glossary::REJECT_DIRECTLY,
        UserAction::OpenTutorial => crate::glossary::TUTORIAL,
        UserAction::ToggleOfflineMode => crate::glossary::OFFLINE_MODE,
    }
}

/// Context-dependent button labels for the same configured action. Persisted IDs stay unchanged.
pub(crate) fn action_button_names(action: labello_domain::UserAction) -> String {
    use super::*;
    use labello_domain::UserAction;
    match action {
        UserAction::NextImage => {
            return format!(
                "{ANNOTATION}: {}. {REVIEW}: {}. {MIGRATION}: {}.",
                [SUBMIT_NEXT, CONFIRM_NEXT, NEXT_OBJECT, NEXT_GUIDE].join("; "),
                [APPROVE, SUBMIT_CORRECTION].join("; "),
                [
                    SAVE_SKELETON_ADVANCE,
                    SAVE_NEXT,
                    SAVE_MISSING_OBJECT,
                    SAVE_OBJECT,
                    SAVE_OBJECT_CHANGES,
                    SAVE_CHANGES,
                    KEEP_CURRENT_ADVANCE,
                    KEEP_NEXT,
                    CONFIRM_ALL_GUIDES_FINISH,
                    CONFIRM_NO_GUIDES_FINISH,
                    CONFIRM_FINISH
                ]
                .join("; "),
            );
        }
        UserAction::OpenSettings => {
            return format!(
                "{SETTINGS}; Open {}; Open shortcut {}.",
                SETTINGS.to_lowercase(),
                SETTINGS.to_lowercase()
            );
        }
        UserAction::ToggleWorkflowPanel => {
            return format!(
                "{WORKFLOW}; Open {WORKFLOW}; Close {WORKFLOW}; Toggle {WORKFLOW_PANEL}."
            );
        }
        UserAction::ToggleInspectorPanel => {
            return format!(
                "{INSPECTOR}; Open {INSPECTOR}; Close {INSPECTOR}; Toggle {INSPECTOR_PANEL}."
            );
        }
        UserAction::MarkKeypointAbsent => {
            return format!(
                "Mark keypoint as {}; Mark <keypoint name> as {}; {NOT_PRESENT}.",
                NOT_PRESENT.to_lowercase(),
                NOT_PRESENT.to_lowercase()
            );
        }
        _ => {}
    }
    let names: &[&str] = match action {
        UserAction::UndoEdit => &[UNDO, UNDO_LAST_KEYPOINT],
        UserAction::ToggleKeypointHidden => &[VISIBLE, OCCLUDED],
        _ => &[],
    };
    names.join("; ")
}

pub(crate) fn action_category(action: labello_domain::UserAction) -> &'static str {
    use labello_domain::UserAction;
    match action {
        UserAction::NextImage
        | UserAction::UndoEdit
        | UserAction::RedoEdit
        | UserAction::SaveAnnotations
        | UserAction::SkipAssignment
        | UserAction::PreviousImage => crate::glossary::ASSIGNMENT,
        UserAction::SelectPreviousWorkflow
        | UserAction::SelectNextWorkflow
        | UserAction::SelectPreviousObject
        | UserAction::SelectNextObject
        | UserAction::DeleteAnnotation
        | UserAction::ToggleKeypointHidden
        | UserAction::MarkKeypointAbsent
        | UserAction::AddMissingObject => crate::glossary::ANNOTATION,
        UserAction::SelectPreviousPrelabel
        | UserAction::SelectNextPrelabel
        | UserAction::AcceptPrelabel
        | UserAction::DiscardPrelabel => "Prelabels",
        UserAction::TogglePanMode
        | UserAction::ZoomIn
        | UserAction::ZoomOut
        | UserAction::FitImage
        | UserAction::RefocusObject => crate::glossary::CANVAS,
        UserAction::OpenTutorial
        | UserAction::ToggleWorkflowPanel
        | UserAction::ToggleInspectorPanel
        | UserAction::OpenSettings
        | UserAction::RetryImageLoad => crate::glossary::WORKSPACE,
        UserAction::AcceptReviewObject | UserAction::RejectReviewObject => crate::glossary::REVIEW,
        UserAction::SelectBoundingBoxTool
        | UserAction::SelectKeypointTool
        | UserAction::ToggleOfflineMode => "Legacy",
    }
}

pub(crate) fn action_description(action: labello_domain::UserAction) -> &'static str {
    use labello_domain::UserAction;
    match action {
        UserAction::NextImage => {
            "The primary work button: confirm the current object or submit the image. Its label and effect depend on the workflow; see button names below."
        }
        UserAction::UndoEdit => {
            "Undo the last annotation edit. In migration, Undo last keypoint removes the last draft point. Review correction currently uses fixed Ctrl/Cmd+Z instead."
        }
        UserAction::RedoEdit => {
            "Redo the last undone annotation edit. Not available in migration or review."
        }
        UserAction::SaveAnnotations => {
            "Save annotations without leaving the image. Migration saves use Confirm / submit instead."
        }
        UserAction::SkipAssignment => {
            "Save partial annotation, release this item, and continue with another item in the workflow."
        }
        UserAction::DeleteAnnotation => {
            "Delete the selected annotation or pending prelabel. In migration, delete a missing object being added or remove the last guide keypoint. In review, only a selected new addition can be removed."
        }
        UserAction::OpenTutorial => "Show or hide workflow instructions.",
        UserAction::ToggleWorkflowPanel => "Open or close workflow navigation.",
        UserAction::ToggleInspectorPanel => "Open or close object controls.",
        UserAction::OpenSettings => "Open this keyboard shortcut editor.",
        UserAction::SelectPreviousWorkflow => "Cycle to the previous enabled workflow.",
        UserAction::SelectNextWorkflow => "Cycle to the next enabled workflow.",
        UserAction::SelectPreviousObject => {
            "Select the previous annotation or migration object. In review, use the Previous object button; this shortcut is unavailable there."
        }
        UserAction::SelectNextObject => {
            "Select the next annotation or migration object. This shortcut is unavailable in review."
        }
        UserAction::SelectPreviousPrelabel => "Highlight the previous prelabel.",
        UserAction::SelectNextPrelabel => "Highlight the next prelabel.",
        UserAction::AcceptPrelabel => {
            "Confirm the selected prelabel with its current edits, then focus the next one."
        }
        UserAction::DiscardPrelabel => {
            "Delete the selected pending prelabel. Prelabel bounding boxes stay selected until Confirm / submit confirms the deletion."
        }
        UserAction::ToggleKeypointHidden => {
            "Toggle occlusion for the keypoint being edited, or for the next placement."
        }
        UserAction::MarkKeypointAbsent => "Record an allowed optional keypoint without a position.",
        UserAction::AddMissingObject => {
            "Begin or cancel a skeleton for an object missing from the imported data."
        }
        UserAction::RetryImageLoad => "Try to claim and load an image again.",
        UserAction::TogglePanMode => "Use primary drag to move a zoomed image.",
        UserAction::ZoomIn => {
            "Increase canvas zoom with this shortcut. Wheel, touchpad scrolling and pinch also zoom."
        }
        UserAction::ZoomOut => {
            "Decrease canvas zoom with this shortcut. Wheel, touchpad scrolling and pinch also zoom."
        }
        UserAction::FitImage => "Fit and center the image.",
        UserAction::RefocusObject => {
            "Center and zoom to the active review object, migration object, or companion box."
        }
        UserAction::AcceptReviewObject => {
            "Shortcut-only direct approval of the current review target. The visible Approve / Submit correction button uses Confirm / submit and confirms the current item or submits the overview."
        }
        UserAction::RejectReviewObject => {
            "Shortcut-only rejection of the current review target through the existing correction/rejection flow. It is not the primary Submit correction button."
        }
        UserAction::PreviousImage => "Return to the previous item in this workflow's history.",
        UserAction::SelectBoundingBoxTool
        | UserAction::SelectKeypointTool
        | UserAction::ToggleOfflineMode => "No longer used.",
    }
}
