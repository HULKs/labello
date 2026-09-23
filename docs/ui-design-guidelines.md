# Labello UI and design guidelines

Use these criteria for shared UI changes. [UI implementation](ui-ownership.md)
defines state and async boundaries; [annotation and review](annotation.md) explains
the user workflow.

## Product rules

- Keep the image and current task central. The canvas stays dark, low-noise, and
  shadow-free; metadata and secondary panels yield space first.
- Build hierarchy with typography and spacing. Use teal for primary intent,
  amber for attention, and elevation only for floating content.
- Reuse semantic tokens, typography, geometry, and helpers from
  `crates/labello-ui/src/theme.rs`. Do not add local styling when an existing
  intent covers it.
- Use primary, standard secondary, quiet, and danger actions. A region normally
  has one primary action. Preserve native hover, press, focus, open, selected,
  and disabled states instead of applying direct fills.
- Embedded button shortcuts use `theme::button_shortcut` so their foreground
  follows the button's visual state. Semantic helpers and selected placement
  buttons use the shared button renderer, which scopes weak-text styling and
  disabled opacity to that button and restores the surrounding style. Disabled
  buttons retain disabled semantics and a faded appearance with enough text
  contrast; supporting text keeps the normal weak-text token. Danger button
  labels use the readable text foreground, with danger conveyed by fill/stroke.
- Use standard `egui` controls. Add a shared helper only for a repeated
  Labello-specific pattern. Add no theme, icon set, widget library, table
  dependency, or screenshot framework without a concrete unmet need.
- Create hierarchy before adding borders or cards. Avoid nested cards and
  shadows on ordinary content.
- Use sentence case and human labels. Use monospace for IDs, paths, dimensions,
  geometry, and aligned numbers.

## Layout

- Reuse `LayoutMode`: Compact below 600 points, Medium from 600 through 1287,
  and Wide from 1288. Heights below 480 points are short viewports.
- Validate width and height together. Long content and larger text must not
  collapse siblings or push primary controls offscreen.
- Keep global identity, dataset, navigation, status, and account controls in the
  app shell. The second bar owns image identity, workflow context and canvas
  controls. Dataset inspection keeps image names beneath the navigator thumbnails
  and uses the second bar only for canvas navigation and panel controls.
  The bottom bar owns workflow commands in every layout, including
  annotation, review, correction and every migration phase.
- When every global destination, action, and account element cannot fit in the
  app bar, replace all of them with one modal left-drawer trigger. The drawer
  owns its dismiss action, closes after navigation, and restores focus to its
  trigger when dismissed without navigation.
- Render each action once per layout. Keep primary work actions visible and move
  secondary actions to overflow when space is limited.
- Wide work views use workflow, canvas, and inspector panes; Medium and Compact
  use center-left Workflow and center-right Inspector drawers. Workflow actions
  stay in the bottom bar at every width.
- Use aligned rows or grids for desktop comparison. Use 44-point, touch-friendly
  cards and stacked fields on Compact layouts. Keep forms and pages bounded.
- Truncate or wrap long content deliberately; expose the complete value by
  tooltip or accessibility text.
- Dataset inspector filter menus use compact 32-point choices inside standard
  44-point triggers. Workflow-type icons, class-color markers and status symbols
  support the full accessible choice names; truncated labels retain tooltips.
  Menus use the available viewport height instead of an arbitrary fixed height,
  and scroll only when their contents cannot fit. This density exception applies
  to the inspector's workflow, class and status choices. The Images and Overlays
  status menus share these triggers and icon rows; Overlays keeps independent
  selected states for each visible status.

## State and safety

- Each remote region shows one base state: initial loading, loaded, empty,
  initial failure with Retry, loaded while refreshing, or loaded and stale after
  refresh failure.
- Keep loaded data visible during refresh. View-specific bars start blank with
  their layout space reserved. Within the same view and workflow, retain their
  last loaded contents during image navigation and disable retained controls.
  Loading feedback belongs in the image region. Never show zero placeholders
  after failure or present retained contents as newly loaded data. The global
  top bar stays visible and keeps its layout during loads.
- Put validation and failures in the affected field, section, or page. Reserve
  global notices for cross-screen events.
- Hide account-scoped content while authentication is unresolved. Clear stale
  state when endpoint or session identity changes, and ignore responses owned by
  obsolete requests, workspaces, or datasets.
