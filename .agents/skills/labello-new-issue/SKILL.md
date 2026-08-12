---
name: labello-new-issue
description: "Analyze, draft, iterate, and only after explicit approval file a new Labello GitHub issue supplied directly by the user, including suitable labels, Labello project placement, status, and organization issue-field priority. Use when the user describes a bug, improvement, feature request, investigation, or documentation problem in the prompt and wants it turned into a GitHub issue."
---

# Labello New Issue

Turn a prompt-supplied concern into a clear, evidence-backed GitHub issue. Preserve the approval boundary: draft first, create only after the user explicitly approves.

## Establish the issue

1. Read the repository `AGENTS.md` and obey its ownership, safety, and verification rules.
2. Extract the reported behavior, expected outcome, affected workflow, constraints, and any explicit scope exclusions from the prompt.
3. Make safe, narrow assumptions when details can be discovered from the repository. Ask only when a missing product decision would materially change the issue.
4. Search open and closed issues in `HULKs/labello` semantically for duplicates
   or useful existing context.
5. If the issue already exists on GitHub, return its link and explain the match instead of drafting a duplicate.

## Investigate

1. Check `git status` and preserve unrelated worktree changes.
2. Trace the relevant code, tests, callers, state transitions, and normative current documentation. Use code and tests as the current-behavior authority.
3. Reproduce or verify the behavior proportionally to the issue. Use deterministic UI tests, the native inspector, or the browser for visual and interaction issues when practical.
4. Distinguish confirmed behavior from inference. Separate adjacent product-policy questions instead of silently broadening scope.
5. Keep the issue at the narrowest complete owner and search GitHub again with the refined terminology.

Do not implement the fix during this workflow.

## Draft

Present a complete proposed issue containing:

- title;
- existing labels and any missing labels proposed for creation;
- Labello project;
- project status, normally `Backlog`;
- priority using the organization Priority field's actual options;
- body with `Problem`, `Reproduction` or `Current behavior`, `Desired behavior`, `Acceptance criteria`, `Scope`, and `Implementation notes` sections.

Omit sections that genuinely do not apply, but always make the problem, desired outcome, and completion boundary unambiguous. Keep implementation notes helpful but non-prescriptive.

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
2. Add it to the resolved Labello project.
3. Set project `Status` to the approved value, normally `Backlog`.
4. Set priority through the organization issue-field-values API, because an issue-derived Priority field is not a writable project custom field.
5. Verify the issue title, state, labels, project, project status, and priority through read-back calls.
6. Return the issue link and applied metadata.

If creation succeeds but later metadata fails, repair the existing issue; never create a duplicate. Do not modify local repository files unless the user separately requests it.
