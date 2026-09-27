# Annotation and review

Choose a dataset and workflow, then open Annotate or Review. Work is assigned
per image and task; the server checks your role and exact assignment on every
mutation. Inspect lets you browse images without claiming work.

## Annotate

Draw bounding boxes or place the workflow's ordered keypoints. Select an existing
object to edit it, and drag a placed keypoint to correct its position. New objects
remain selected and editable after placement. Drawing a manual box preserves the
current zoom and pan, including after reviewing prelabels. During prelabel review,
Refocus explicitly zooms to the selected object. The inspector offers Visible and
Occluded controls for positioned keypoints when the workflow allows occlusion.
After releasing a placed or dragged keypoint, its editing visibility control and
occlusion shortcut update that point without moving it. Clicking another placed
point selects it for these controls. While a skeleton is incomplete, the separate
placement controls still configure the next point.
Submit & next stays at the bottom right, including compact layouts. Skeleton
keypoints have three outcomes:

| Outcome | Meaning | Canvas marker |
| --- | --- | --- |
| Visible | Exact positioned keypoint | Filled circle |
| Occluded | Estimated position for a hidden keypoint | Hollow diamond |
| Not present | Optional keypoint without coordinates | No marker or incident edge |

Autosave records edits, and Undo/Redo can restore earlier work as a new revision.
Submission completes workflows without review; approval workflows enter a review
round. Skipping releases work without approving it. Guided box-to-skeleton work
has additional [migration](migration.md) steps.