- Keep drafts transactional. Failed saves leave drafts editable. Block
  navigation, dataset changes, and sign-out until staged edits are saved or
  explicitly discarded.
- Roll back loading ownership when work cannot be queued; local failure must not
  leave controls permanently busy.

## Interaction and accessibility

- Use native `egui::Modal` for blocking decisions. Only the highest-priority
  overlay is active; it blocks background input, and Escape reaches it first.
- Constrain overlays to the viewport and keep decisions reachable by scrolling
  the whole surface on short screens. Blocking drawers follow the same rules.
- Popup menus and drawers suppress workspace shortcuts. Consume captured
  keyboard events before other controls process them.
- Use a danger action plus concise confirmation for destructive work. Never use
  double-click as confirmation.
- Preserve 44-point targets, except for the inspector menu choices and compact
  saved-feedback disclosure specified here. Preserve visible focus, tooltips,
  associated field labels, and complete, contextual AccessKit names.
- Expose selected, open, disabled, loading, and modal states semantically. Never
  rely on color alone; retain text, stroke, pattern, thickness, handle, or shape
  cues.
- Match cursors to create, move, resize, pan, and disabled behavior. Keep
  gestures and shortcuts discoverable through controls or concise hints.

### Measurable accessibility criteria

- Normal text and text rendered into controls must have a contrast ratio of at
  least 4.5:1 against its background. Text at least 18 points, or at least
  14 points and bold, may use 3:1.
- Control boundaries, annotation handles, focus indicators, and meaningful
  non-text states must have at least 3:1 contrast against adjacent colors.
  Focus must remain visible on every interactive control and cannot be
  represented by color alone.
- Browser zoom from 100% through 200% must preserve access to every primary
  action and all non-canvas content. Reflow may switch `LayoutMode`; it must not
  create page-level horizontal scrolling except for a deliberately scrollable
  data region.
- Long labels and browser or OS text enlargement must wrap, truncate with an
  accessible full value, or expand their region without covering adjacent
  controls. A clipped visual label still requires a complete accessible name.
- Keyboard-only users must be able to sign in, choose a dataset and task, claim
  and release work, create/edit/delete annotations, submit or skip an
  assignment, complete review decisions, edit and save
  administration forms, operate import decisions, open settings/help, and
  dismiss or confirm every modal. Canvas-only spatial placement may require a
  pointer, but all surrounding commands and any non-spatial alternative must
  remain keyboard reachable.
- Tab order follows the visual and task order. Overlays trap focus while open,
  restore it to the invoking control when closed, and expose an accessible name
  and modal state. Escape closes only the highest-priority dismissible overlay.
- Every icon-only action has a contextual accessible name and tooltip. Dynamic
  loading, error, selection, expanded, disabled, and completion states must be
  represented in the accessibility tree, not only painted.

Labello does not currently claim certification for a specific
screen-reader/browser pair. AccessKit labels and the Chromium accessibility
tree are the supported semantic verification surfaces. A release must not claim
screen-reader support until its critical workflows have also been exercised
with named browser, operating-system, and screen-reader versions and the result
has been recorded.

## Screen patterns

- **Login and setup:** keep sign-in focused and hide methods until session
  discovery completes. Provide About without authentication and put endpoint
  editing in Advanced connection. Use a bounded sign-in column with the primary
  action before secondary navigation. Center the signed-out icon and name
  vertically in the app bar. Give the sole enabled login method primary emphasis;
  when both are enabled, keep local development secondary to GitHub. Collapse
  excess top spacing on short screens and scroll keyboard-focused actions into view.
  After sign-in, feature one valid next dataset
  action, list all datasets separately, and keep creation secondary. Use a home
  glyph for the setup navigation action while retaining its destination-based
  accessible name and tooltip. Keep the authenticated section selector visible
  in every Setup section, with About last. Each section owns its heading;
  do not repeat a dataset-specific welcome banner above unrelated sections.
  Signed-out secondary navigation also puts About last.
- **Workspace:** preserve tested canvas geometry and gestures; keep Pan and Fit
  visible, with Refocus for review and migration. Omit explicit zoom buttons and
  percentage displays; keep configurable zoom actions and wheel, touchpad, and
  pinch instructions in Settings. Review opens each focused item for direct editing. Primary drag edits the item;
  modifier-drag and middle-drag remain available for panning. Omit the Pan button
  in review; place review phase
  near the canvas; prefer compact object summaries over coordinate-heavy
  labels. Show source images without a grid overlay in annotation, review, and
  migration canvases.
