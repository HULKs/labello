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
4. Continue through unresolved or changed targets, then inspect the full image
   for missing objects and confirm it.

A newly saved, added, or edited migration skeleton needs at least one positioned
Visible or Occluded keypoint. Historical all-absent versions still replay, but
must be redrawn or excluded before another version is saved. Template-generated
seeds are derived pending work, not authoritative skeleton ground truth.

## Revisit a resolved object

From full-image confirmation, select an overview entry, completed canonical
skeleton, or excluded guide. The server records the revisit and returns the
current cursor. Saving returns to the overview if all other targets remain fresh;
otherwise changed dependencies take precedence. Failed activation or saving
preserves the workspace and draft for retry.

The UI does not start global correction passes. Existing historical passes remain
readable and resume their outstanding decisions through the normal keep, edit,
and exclude controls. Only the latest pass for an assignment gates completion;
older passes stay historical.

## Add a missing object

In the full-image view, tap blank image space to start a skeleton for an object
without an imported guide. Add missing object is also available as a button and
shortcut. Existing objects take selection priority. After the last keypoint,
the draft remains editable until explicitly saved: drag points to reposition
them or use the inspector's Visible/Occluded controls when permitted by the task.
The same visibility controls apply to placed points in guided migration drafts.

A discovered skeleton creates a linked bounding box in the configured guide task
in the same event transaction. The box uses positioned Visible and Occluded
keypoints and spans at least 15% of image width and height, with a one-original-pixel
minimum on tiny images. Bounds shift inward at image edges. This sizing applies
to newly derived or regenerated boxes; existing and independently edited boxes
retain their geometry.

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

Review defaults to canonical dispositions, then discovered skeletons in stable ID
order, then full-image confirmation. Every decision binds the current disposition
or annotation version. Unchanged objects can be approved while earlier corrections
remain local. Final approval requires this reviewer to approve every current item
in the current round.

Focus uses a valid companion box, then positioned-keypoint bounds, then the full
image for historical objects without positions. Corrections use the same
[overview submission](annotation.md#review-and-correct) as ordinary review and
start a fresh review round. Companion boxes keep their separate guide-task review.

For transaction, retry, and compatibility details, see
[assignment](assignment.md#migration-companions),
[API commands](api.md#manual-migration-routes), and
[persistence](event-history.md#discovered-migration-companions).
