# Annotation and review

Choose a dataset and workflow, then open Annotate or Review. Work is assigned
per image and task; the server checks your role and exact assignment on every
mutation. Inspect lets you browse images without claiming work.

## Annotate

Choose a class and activity in Workflow. The class heading and type icons are
centered; configured activities share the card width. A workflow with existing
objects has Objects and Overview buttons. Workflows without focusable sources
show one normal annotation action. Every disabled choice has a reason icon.
The boosted workflow has an amber rim and glow, with a short shimmer
when motion is enabled. Its tooltip and accessible description identify the boost.

Objects displays one box or whole skeleton, zoomed for focused work. Confirm saves
and scores that item, then opens the next eligible Objects item, including across
images. It does not enter Overview after the last object. Migration uses the same
item boundary, with the current guide and skeleton keypoints.

Overview displays the whole image and handles missing boxes or skeletons directly.
It is available only when all prerequisite object work is complete. Both applicable
passes are required. Complete Overview to submit the image for review or finish a
workflow that has review disabled.

Drag to draw a box or place the skeleton's ordered keypoints. Box corners and edges
resize it; dragging inside moves it. Use Visible or Occluded for positioned points
when permitted, or Not present for an optional point without coordinates. A whole
skeleton counts as one item regardless of its number of keypoints.

Autosave preserves unfinished geometry. Skip saves partial work and releases it
for another worker. Confirm remains at the bottom right, including compact layouts.
The assignment and [migration](migration.md) guides describe exact completion rules.

Switching workflows saves unfinished work and releases its reservations before
loading the destination. Availability failures retain a visible reason; an unknown
result remains selectable for retry. Automatic balance switches show a dismissible
notice without blocking the new item. A configured Overview backlog limit may
switch Objects to Overview so waiting images can finish.

## Canvas and shortcuts

Shortcut settings are personal and apply to every dataset on the same server.
They are available from Setup before opening a dataset. When older saved
shortcuts exist, Settings offers a dataset source to copy into the global draft.
Choose a source and save, or save the current settings. The original copies
remain preserved; changing datasets never replaces saved global shortcuts.

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
| Previous | Left arrow; returns to the previous eligible item in this queue |
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

Review starts only after annotation Overview is finished. In Objects, Approve
confirms the current object; edit it and Submit correction to publish its correction.
Both advance directly to another Objects item. Reset restores the original editor.
In Overview, inspect the whole image and add or correct missing annotations in place.
Approve finishes an unchanged image; Submit correction publishes its changes.
Incomplete additions block confirmation but can be saved and skipped.

Added objects return to Objects review and then Overview. Movement or geometry
edits require another Overview; unchanged object approvals remain valid.
Normal Objects review excludes the final author. Overview excludes everyone who
worked on that image. If no independent work exists anywhere in the user's dataset
review queues, the server may grant a recorded final exception. See
[review eligibility](assignment.md#review-eligibility-and-corrections).

Correction reasons are optional and total at most 2000 UTF-8 bytes. Migration
exclusions require a category and a note for Other. Failed requests preserve the
exact submission for retry.

## Previous work and navigation

Previous follows the current workflow's queue. Objects moves by object; Overview
moves by image. The default history depth is five and administrators can change it.
Going C → B → A keeps B and C reserved; advancing returns through B and C before
requesting new work. Reopening rechecks current ownership, versions and eligibility.
A failed opening keeps the current workspace available.

Skip saves partial work, releases the current item and prefers another available
item. Leaving the workflow releases current, prepared and history reservations,
while keeping saved work and visit history. Statistics opens above the workspace
and preserves its assignment, editor and canvas. See
[reservation rules](assignment.md#previous-skip-and-partial-work).

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

[Model prelabels](prelabels.md) are configured by the dataset administrator.
Users receive prepared Objects items without selecting or generating a model.
Confirm & next accepts the current geometry; Delete marks a prediction for explicit
confirmation. Saving or skipping a partial prediction does not accept it.
The next item stays in Objects. Overview is selected separately for missing objects.

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
| Previous (`previous_image`) | Previous item in annotation, review and migration, within the configured queue history. |
| Undo (`undo_edit`) | Undo in annotation; Undo last keypoint in migration. Review correction uses fixed Ctrl/Cmd+Z, not this binding. |
| Redo (`redo_edit`) | Redo in annotation. No migration or review redo. |
| Save (`save_annotations`) | Save, inline or in More, in annotation. Migration object saves use Confirm / submit. |
| Skip (`skip_assignment`) | Skip in work views; release and claim another assignment. |
| Delete (`delete_annotation`) | Delete in annotation, including pending prelabels. Migration deletes a missing object being added, or removes the last guide keypoint. In review this shortcut only removes a selected new addition; Reset item is a different action. |
| Previous workflow (`select_previous_workflow`) | Shortcut-only cycling; the Workflow panel also permits direct workflow selection. Annotation only. |
| Next workflow (`select_next_workflow`) | Shortcut-only cycling of enabled workflows in annotation. |
| Previous object (`select_previous_object`) | Historical alias for Previous in item queues; hidden from new shortcut configuration. |
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
