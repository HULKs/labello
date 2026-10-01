# Product glossary

This is the vocabulary for application-provided UI text. Names supplied by users,
such as dataset names, class names, keypoint names, and workflow names, remain
user content. API fields, event names, persisted identifiers, and file-format
terms keep their compatibility spellings.

## Using the glossary

Use the canonical name for the same action or concept in every view, including
buttons, menus, headings, dialogs, status messages, shortcut search, tooltips,
and accessible names. Context may qualify an action, for example "Retry import"
or "Open settings". Sentence case, grammatical plurals, and lowercase words in
sentences do not create new concepts. Explanations must preserve the glossary's
meaning. Icon-only controls keep the complete contextual accessible name.

Compact forms are allowed only where listed below. A binding that dispatches
different operations in different phases lists the concrete glossary names in
shortcut help and search. A shared shortcut does not make its operations equivalent.

The shared UI catalog in `crates/labello-ui/src/glossary.rs` owns these names and
definitions. Renderers reference its constants; shortcut metadata lives beside
it. Tests require this table to match the catalog, require active shortcut names
to have entries, and reject copied catalog labels and retired terms in production
UI source. Add or update the entry before introducing a new product concept or
action. Contextual explanatory prose stays with the rendering owner.

Compact item progress may display `3 / 12` for `Item 3 / 12` or prelabel
`Object 3 of 12`. Full phase and progress wording remains accessible.
The context widget uses the shared task-type icon at every width. Its tooltip
and accessible description retain `Bounding boxes` or `Skeletons`.

## Names and meanings

