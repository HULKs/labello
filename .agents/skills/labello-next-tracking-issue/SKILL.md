---
name: labello-next-tracking-issue
description: "Reserve, analyze, draft, iterate, and only after explicit approval file the next unlocked, unchecked, and not-yet-filed Labello work item from docs/tracking/issues.md or docs/tracking/feature-requests.md in GitHub, then mark the tracking item complete. Use when the user asks for the next tracking issue, the next backlog item, or to continue moving Labello tracking work into GitHub with labels, project placement, status, and priority. Do not use for an issue supplied directly in the prompt."
---

# Labello Next Tracking Issue

Turn one repository tracking item into a clear, evidence-backed GitHub issue. Preserve the approval boundary: draft first, create only after the user explicitly approves.

## Select the work item

1. Read the repository `AGENTS.md` and obey its ownership, safety, and verification rules.
2. Read `docs/tracking/issues.md` from top to bottom, followed by `docs/tracking/feature-requests.md`.
3. Treat each unchecked checkbox and its attached indented text as one work item. Skip checked items and any item carrying a `LOCKED (<owner>, <YYYY-MM-DD>)` marker not known to belong to the current workflow. Never remove or steal another owner's lock; if its status is unclear, stop and ask the user.
4. List GitHub issues in `HULKs/labello`, including open and closed issues, before selecting an item. Compare titles and bodies semantically, not only by exact title.
5. Skip a tracking item already represented by a GitHub issue. Report the existing issue briefly if this changes which item is next.
6. Select the first unlocked, unchecked, unfiled item in document order. Record its file, line, exact wording, and surrounding details.
7. Re-read the item immediately before reserving it. If it is still eligible, add an attached indented `- LOCKED (<owner>, <YYYY-MM-DD>)` marker, using the current shared-coordination owner or agent identifier and an ISO date. Make only this narrow edit and verify it in the diff. If the item changed or acquired another lock, restart selection.

Keep the selected item locked throughout investigation, drafting, user revisions, approval, and filing. Do not check it before a GitHub issue exists. If the user explicitly abandons the workflow before filing, remove only this run's lock; otherwise retain the lock so the work can be resumed.

## Investigate

1. Check `git status` and preserve unrelated worktree changes.
2. Trace the relevant code, tests, callers, state transitions, and normative current documentation. Use code and tests as the current-behavior authority.
3. Reproduce or verify the behavior proportionally to the issue. Use deterministic UI tests, the native inspector, or the browser for visual and interaction issues when practical.
4. Separate the verified defect or missing behavior from adjacent product-policy questions. Keep the issue at the narrowest complete owner.
5. Search GitHub again using the refined terminology to catch non-obvious duplicates.

Do not implement the fix during this workflow.

## Draft

Present a complete proposed issue containing:

- title;
- existing labels and any missing labels proposed for creation;
- Labello project;
- project status, normally `Backlog`;
- priority using the organization Priority field's actual options;
- body with `Problem`, `Reproduction` or `Current behavior`, `Desired behavior`, `Acceptance criteria`, `Scope`, `Implementation notes`, and `Tracking source` sections.

For `Tracking source`, link to the source line on the repository's default branch and quote the original checkbox text. Keep implementation notes helpful but non-prescriptive.

Choose priority from current organization options. Unless the project defines different semantics, use:

- `Urgent`: immediate security, data-loss, or production-blocking risk;
- `High`: a core workflow is broken without a reasonable workaround;
- `Medium`: meaningful defect or improvement with a workaround;
- `Low`: minor polish or long-horizon work.

Use the smallest useful label set: one type label, relevant `area:` and `workflow:` labels, and only material cross-cutting labels such as `ux`, `accessibility`, `performance`, `reliability`, or `security`. Explicitly identify labels that do not yet exist.

Stop after presenting the draft. Iterate on the full text until the user explicitly approves it. Do not interpret general discussion, partial agreement, or requested edits as approval to create the issue.

## File after approval

Before creating anything, preflight all required capabilities:

1. Verify `gh` authentication and at least repository write/triage permission.
2. Resolve the organization project whose title is exactly `Labello`; do not assume its number.
3. Verify the token has project access.
4. Resolve the organization-level issue field named `Priority` with `GET /orgs/HULKs/issue-fields`, and verify the approved option exists.
5. Verify every approved label. Create a missing label only when its creation was explicitly included in the approved draft.

If a preflight fails, stop before creating the issue and give the exact corrective action.

Then:

1. Create the issue in `HULKs/labello` using exactly the approved title, body, and labels.
2. As soon as creation returns the canonical issue number and URL, mark the selected tracking checkbox `[x]` and replace this run's lock with an attached indented `- Filed as [#<number>](<canonical-url>) on <YYYY-MM-DD>.` note. Verify the focused local diff. This completes the tracking item because it has been submitted to GitHub, not because the underlying product work is finished.
3. Add the issue to the resolved Labello project.
4. Set project `Status` to the approved value, normally `Backlog`.
5. Set priority through the organization issue-field-values API, because an issue-derived Priority field is not a writable project custom field.
6. Verify the issue title, state, labels, project, project status, priority, and completed tracking entry through read-back calls.
7. Return the issue link, applied metadata, and tracking-file update.

If creation succeeds but the tracking edit or later metadata fails, repair the existing issue or tracking entry; never create a duplicate. Apart from the lock and completion edits required above, do not modify local repository files unless the user separately requests it.
