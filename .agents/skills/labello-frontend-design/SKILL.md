---
name: labello-frontend-design
description: Design, implement, or critique Labello's Rust/egui UI with deliberate visual hierarchy and interaction design. Use for new screens, layout changes, and visual polish; not for backend work or UI logic fixes with no design component.
license: Apache-2.0; see LICENSE.txt
---

# Labello frontend design

Adapted from Anthropic's [frontend-design skill](https://github.com/anthropics/skills/blob/41bbe19d1a1a7eaab5e7bb9050a417e5c6cffc8f/skills/frontend-design/SKILL.md).
Modified for Labello's Rust/egui implementation, established visual identity,
and verification workflow. The upstream license is retained in
[LICENSE.txt](LICENSE.txt).

Make the annotation task easier to see, understand, and complete. Give the
requested screen a deliberate composition grounded in its real content. Keep
the user's requested scope and visual direction; a design critique can end in
findings, while an implementation request should reach a working UI.

## Ground the design

Read [UI design guidelines](../../../docs/ui-design-guidelines.md) and
[UI ownership](../../../docs/ui-ownership.md) before proposing changes. They
own the current product rules and acceptance criteria. Inspect the affected
screen, its callers, and the relevant command/response flow to distinguish
supported actions from presets or unfinished capabilities.

Identify the user, current decision, primary action, and content that needs the
most space. Use the request and existing screen to resolve ordinary choices;
ask only when missing information materially changes scope or correctness.
Use representative Labello content, including long labels and realistic object
counts, when judging a composition.

For a new screen or substantial redesign, outline the composition before
coding. Name the focal content, action hierarchy, typography roles, existing
semantic tokens, and how the layout changes when width or height runs out.
A small wireframe helps when comparing layouts. For a narrow polish request,
keep this to the affected region. Review the proposal against the brief and
current product rules, then proceed within the authorized scope.

## Make deliberate visual choices

- Give the screen one clear focal area. In annotation and review, the image and
  current decision earn that space. Supporting metadata should help the task
  without competing with the canvas.
- Build hierarchy with grouping, alignment, spacing, and type before adding
  containers. A border should explain a boundary; a card should group a real
  unit; numbering should represent an actual sequence. Remove decoration that
  contributes no information.
- Work from [theme.rs](../../../crates/labello-ui/src/theme.rs). Select tokens
  by meaning and reuse its typography and action helpers. Improve composition
  within the established identity. A palette or font redesign belongs only in
  a brief that calls for it, with shared tokens and affected screens updated
  together.
- Make density serve the task. Align values for comparison and give forms a
  readable measure. On small screens, reorganize secondary content while
  keeping the current decision and primary controls reachable. Test short
  viewports as well as narrow ones.
- Spend visual emphasis where it helps users act. Keep canvas decoration quiet
  enough to preserve image detail and annotation distinctions. Use existing
  semantic cues consistently across the canvas and its inspector.
- Use motion only to explain a state change or acknowledge input. Keep any new
  motion brief and nonessential; account for reduced-motion needs before
  introducing it. Avoid continuous repainting for decorative effects.
- Write labels around the user's action and use the same vocabulary throughout
  the flow. Empty and failure states should explain the situation and offer an
  available next step. Keep implementation details out of product copy unless
  they help the user decide what to do.

## Implement in Rust and egui

Shared UI belongs in `labello-ui`; follow the feature and rendering owners in
the ownership document. Keep browser bootstrap in `labello-wasm` thin. Translate
design intent into egui layout and widget composition rather than introducing
an HTML/CSS/React layer.

Reuse `LayoutMode` and existing panel, grid, wrapping, scrolling, drawer, and
modal patterns. Lay out in egui logical points using available space; account
for text measurement and clipping instead of scaling a fixed desktop mockup.
Keep widget IDs stable across frames and repeated items so focus, editing, and
open state survive redraws and responsive changes.

Prefer standard widgets and theme helpers so hover, pressed, selected,
disabled, and keyboard focus states stay intact. Scope local style changes to
their region. Use custom painting for canvas geometry or necessary visuals,
with corresponding interaction and AccessKit semantics for interactive parts.
A painted label or hit rectangle alone does not make an accessible control.

Rendering reads feature state and emits actions through the existing command
path. Preserve loading ownership, draft edits, and stale-response handling
when reorganizing controls. Plan the affected loading, empty, failure,
refreshing, stale, and disabled presentations along with the loaded screen.

## Inspect and critique

For implementation work, read the
[inspector guide](../../../apps/egui-mcp-inspector/README.md) before launching
inspection. Use its headless recipe when needed. Compare before and after
screenshots of the affected states, and inspect the accessibility tree.
Check whether the primary action is obvious, labels remain legible, alignment
supports scanning, and the canvas retains useful space. Revise specific
problems found in that pass rather than adding another layer of decoration.

Use the applicable viewport, DPR, zoom, larger-text, keyboard, contrast, and
focus matrix in the UI guidelines. Follow
[verification](../../../docs/verification.md) for focused `egui_kittest`
regressions and canonical changed-path checks. Native inspection proves shared
egui rendering and semantics; Chromium is required for actual browser claims.
Keep evidence tied to the tested revision and report missing checks explicitly.

For a critique-only request, report the observed design problems and proposed
changes with their user impact. For implementation, report what changed, the
inspection and test evidence, and remaining limitations. Use the authorized
Labello publication workflow when the user also requests a PR.