<!-- glossary:start -->
| Name | Meaning |
| --- | --- |
| Objects | A workflow pass for focused annotation or review of one object at a time. |
| Overview | A workflow pass for checking the complete image and handling missing objects after its Objects work is finished. |
| Previous | Return to the previous item in this workflow's saved history. |
| Boosted workflow | A workflow whose annotations currently earn a scoring bonus. |
| Dataset | Images, workflow configuration, annotations, and their history managed together. |
| Workflow | A configured labeling process for one annotation type and class. Stored as a task in the API and dataset configuration. |
| Assignment | Work claimed by one person for an image and workflow. |
| Annotation | A saved or draft bounding box or skeleton describing an object. |
| Object | An entity in an image, which may have related bounding-box and skeleton annotations or a pending prelabel. |
| Prelabel | A model-generated suggestion awaiting human confirmation; it is not yet a saved annotation. |
| Guide | Read-only source geometry used to place or check an annotation during migration or companion annotation. |
| Class | The configured category of an object. |
| Bounding box | A rectangular annotation around an object. |
| Skeleton | An annotation containing named keypoints and their visibility states. |
| Keypoint | A named location in a skeleton. |
| Visible | A keypoint positioned at a directly visible location. |
| Occluded | A keypoint with a position inferred despite visual obstruction. |
| Not present | An allowed optional keypoint recorded without a position. |
| Review | Checking submitted annotations and recording approval, rejection, or corrections. |
| Correction | An edit made to address an annotation problem. |
| Migration | Guided creation of skeleton annotations from existing bounding-box guides. |
| Bounding box annotation | Create or edit bounding boxes for the selected class. |
| Bounding box review | Review submitted bounding boxes for the selected class. |
| Skeleton review | Review submitted skeletons for the selected class, including migrated skeletons. |
| Skeleton annotation | Create or edit skeletons directly for the selected class. |
| Add missing objects | Begin a skeleton for an object without a bounding-box guide during migration full-image confirmation. |
| Boxes | Compact class activity label for bounding box annotation. |
| Migrate | Compact class activity label for migration. |
| Missing | Compact class activity label for adding missing objects. |
| Full image | The complete image, including objects outside the currently focused guide. |
| Pending | A workflow on an image that has not started. |
| In progress | A workflow on an image with work underway. |
| Awaiting review | Submitted work waiting for a reviewer. |
| Needs correction | Work returned for annotation changes. |
| Completed | A workflow on an image that has met its completion requirements. |
| Excluded | An item deliberately omitted with a recorded reason. |
| Approved | A positive review decision. A review outcome is distinct from workflow completion state. |
| Rejected | A negative review decision requiring changes. |
| Draft | Local or staged changes that have not been committed by the relevant save or submit action. |
| Saved | Changes successfully committed by a save operation. |
| Saving | A save operation is pending. |
| Unsaved changes | Edits that have not yet been saved. |
| Canvas | The image area used for viewing and editing geometry. |
| Workspace | The image, workflow controls, canvas, and associated panels. |
| Inspector | The panel containing details and controls for the current object. |
| Inspect | Open read-only dataset image inspection. |
| Images | The dataset image collection or its navigation panel. |
| Overlays | Annotation geometry displayed over an image during inspection. |
| Annotate | Open the annotation workspace to create or edit annotations. |
| Admin | Dataset configuration and management controls. |
| Setup | Dataset selection, creation, and application setup. |
| Settings | Personal application preferences and shortcut configuration. |
| Statistics | Workflow progress and contributor activity. |
| About | Application and server build information. |
| Tutorial | Instructions for the selected workflow. |
| Schema | Reusable class, workflow, and skeleton definitions. |
| Snapshots | Captured dataset metadata and history; image bytes and authentication state are not included and native restore is unavailable. |
| Snapshot | One captured set of dataset metadata and history. |
| Import | Create a new dataset from a supported ground-truth source. |
| Export | Capture and deliver supported detection or pose data from a dataset. |
| Ingestion | Discover image files in configured roots and update the image index. |
| Preflight | Validate a proposed import or export before starting publication or capture. |
| Model | A configured inference model used to generate prelabels. |
| Roles | Permissions assigned to people within a dataset. |
| Annotator | A person permitted to claim and annotate work. |
| Reviewer | A person permitted to review submitted annotations. |
| Data admin | A person permitted to manage dataset configuration and data. |
| People | Dataset members and role management. |
| Leaderboard | Contributor ranking for the selected activity period. |
| Streak | Consecutive qualifying days of annotation activity. |
| Score | Contribution points computed by the scoring policy. |
| Split | A dataset partition such as train, validation, or test. |
| Source | The input selected for import or configuration reuse. |
| Status | The current state of an item or operation. |
| Save | Commit current edits without submitting the image or advancing to another assignment. |
| Submit | Complete and submit the current image workflow when its requirements are satisfied. |
| Submit & next | Finish the current item and advance within this workflow's queue. |
| Confirm & next | Confirm the current prelabel geometry or deletion, then advance to the next object. |
| Next object | Move to the next object in the current image; the workflow determines whether confirmation is required first. |
| Previous object | Historical object-navigation label; item queues use Previous across image boundaries. |
| Next guide | Save the current companion annotation and advance within Objects; Overview is completed separately. |
| Previous image | Historical image-navigation label; item queues use configurable Previous history. |
| Next image | Navigate to the next image in dataset inspection. |
| Approve | Confirm an acceptable review item or submit final image approval, according to the review phase. |
| Submit correction | Confirm the current correction or submit accumulated corrections, according to the review phase. |
| Approve directly | Shortcut-only direct approval of the current review target. |
| Reject directly | Shortcut-only rejection through the review correction and rejection flow. |
| Reject | Record a negative review decision. |
| Confirm / submit | The context-dependent primary-work shortcut; each concrete operation retains its own glossary entry. |
| Save skeleton & advance | Save the skeleton for the current migration guide and advance. |
| Save & next | Compact form of Save skeleton & advance. |
| Save missing object | Save a skeleton for a newly discovered object during migration. |
| Save object | Compact form of Save missing object. |
| Save object changes | Save edits to a previously added migration object. |
| Save changes | Commit staged edits; also the compact form of Save object changes in migration. |
| Keep current & advance | Keep the current migration disposition and advance to the next guide. |
| Keep & next | Compact form of Keep current & advance. |
| Confirm all guides & finish | Confirm the migration full-image scan after all guides are resolved. |
| Confirm no guides & finish | Confirm that the image has no migration guides and needs no skeletons. |
| Confirm & finish | Compact form of the applicable migration full-image confirmation. |
| Skip | Save partial work, release this item, and request another item in this workflow. |
| Undo | Reverse the last supported annotation edit. |
| Undo last keypoint | Remove the last draft keypoint during migration. |
| Redo | Reapply the last undone annotation edit where supported. |
| Delete | Remove the selected annotation, pending prelabel, or editable migration addition according to context. |
| Discard changes | Abandon staged edits without saving them. |
| Cancel | Cancel the current interaction or pending operation where cancellation is supported. |
| Close | Dismiss the current view or overlay. |
| Retry | Repeat a failed operation while preserving its applicable retry identity. |
| Refresh | Reload the current region from the server. |
| Reset | Restore the relevant setting or item to its defined baseline. |
| Pan | Move the visible image region without editing annotations. |
| Fit | Fit and center the complete image in the canvas. |
| Refocus | Center and zoom the canvas to the active object or guide. |
| Zoom in | Increase canvas magnification. |
| Zoom out | Decrease canvas magnification. |
| Return to review | Reopen eligible completed workflows for a new review. |
| Sign in | Authenticate to access Labello. |
| Sign out | End the current authenticated session. |
| Search | Filter the displayed entries by the entered query. |
| Name | A human-readable name, including names supplied by dataset administrators. |
| Workflow ID | The stable identifier of a workflow; serialized as a task identifier. |
| Per workflow | Progress grouped by configured workflow. |
| Workflow panel | The panel for selecting the active workflow. |
| Inspector panel | The panel for object details and controls. |
| Previous workflow | Select the previous enabled workflow. |
| Next workflow | Select the next enabled workflow. |
| Previous prelabel | Select the previous model-generated suggestion. |
| Next prelabel | Select the next model-generated suggestion. |
| Confirm selected prelabel | Confirm the selected prelabel with its edits and advance. |
| Delete selected prelabel | Delete the selected pending prelabel; pending bounding-box deletion still requires confirmation. |
| Visible / Occluded | Toggle the keypoint visibility state for editing or placement. |
| Add or cancel missing migration object | Begin or cancel a skeleton for an object absent from imported data. |
| Retry image load | Try again to claim and load an image. |
| Bounding-box tool | Legacy annotation tool binding retained for compatibility, not an active configurable action. |
| Keypoint tool | Legacy annotation tool binding retained for compatibility, not an active configurable action. |
| Offline mode | Legacy binding; browser offline annotation is not an operational workflow. |
| Retained prelabels | Model predictions cached for reuse, bound to the image, workflow, model, and reset generation. |
| Dataset prelabels | Dataset-wide generation and management of retained prelabels. |
| Refresh prelabels | Reload or regenerate the current eligible prelabels. |
| Remove prelabels and pause | Remove retained prelabels in the selected scope and pause generation; preserve annotations and drafts. |
| Resume prelabels in this scope | Allow prelabel generation again within the selected administrative scope. |
| Reset item | Reset the editable review item according to its current correction state. |
| Create | Create the specified dataset, class, workflow, or configuration item. |
| Download | Transfer the selected available artifact to the browser. |
| Reload | Fetch current server state again, with the applicable draft-discard guard. |
| Reopen | Make a resolved item editable again where the workflow permits it. |
| Remove | Remove the specified configuration or retained artifact after its applicable confirmation. |
| Confirm | Commit the stated decision; its object and effect must be explicit in context. |
| Enabled | Available for use under the item's configuration and access rules. |
| Disabled | Not available for use under the item's configuration or current state. |
| Automation | Administration of inference models and dataset prelabel generation. |
| Exclusion | A recorded reason for omitting an item from annotation or migration coverage. |
| Confidence | The model score for a predicted object. |
| Threshold | The cutoff applied to the named model score or overlap measure. |
| Image | One source image in a dataset, identified independently of its filename. |
| Keybindings | Saved mappings from configurable actions to input shortcuts. |
<!-- glossary:end -->

