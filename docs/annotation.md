# Annotation and review

Choose a dataset and workflow, then open Annotate or Review. Work is assigned
per image and task; the server checks your role and exact assignment on every
mutation. Inspect lets you browse images without claiming work.

## Annotate

Draw bounding boxes or place the workflow's ordered keypoints. Select an existing
object to edit it, and drag a placed keypoint to correct its position. New objects
remain selected and editable after placement. The inspector offers Visible and
Occluded controls for positioned keypoints when the workflow allows occlusion.
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
Saving, image loading, and a pending transition temporarily disable all cards
and take precedence in that order. Checking or failed availability alone leaves
cards selectable; a known unavailable result remains visible during refresh.
The current workflow keeps its white dot beside any reason icon. If
availability causes an automatic switch, a persistent notice names the old and
new workflow. The [focus bonus](scoring.md#focus-workflow) marks the current
bonus workflow without changing assignment order.

## Canvas and shortcuts

Open Settings with `Ctrl+,` on Windows/Linux or `Cmd+,` on macOS. Search actions,
record bindings, resolve contextual conflicts, and choose **Save changes**.
Settings are staged until saved and can be restored to defaults.

| Action | Default interaction |
| --- | --- |
| Zoom | Mouse wheel, two-finger touchpad scrolling, pinch, or configured zoom keys |
| Fit | Fit control or double-click |
| Pan in annotation | `P` toggles Pan mode, then primary-button drag; `Escape` exits it |
| Pan while editing | `Ctrl` plus primary-button drag, or middle-button drag; the modifier is configurable |
| Refocus active object | `R`, or Refocus in review, migration, and selected migration companions |
| Submit and next | `Space`; in review this acts on the focused item or final overview |
| Previous image | Left arrow; subject to the previous-assignment eligibility rules |
| Delete annotation | `Delete`; also discards a selected locally added review object |
| Pen annotation | Primary-tip drag creates/moves/resizes boxes; tap places keypoints and drag moves them. See the [stylus contract](stylus-input.md) for tested event streams and pending device coverage. |

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
