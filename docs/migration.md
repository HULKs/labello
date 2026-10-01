# Guided box-to-skeleton migration

Manual migration uses imported bounding boxes as read-only guides for a skeleton
workflow. Configure it during [import planning](import.md#mapping-and-workflow-semantics).
The imported target set is frozen; every guide needs one human-authored skeleton
or an audited whole-object exclusion, followed by a full-image confirmation.

## Resolve each guide

1. Place the workflow's ordered keypoints inside the relevant image context.
2. Use Visible for exact positions, Occluded for estimated hidden positions, and
   Not present for an optional point without coordinates.
3. Save the skeleton, or choose Exclude object with a category and any required
   note. Not present applies to one keypoint; exclusion applies to the whole object.
4. Continue directly to the next Objects item, including across images. Select
   Overview separately to inspect eligible full images for missing objects.

A newly saved, added, or edited migration skeleton needs at least one positioned
Visible or Occluded keypoint. Historical all-absent versions still replay, but
must be redrawn or excluded before another version is saved. Template-generated
seeds are derived pending work, not authoritative skeleton ground truth.

## Revisit a resolved object

Use Previous in Objects to revisit an eligible earlier guide. Saved partial
keypoints remain one object draft. Previous retains the forward path; Skip saves
the draft and releases the unfinished object. Reacquisition validates the guide,
disposition, task and current ownership before allowing edits.

In Overview, selecting a resolved canonical object records a durable revisit
dependency owned by the displayed Overview assignment. Saving or resolving that
object clears the dependency and returns to full-image confirmation without
changing queues or completing the Overview assignment. Reload restores the
selected edit while retaining the full-image view. Overview cannot finish while
the selected object still requires a decision.

Historical image assignments retain their direct-revisit dependency and correction
pass events. New item queues use their exact item lease rather than a global pass.

## Add a missing object

In the full-image view, tap blank image space to start a skeleton for an object
without an imported guide. Creation and editing need no mode buttons: existing
objects take selection priority, and clicking one opens it for editing. After the
last keypoint, the draft remains editable: drag points or use Visible/Occluded
controls when permitted by the task. For a one-keypoint schema, clicking empty
canvas again confirms the current object and starts another at the clicked
location. For schemas with multiple keypoints, explicitly save the completed
skeleton before starting another object or selecting a different one. Unchanged
previously saved objects can be left without another confirmation. Failed saves
preserve the draft; a pending one-keypoint placement resumes after successful
retry. Saving objects does not confirm or finish the image. Use Submit at the
bottom right to complete full-image confirmation, including images without guides.

Delete or Remove added object removes the whole selected added skeleton. An
unsaved addition is discarded locally and immediately returns to the overview.
A saved addition returns to the overview after the server accepts its deletion;
failed deletion keeps the editor available for retry. Successful deletion needs
no separate discard step. Undo last keypoint still removes one keypoint at a time.

The overview keeps its canvas view when selecting added objects. Refocus remains
available explicitly. The same visibility controls apply to placed points in
guided migration drafts.

A discovered skeleton creates a linked bounding box in the configured guide task
in the same event transaction. The box uses positioned Visible and Occluded
keypoints and spans at least 15% of image width and height, with a one-original-pixel
minimum on tiny images. Bounds shift inward at image edges. This sizing applies
to newly derived or regenerated boxes; existing and independently edited boxes
retain their geometry.

In Annotate, an untouched, unreviewed generated box appears as the exact source
keypoints that triggered its creation. Select the object and draw its bounding
box; the keypoints remain read-only, and saving revises the existing companion
rather than adding a second box. Multiple positioned points remain visible with
their visibility states. After drawing a box, **Next guide** saves it and focuses
the next pending source guide. The final box exposes **Submit & next**, which
completes the image. An undrawn selected guide must be drawn or deleted before
continuing; it does not cause a global submission error. Independently edited or reviewed boxes retain
their normal box presentation. If historical source geometry is unavailable, the
existing box remains available for ordinary editing.

The guide task enters Needs correction and follows ordinary annotation/review
rules. Its previous decisions remain in history. Discovery does not extend the
frozen imported target count or assign an imported object group to the new pair.
Migration review approves the skeleton; ordinary box review approves the box.

## Reconcile companions

Still-derived boxes follow skeleton edits and removal. Independent box edits or
reviews, changed versions/configuration, or competing guide-task assignments
prevent an automatic overwrite. Use the full-image workflow's per-object
reconciliation action to confirm regeneration against the exact current versions.

Historical repair requires recorded full-image discovery provenance and positioned
keypoints. Missing provenance or coordinate-less objects remain unresolved; reads
never invent a box. Each committed link records durable progress, so interrupted
repair resumes with the remaining objects. Failed saves and reconciliation retain
unsaved skeleton input.

## Review migration work

Review waits for annotation Overview. Objects contains canonical dispositions and
discovered skeletons as separate items. Each decision binds the current disposition
or annotation version and advances directly to the next Objects item. Overview is
available after all object decisions and checks missing objects in place.

Focus uses a valid companion box, then positioned-keypoint bounds, then the full
image for historical objects without positions. Corrections submit on the current
item. Added objects require Objects review; movement or edits require Overview,
with unchanged approvals preserved. Companion boxes retain their separate guide-task
review. Normal reviewer exclusions and the recorded dataset-wide fallback apply.

For transaction, retry, and compatibility details, see
[assignment](assignment.md#migration-companions),
[API commands](api.md#manual-migration-routes), and
[persistence](event-history.md#discovered-migration-companions).