## Replaced terminology

| Previous UI wording | Canonical wording | Boundary |
| --- | --- | --- |
| Task, task name, task ID, Per Task | Workflow, workflow name, Workflow ID, Per workflow | Serialized task identifiers and API fields retain their names. |
| Model object, suggestion, hint | Prelabel | A pending model prediction is distinct from an annotation and from an object in the image. |
| Backups | Snapshots | Snapshot capture does not imply image backup or a restore operation. |
| Stats | Statistics | Navigation and the modal use the same destination name. |
| Submitted as a workflow status | Awaiting review | Review outcomes remain separate. |
| Whole image | Full image | The phase scans the entire image, beyond the focused object. |

## UI coverage

| UI owner | Vocabulary covered |
| --- | --- |
| Setup and app bar | Dataset, schema, account actions, navigation destinations |
| Workflow panel, canvas, Inspector | Workflow, assignment, object, annotation, geometry, visibility, navigation |
| Annotation and prelabels | Save, Submit & next, Confirm & next, prelabel, guide, Undo, Delete |
| Review and correction | Review, Approve, Submit correction, full-image phase, review outcomes |
| Every migration phase | Guide, skeleton, missing object, exclusion, save/keep/confirm actions and compact forms |
| Dataset inspection | Images, Overlays, workflow/status filters, Return to review |
| Administration | Workflow configuration and IDs, classes, models, roles, ingestion, snapshots |
| Import and export | Workflow/class mappings, source, profile, preflight, split, validation and job messages |
| Statistics | Per workflow, completion states, annotations, contributor activity |
| Settings and help | Canonical action names, contextual button names, shortcut search and explanations |

An annotation describes an object. A prelabel proposes an annotation. A guide
provides read-only geometry for creating or checking another annotation. Those
terms are intentionally distinct. "Workflow" describes configured labeling;
"assignment" describes claimed work on one image. Review outcomes and workflow
completion states also remain distinct.