- **Image overlays:** use filled circles for visible keypoints and hollow
  diamonds for occluded keypoints in saved annotations, active drafts, reviewer
  corrections, and migration. Not-present keypoints have no image marker or
  incident edge. Suggestions retain hollow markers and their prelabel color;
  visible suggestions use circles and occluded suggestions use diamonds.
  Ordinary bounding boxes use Labello annotation teal, including the active box
  outline and resize-handle outlines. Selection remains visible through the
  stronger outline, fill and white handle centers. Explicit review/migration
  warning and error styles retain their semantic colors; skeletons retain class
  colors. Use a single one-point contrasting halo on boxes, edges, keypoints,
  selection handles, and focus indicators. Choose
  black for light class colors and white for dark ones, keeping at least 4.5:1
  between the color and its halo. Inspect legibility on light, dark, and textured
  images; this does not guarantee contrast against every image pixel. Keep context
  guides, drafts, and box suggestions dashed, with gaps wide enough to separate
  the outlined dashes. Give every dashed box corner a continuous L-shaped dash
  and distribute edge gaps evenly between corner dashes. For unfocused migration context boxes, scale dash length
  and gap with viewport zoom so both shrink when zooming out. Keep stroke width
  fixed in screen space.
  Expose keypoint names, states, and marker descriptions on the canvas's
  accessibility node without encoding image coordinates.