The workflow selector identifies the current workflow and available work. Reason
icons explain disabled cards through short tooltips and accessible descriptions.
They distinguish completion balance, disabled review, an empty dataset, finished
annotation or review, work awaiting submission, competing claims, review revisions,
and excluded imports. Mixed restrictions use a generic unavailable icon.
Submission, image loading, and a pending transition temporarily disable all cards
and take precedence in that order. Background annotation saves keep workflow cards
and keypoint controls stable and usable; the save indicator reports their progress.
Switching workflows during autosave opens the usual confirmation, and submission
or release waits until the current save finishes. Checking or failed availability alone leaves
cards selectable; a known unavailable result remains visible during refresh.
The current workflow keeps its white dot beside any reason icon. If
availability causes an automatic switch, a persistent notice names the old and
new workflow. The [focus bonus](scoring.md#focus-workflow) marks the current
bonus workflow without changing assignment order.

## Canvas and shortcuts

Open Settings with `Ctrl+,` on Windows/Linux or `Cmd+,` on macOS. Search actions,
record bindings, resolve contextual conflicts, and choose **Save changes**.
Settings are staged until saved and can be restored to defaults.

Bindings accept keyboard keys, right-click, and extra mouse buttons 4 and 5,
with Shift, Alt, and the primary Ctrl/Cmd modifier. To assign Delete annotation
to right-click, record that action, press the right mouse button, and save.
Mouse bindings activate once on press over the annotation canvas, using the
same selected target and workflow guards as keyboard bindings. They do not
activate over settings, toolbars, drawers, or other overlays, or from pen/touch
input. Left-click and middle-button drag remain reserved for editing and panning;
wheel and multi-click gestures are not configurable bindings. Existing keyboard
defaults are unchanged.

Submission and completion actions require a fresh press. Holding a shortcut or
the activation key of a focused button acts on at most one item, even if saving
finishes or another object or image loads while the key is held. Release and
press again to continue. This includes prelabel decisions, companion processing,
review approvals/corrections, and migration save-and-advance/final confirmation.
A press while an action is unavailable does not queue a later submission.

| Action | Default interaction |
| --- | --- |
| Zoom | Mouse wheel, two-finger touchpad scrolling, pinch, or configured zoom keys |
| Fit | Fit control or double-click |
| Pan in annotation | `P` toggles Pan mode, then primary-button drag; `Escape` exits it |
| Pan while editing | `Ctrl` plus primary-button drag, or middle-button drag; the modifier is configurable |
| Refocus active object | `R`, or Refocus in review, migration, prelabel review, and selected migration companions |
| Confirm / submit | `Space`; in review this acts on the focused item or final overview |
| Previous image | Left arrow; subject to the previous-assignment eligibility rules |
| Delete annotation | `Delete`; also discards a selected locally added review object |
| Pen annotation | Primary-tip drag creates/moves/resizes boxes; tap places keypoints and drag moves them. See the [stylus contract](stylus-input.md) for tested event streams and pending device coverage. |

The browser regression uses the disposable server and Python dependencies from
[the browser input procedure](stylus-input.md#automated-browser-procedure).
After building the server and release WASM distribution, run
`/tmp/labello-stylus-env/bin/python apps/labello-wasm/tests/mouse_bindings.py` from
the repository root. It checks persisted Shift+right-click deletion, modifier
matching, canvas scope, and suppression of the browser context menu.

Canvas zoom ranges from fit-to-view to 48 times that scale, including restored
workspace views. Zoom magnifies the working preview; it does not fetch additional
image detail. Box creation requires a drag of at least three canvas points in
each dimension, independent of zoom, so small objects remain drawable at deep
zoom; clicks and tiny pointer jitter do not create boxes.

Migration-created box work uses [source keypoints as read-only guides](migration.md#add-a-missing-object)
until the box is drawn. Select guides on the canvas, in Inspector, or with the
configured previous/next object shortcuts. Drawing updates the selected companion.

Review opens objects for direct editing. Use modifier-drag or middle-drag to pan.
Refocus uses current correction geometry. Workflow actions remain in the bottom
bar; on smaller screens, Workflow and Inspector open as drawers. Shortcuts are
suppressed while text fields, menus, or blocking overlays own input.

Working images use Data Saver WebP previews, quality 80 with a maximum edge of
1280 pixels. Coordinates always refer to original image dimensions. Retry image
load repeats that profile; the UI has no quality selector or original-detail
fallback. Gallery thumbnails use a separate 256-pixel profile.

## Review and correct

1. Inspect each current object. **Approve** accepts an unchanged item.
2. Correct erroneous geometry, remove an incorrect object, or change a migration
   disposition. **Submit correction** retains that correction locally and moves
   to the next item. Existing Y/N bindings remain available.
3. Use Previous object, Next object, or Overview to revisit decisions. Reset item
   restores the original and requires another decision. Discard corrections
   clears all staged changes.
4. At the image overview, add missing boxes or skeletons. Finishing one skeleton
   retains it so you can add another. Delete removes the selected local addition.
5. Once every original item has a decision and all additions are valid, submit
   the final full-image decision. Without corrections, approval completes the
   task. With corrections, submission saves the complete batch and starts a
   fresh review round.

Only the overview sends corrections to the server. Saving corrections never
completes the task. One reviewer must approve every current object and the full
image in the new round; the same reviewer may claim it. Previous approvals do
not carry over. A rejection requires a substantive change: unchanged geometry,
a comment alone, or a missing-object location marker cannot reject work.

Optional correction reasons can describe an object or the full submission.
Combined reasons, including object labels and separators, must fit 2000 UTF-8
bytes. Migration exclusion requires a category and a note for Other. Failed
requests retain the exact submission for retry.

## Previous work and navigation

Previous image can reopen the immediately previous eligible skipped or completed
review in the same dataset and task. It is not a general history browser.
Completed review opens a revision; the old outcome stays effective until the
replacement is committed. Later work, changed configuration or targets, or a
competing lease can make reopening unavailable. See the precise
[previous-review rules](assignment.md#previous-review-and-decision-revisions).

Untouched work can be released directly when leaving. Changed work prompts for a
decision, and cancelling keeps the draft. A failed release or previous-review
opening leaves the current workspace available. Statistics opens above the
workspace and preserves the assignment, draft, selection, and canvas view.

## Drafts, feedback, and connection state

Browser draft recovery is best effort and scoped to the server, account, dataset,
and assignment. Server event history remains authoritative. An expired session
prompts sign-in recovery; returning as a different account cannot apply the old
draft. Changing the API endpoint clears account and dataset state. Locally staged
review-revision decisions do not survive reload.

Saved feedback shows only reasons matching the active image and workflow, newest
first. The event, explanation, workflow, and current or historical status remain
visible. The author row shows a GitHub username and avatar when available, or
Unknown author; **Additional info** reveals object identity and UTC time. Earlier
and superseded feedback stays labeled as history. Dismissal lasts for the opened
context; reloading or reopening can show the feedback again. Historical
missing-object locations are read-only; create the missing annotation to correct
current work. Inspector return-to-review reasons are recorded but are not yet
shown in these notices.

Annotation and review headers show users with active leases, including their
active dataset names. These names are visible across dataset-role boundaries to signed-in users.
Presence does not grant dataset access or renew work. Every authenticated view
polls presence and shows the connection dot. Its text details explain green for
a successful connection, yellow for checking, temporary failure, saving or unsaved
work, and red for sustained connection or save errors.
Closing the browser does not immediately release its leases.

See [current limitations](limitations.md) before relying on offline work,
prelabels, stylus support, or browser recovery.

## Model suggestions

See [Model prelabels](prelabels.md) for selecting a model or no prelabels,
loading and fallback states, overlap suppression, and confirming prelabels.
Each object opens selected and zoomed in for editing. **Confirm & next** keeps it
and advances; **Delete** or the Delete key removes it. The last object returns to
the full image for **Submit & next**. Pending objects are never accepted by autosave.
Hints load independently of the image. Model failures leave manual annotation
available; confirmed objects follow the same submission and review workflow.

## Shortcut settings and button names

Settings groups actions by Assignment, Annotation, Prelabels, Canvas, Workspace,
and Review. Search accepts button names, descriptions, categories and key names,
including modifiers and `conflict`. Expand **Button names and contexts** for
context-dependent controls. A conflict names the other action and the overlapping
workspace context; **Show conflicting shortcuts** filters the list to those rows.

Click the current binding to record a replacement; Escape cancels recording.
Reset restores one default, while Restore all defaults stages a complete reset.
Save changes publishes the draft and closes settings after a successful save.
The top-right close button, Cancel, or Escape with unsaved changes opens a discard
decision; a failed save retains the draft and keeps settings open. Closing is
disabled while saving. Pan drag records a modifier
for left-drag; middle-drag remains available independently.

Names describe the UI; persisted action identifiers and default keys are unchanged.
The following inventory maps every active action to its controls. The button column
also serves as the reverse lookup from a control to its setting. Visibility and
eligibility still depend on the selected task, assignment and loading state.

| Setting (persisted action) | Buttons or other controls and current context |
| --- | --- |
| Confirm / submit (`next_image`) | Annotation: Submit & next; Confirm & next for pending prelabels; Next object to focus pending work; Next guide for companion boxes. Review: Approve or Submit correction. Migration: Save skeleton & advance / Save & next; Save missing object / Save object; Save object changes / Save changes; Keep current & advance / Keep & next; Confirm all guides & finish or Confirm no guides & finish / Confirm & finish. Object actions and final image submission retain their different semantics. |
| Previous image (`previous_image`) | Previous image in annotation, review and migration; returns to the preceding eligible assignment. |
| Undo (`undo_edit`) | Undo in annotation; Undo last keypoint in migration. Review correction uses fixed Ctrl/Cmd+Z, not this binding. |
| Redo (`redo_edit`) | Redo in annotation. No migration or review redo. |
| Save (`save_annotations`) | Save, inline or in More, in annotation. Migration object saves use Confirm / submit. |
| Skip (`skip_assignment`) | Skip in work views; release and claim another assignment. |
| Delete (`delete_annotation`) | Delete in annotation, including pending prelabels. Migration deletes a missing object being added, or removes the last guide keypoint. In review this shortcut only removes a selected new addition; Reset item is a different action. |
| Previous workflow (`select_previous_workflow`) | Shortcut-only cycling; the Workflow panel also permits direct workflow selection. Annotation only. |
| Next workflow (`select_next_workflow`) | Shortcut-only cycling of enabled workflows in annotation. |
| Previous object (`select_previous_object`) | Previous object in annotation and migration; migration also revisits earlier guides. Review has a Previous object button, but its keyboard context currently excludes this binding. |
| Next object (`select_next_object`) | Next object in migration and shortcut-only annotation selection. The prelabel primary Next object uses Confirm / submit. Review's Next object button is not dispatched by this binding. |
| Visible / Occluded (`toggle_keypoint_hidden`) | Visible and Occluded controls for the editing target or next placement; only where the task allows occlusion. |
| Not present (`mark_keypoint_absent`) | Mark a named keypoint as not present in annotation; Not present in migration. Requires an eligible optional point. |
| Add or cancel missing migration object (`add_missing_object`) | Shortcut-only start/cancel in migration's full-image phase. Blank-canvas creation and Discard object changes are related contextual interactions, not universal aliases. |
| Previous prelabel (`select_previous_prelabel`) | Shortcut-only previous pending prelabel in ordinary annotation. |
| Next prelabel (`select_next_prelabel`) | Shortcut-only next pending prelabel in ordinary annotation. |
| Confirm selected prelabel (`accept_prelabel`) | Shortcut-only confirmation of the selected pending prelabel. The primary Confirm & next uses Confirm / submit; both can reach prelabel confirmation under its eligibility guards. |
| Delete selected prelabel (`discard_prelabel`) | Shortcut-only deletion of a pending prelabel; deleted model boxes still need Confirm / submit. Delete also reaches that operation for a pending selection, but is the broader Delete binding. |
| Pan (`toggle_pan_mode`) | Pan, with a selected state, in eligible work views. Review enforces pan mode. |
| Zoom in (`zoom_in`) | Shortcut-only zoom; wheel, touchpad and pinch remain separate gestures. |
| Zoom out (`zoom_out`) | Shortcut-only zoom; no explicit zoom buttons. |
| Fit (`fit_image`) | Fit; double-clicking the canvas also fits. |
| Refocus (`refocus_object`) | Refocus for the active review/migration object or companion box. |
| Tutorial (`open_tutorial`) | Tutorial / Open tutorial; toggles workflow instructions. |
| Workflow panel (`toggle_workflow_panel`) | Workflow panel/drawer open and close controls. |
| Inspector panel (`toggle_inspector_panel`) | Inspector panel/drawer open and close controls. |
| Settings (`open_settings`) | Settings / Open settings. The configured shortcut is handled in work views; the global button is also available from Setup. |
| Retry image load (`retry_image_load`) | Retry assignment loading when annotation has no current image. Other Retry buttons are separate actions. |
| Approve directly (`accept_review_object`) | Shortcut-only direct review approval; distinct from the primary Approve button's Confirm / submit binding. |
| Reject directly (`reject_review_object`) | Shortcut-only review rejection through the existing correction/rejection flow. Not an alias for Submit correction. |
| Pan drag (modifier, not an action ID) | Hold the configured modifier and left-drag the canvas. Middle-drag is fixed. |

Buttons without their own configurable action include settings Search, Reset,
Restore all defaults, Save changes, Cancel and discard confirmation; work More,
Overview, Reset item, Back to overview, Discard changes/corrections, migration
exclusion and reason controls, and direct workflow/object selection. Global
Setup/Home, Admin, Statistics and Sign out, and administration/import/export
forms likewise have no configurable action in this catalog. Tab, Enter and
Escape retain their contextual widget/modal behavior. Legacy tool-selection and
offline action IDs are excluded from settings.

This inventory describes current behavior, including existing review/migration
shortcut differences; it does not promise uniform undo or deletion semantics.

### Review summary and submitter

The review context summary shows the workflow, review position and submitter's
avatar. It is informational. Use the separate Inspector toggle beside Workflow
to open full review details. Narrow layouts put the summary above those controls.
Hover information and accessible text identify the submitter; unavailable photos
use initials, and missing submission identity uses an unknown-author fallback.
The submitter is the author of the current submission or migration confirmation,
not the current reviewer or an annotation's latest editor.

## Correction feedback inbox

The feedback button in the top application bar opens a bounded inbox anchored
at the button. It lists corrections to your work across accessible datasets.
View opens a read-only overlay with the image, before/after annotations,
reviewer identity and any explanation. Select a changed object to focus it.
Viewing an item acknowledges it after its image and comparison have rendered;
optional items can also be dismissed directly. Dismissal persists across sessions.

Each workflow has a mandatory-feedback threshold, initially five pending
correction submissions. Data administrators configure it in Admin > Schema >
Mandatory feedback. When any workflow reaches its threshold, all further
labeling is blocked across datasets. In annotation/review the feedback overlay
opens automatically. It cannot close until every required item is viewed;
falling below five does not release the block. Next feedback advances through
required items, including other workflows that reached their threshold.
The underlying assignment and draft remain intact. Failed loads or
acknowledgements offer retry and do not acknowledge unseen feedback.

Feedback starts with corrections committed after the feature's first
activation on the server. Existing historical corrections are not backfilled.
Removed images/workflows and revoked dataset access do not trap users behind
inaccessible feedback. Correction history itself is never deleted by dismissal.