- **Review context:** use the same exact-target projection in every layout.
  Identify workflow, class, geometry type, position, and persisted target version.
  Final checks say "Final check / Full image" and omit object-only fields; excluded
  migration targets use disposition versions. Actual edits show base version and
  unsaved input. Opening an item alone is not a correction.
  The second-bar indicator leads with position or Image overview and toggles
  Inspector, which starts closed. Size it to measured text/icon width; truncate
  identity with full accessible details, but wrap type and phase. The shell
  reserves their actual height. Same-view image loads retain the complete
  previous bar presentation, including control placement and summary dimensions.
  Identity, phase and actions update together when the next image is ready.
  First load and changes of view, workflow, dataset or account show blank reserved
  bars. Empty or failed loads discard retained presentation; preview failure
  remains separate from valid target context. Never pair old identity with a
  new phase. The global header keeps its navigation, utilities and layout during
  loads. The dataset inspector reserves its context row during initial gallery
  loading and retains fixed controls across image loads.
  On Compact, keep Inspector, Refocus, Fit, and Workflow in one top row. Keep
  Previous image, Previous object, Discard changes, and Skip visible below the
  decision and Next object/Overview controls. Added migration objects also expose
  Remove item. Icon fallback retains full names/tooltips. Short empty states scroll.
  Use Approve for unchanged items and Submit correction for valid edits. Space
  and Y/N follow the same [review flow](annotation.md#review-and-correct).
  Reset requires another decision, navigation retains valid corrections, and only
  the overview submits them. Incomplete additions block submission. Failed
  requests preserve exact retry identity. Reviewed keypoints have no extra
  selection circle.
  Revisions keep a visible Revising indication in the identity line and distinguish
  effective decisions from staged replacements without a redundant notice.
  Previous image returns to the eligible previous assignment; Previous object
  stays in the current image, retaining corrections and stopping at the first
  review target. Migration retains its audited revisit/discard rules.
  Measure secondary actions with actual fonts, icons, and shortcuts. Show the
  longest prefix that fits, trying icons before overflow. Required controls wrap
  and remain visible. Moving a focused action to overflow transfers focus to its
  trigger without dispatch; menu labels/shortcuts stack and scroll.
  Compact availability feedback uses the truncatable identity line without
  displacing type/phase or adding a row. Short viewports retain identity, phase,
  and useful canvas height; migration bars remove spare padding.
- **Saved reasons:** show only feedback matching the active image and workflow,
  sharing the persistent, dismissible area with workflow-change notices. Lead
  with the event and explanation at a four-point gap; keep the 44-point dismiss
  target beside the content so it cannot inflate that gap. Use eight-point
  grouping gaps, existing text roles and quiet borders. Keep workflow and status
  visible, then a 24-point avatar with the GitHub username immediately to its right.
  Reuse cached avatars and initials fallback; unavailable identity reads Unknown
  author. Align Additional info to the right of that same 24-point author row.
  Keep eight-point gaps above and below the row, without extra vertical button
  padding. When expanded, object IDs and UTC timestamps use the full
  content width below the row. The explanation itself never requires
  expansion. Show message counts at the bottom. Keep earlier/replaced feedback
  explicit and newest messages first. Wrap long explanations in a bounded scroll
  area; at short heights the event title opens full feedback in a bounded non-modal
  window. Use amber only for current rejection/exclusion feedback. Revisiting a
  completed review alone creates no notice. Dismissal applies to the opened
  context. Object and submission reason inputs retain their scope, validation
  and input-preservation behavior.
- **Skeleton outcomes:** present **Visible** and **Occluded** as selected
  coordinate-placement modes with one concise dynamic instruction. Present
  **Not present** as a coordinate-free outcome for one optional keypoint.
  Keep whole-object exclusion in a separate, always-visible **Exclude object**
  section. Explain the keypoint-level versus object-level distinction there,
  and reserve danger styling for the final exclusion action.
- **Admin:** use a shield icon beside Statistics in the app bar's right-side
  utility group, with a 44-point target, an `Open admin` accessible name, and a
  tooltip. Show it only when the existing Admin access check allows the view;
  keep a labeled Admin action in the collapsed drawer.
  Organize by Overview, People, Images, Schema, Automation, and
  Backups, and Export; preserve staged edits between destinations; use wide rows and compact
  cards; retain validation and role protections.
- **Statistics:** use a bar-chart icon in the app bar's right-side utility group,
  with a 44-point target, an `Open statistics` accessible name, and a tooltip.
  Keep a labeled Statistics action in the collapsed navigation drawer.
  Open a dataset-scoped modal above the current workspace or
  setup. Opening, refreshing, and closing it preserve the assignment, image,
  draft, selection, canvas transform, and workspace epoch. Continue legitimate
  in-flight workflow results and expose recovery when ownership becomes invalid.
  Block background input, provide Close and Escape dismissal, restore focus to
  the invoking control, and constrain the content to a scrollable viewport.
  Keep real data visible during refresh, order columns by the workflow, align
  numeric comparisons, and expose an accessible value for every chart item.
  Lead with the full-width score podium and rankings, then Daily activity,
  dataset totals, assignment balance, task/class breakdowns, and throughput.
  Keep Score first in metric choices and its podium expanded on mobile. Put
  other highlights below Rankings in a disclosure; stack chart cards below
  850 content points, retaining values, units, and empty states.
  Keep period controls beside activity and rankings; they share
  the selected period. Provide a keyboard- and touch-operated Activity day
  selector and Previous/Next day buttons with visible counts, alongside calendar
  hover details. At enlarged browser zoom, stack the modal header and selector
  labels. Compact rankings use a bounded sort picker, a 44-point direction
  button, and wrapping score-first summaries. Keep flame and day count beside each contributor. Keep history
  comparison controls below the metric choices and selected names in bounded rows.
  Show acceptance as a percentage with review counts.
  Show avatars beside names with initials
  fallback, and preserve keyboard access and full accessible names when truncating.

## Verification

- Read the complete interaction and async flow, reuse current patterns, and add
  the smallest `egui_kittest` regression for behavior, geometry, or AccessKit
  semantics. Include long content and failure states when relevant.
- Inspect relevant states at 320x568, 390x844, 600x800, 1288x820, 1440x1000,
  and a short size such as 320x320. Exercise each applicable size at device
  pixel ratios 1 and 2; test 390x844 at DPR 3 for a high-density mobile case.
- Repeat critical browser workflows at 200% browser zoom and with the platform's
  larger-text setting where Chromium exposes it. Record any unsupported
  platform behavior instead of silently reducing the test matrix.
- Use native inspector presets for shared rendering and accessibility checks.
  Use Chromium for WASM startup, scaling, browser input, and desktop/mobile
  rendering and inspect the browser accessibility tree. The native inspector
  does not prove browser behavior, zoom reflow, or screen-reader output.
- Run a keyboard-only pass for every changed critical workflow. For modal,
  focus, or semantic changes, record the initial focus, tab sequence, accessible
  name/state, Escape behavior, and restored focus.
- Run focused UI tests, formatting, and Clippy. Run the WASM check and
  `trunk build --release --locked` when browser or shared rendering changes.

### Build information and mismatch

Setup > About is reachable before and after sign-in. It separates Web app and
Server identities, shows release tags with twelve-character source commits,
and gives each row a complete accessible label and full-value tooltip. Missing
release metadata is explicitly development; missing commits are unavailable.
About uses a bounded content column and groups each component's release and
commit together. Web app and Server appear side by side when space permits and
stack at narrow widths. Copy build information is the primary action; server
refresh or retry is secondary. Complete selectable text is available in a
keyboard-accessible manual-copy disclosure that opens automatically when copying
fails or is unavailable. Server loading, unavailable and retry states are local
to About. Clipboard feedback is an accessible polite status and success requires
the platform copy operation to succeed.

Routine identity values stay in About. Only two complete differing release
identities produce the persistent lower-right bottom-bar warning, `Web app and
server builds differ`. Use the existing amber warning intent, a small warning
icon and quiet button interaction/focus states, without a filled alert banner.
The accessible action name also says it opens About. Keep its 44-point target
and lower-right position in wide, medium, compact and short layouts; nearby
future activity content must condense or reflow. Existing assignment transition
confirmation and cancellation apply to this navigation.

## Availability fallback feedback

Automatic workflow selection shows a persistent outlined notice with the old
and new task/class names. The new identity is emphasized, and the notice has a
44px dismiss action. Long visible names truncate within the card while tooltips
and the polite AccessKit status retain complete names. The card floats inside
the workspace without reducing its canvas layout. On short compact screens it
uses the existing identity row when that row presents the notice. Otherwise the
workspace reserves an inline notice above the canvas and reclaims its vertical
canvas inset to keep a usable image area. The notice must not overlap the canvas,
and a 320×320 review workspace retains at least 44px of canvas height. Only a
notice actually presented in the current render pass suppresses that fallback. It is
nonmodal and leaves ordinary work available after the new identity has been
presented. Dismissing
it retains the current workflow identity in the context bar and selector.

## Current workflow marker

The workflow selector reserves a fixed marker slot on every card and paints a
small bright dot only for the committed selected workflow. The same control
renders in the expanded panel and compact/medium drawer. Hover, keyboard focus,
availability and pending transition candidates do not move the dot. An
unavailable current workflow retains it. Existing selected fill/border and
AccessKit selection semantics remain; the marker adds no focus stop or name.
The automatic workflow-change notice remains independent of this visual cue.

Measured workspace action buttons use the same state-dependent shortcut
foreground both inline and in wrapped overflow menus. Moving an action into
More changes only its placement; its shortcut contrast and accessible command
identity remain the same.

Responsive workspace checks must also resize through the viewport matrix while
running only requested frames. Compact action panels must settle without a later
pointer or keyboard event; forced extra frames can conceal a cached-height gap.

## Workspace presence

Presence belongs in the existing top header, with compact circular GitHub profile
photos and a small semantic-color connection dot. Right-align the avatars and
dot immediately before the navigation action icons. The signed-in account has no
separate username label; qualifying active leases include it in presence.
Navigation icons run right to left: Logout, Home/Setup, authorized Admin,
shortcut settings, and statistics. Keep the dataset pill centered when space
permits; on narrow screens it yields to presence. Preserve one header row,
primary action targets, canvas space and keyboard focus through resizing.

Avatars are 28 points inside a single focusable target at least 44 points high
and wide. Show as many as fit, then `+N` for the remaining users. Missing, pending
or failed photos use initials. The shared statistics/presence loader caches
public GitHub photos and failures; it sends no browser credentials. Full
`@githubLogin` handles and active datasets remain available on hover, activation
and through the control's accessible name. Missing logins use the internal user
ID in those details. Counts include the current user under the same active-lease
rules. Avatars and initials stay static; the former text highlight is removed.

A successful empty sample reads "No active labellers". Keep the existing loading,
temporary failure and unavailable states. Do not add a presence bar, daily-count
footer, extra row or lease-duration explanation. Reserve a 44-point focusable
target for the connection dot while painting only a 9-point circle. Color is
accompanied by textual status in its accessible name and details.

Check one and many users, overflow, long handles and dataset names, missing and
failed photos, initial loading, connection loss/recovery, saving/unsaved work,
application errors, and keyboard activation.
