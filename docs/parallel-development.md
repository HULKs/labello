# Parallel issue development

> **Status:** Contributor workflow guide
> **Owner:** Labello maintainers
> **Audience:** Implementers, orchestrating agents, and reviewers

Use this guide when the user authorizes implementation of several issues,
delegation into tracks, or combined testing of dependent PRs. Planning a batch
does not authorize starting it. Preserve any pause for issue-body review until
the user resumes implementation.

## Plan groups and tracks

A group is a set of issues tested together before a separate user testing pass.
A track is an ordered chain of dependent issues within that group. Independent
tracks can run concurrently. Separate groups receive separate test passes.

Read each current issue body and material comments, then trace the affected
owners, shared files, data contracts, and test fixtures. Record whether each
dependency is explicit in the issue or inferred from code. Shared files alone
do not prove a dependency, but conflicting edits or coupled persistence changes
may justify sequencing work.

For each group, record:

- included issues, acceptance criteria, and dependencies outside the group;
- sequential order within each track and which tracks can run concurrently;
- one implementer and worktree per track;
- one branch and PR per issue, with its intended PR base;
- a common integration base and the combined workflows to test;
- applicable manual checks, independent review, and the user testing handoff.

If a group needs another group's unmerged work, record that dependency and its
tested revision. Reconsider the split if the groups cannot be tested separately.
Keep the batch's changing issue list and revision record with its plan or
handoff, rather than adding issue-specific assignments to this guide.

## Allocate worktrees and agents

Inspect `git worktree list --porcelain` before allocating paths. Use
`.worktrees/<group>-<track>/` under the main checkout for each new track and
`.worktrees/<group>-integration/` for the combined build. Preserve existing
worktrees and unrelated changes. Respect an explicitly assigned worktree.

Delegate independent tracks when parallel implementation is authorized and
agent tools are available. Give each agent its issue requirements, branch/base,
owned worktree, dependencies, verification obligations, and requested endpoint.
Use one active writer per worktree. An independent reviewer needs a separate
checkout pinned to the revision under review. Without agent tools, process the
same tracks serially and retain the branch and testing boundaries.

Each sequential track reuses its worktree by switching issue branches after
committing the preceding issue. Branches keep the repository's
`<type>/<description>` convention. For example, issue B depends on issue A:

| Issue | Branch | PR base |
| --- | --- | --- |
| A | `fix/first-workflow-step` | `main` |
| B | `fix/dependent-workflow-step` | `fix/first-workflow-step` |

Create B from A's recorded head. Each PR's diff must contain its own issue's
change relative to its declared base. Publishing a stack does not merge it.
Retarget dependent PRs when prerequisites merge, preserving only the intended
issue diff and rerunning checks for changed heads or bases. Do not rewrite a
published branch without authorization for that rewrite.

Allocate separate runtime data, server ports, browser profiles, MCP server
processes, and evidence directories for concurrent drivers. Follow the
[inspector guide](../apps/egui-mcp-inspector/README.md#parallel-agents)
for headless execution and connection isolation. Use the assigned checkout's
build, and limit concurrent builds to the machine's available CPU and memory.

## Verify each PR and the combined group

Each issue retains its focused regression evidence and canonical verification
against its recorded PR base. The required hosted `Testing` check must pass on
each PR's current head. Combined testing supplements those checks.

Once all tracks are ready for group testing:

1. Record the common base SHA and every issue branch/head SHA. Require clean
   source worktrees so the combination contains exactly those revisions.
2. In the integration worktree, create a fresh local integration branch from
   the common base and merge each track tip in dependency order. A track tip
   already includes its sequential prerequisites. Record the resulting SHA.
   This local test merge is not permission to merge a PR or push to `main`.
3. Resolve conflicts with the owning implementers. Put behavior fixes and
   substantive conflict resolutions on the appropriate issue branches, then
   rebuild the combination. Do not leave a required fix only on the temporary
   integration branch.
4. Run `./scripts/verify.sh changed <common-base-sha>` from the integration
   worktree. Apply every selected risk profile in
   [verification](verification.md), including relevant browser, visual,
   persistence, authorization, and recovery checks.
5. Exercise every issue's acceptance criteria on the combined application,
   including interactions between parallel tracks. Use fresh disposable data
   and the [UI acceptance matrix](ui-design-guidelines.md) where applicable.
6. Record commands, outcomes, artifacts, runtime configuration without secrets,
   omissions, and the exact integrated revision. Follow
   [operations](operations.md) for redaction and permitted evidence.

Repeat successful checks only when a changed head/base, failure, or unresolved
concern invalidates their evidence. A change to any included track invalidates
the old combined result. Rebuild the group and rerun its canonical verification
and affected combined workflows. Update the issue-to-evidence record so the
user can identify exactly which combination is ready to test.

## Fix findings and hand off

Collect findings against the tested group revision. Assign each finding to its
owning issue or identify a scope change that needs user input. Fix it on that
issue's branch. Propagate prerequisite changes through downstream branches in
order, preserving each PR's focused diff. Reverify changed PR heads and rebuild
the group; an isolated passing fix does not establish that the group passes.

For the user testing pass, provide one group handoff with all issue/PR links,
track order, exact tested revisions, reproducible startup and test steps,
evidence, and known limitations. Keep different groups' results separate.
Report incomplete CI or independent review explicitly using the
[workflow scope and acceptance gates](verification.md#workflow-scope).

Independent review must examine the original requirements and final issue
diffs as well as the combined evidence. User testing, group tests, per-PR CI,
and independent acceptance each prove different parts of completion. Follow
the authorized lifecycle endpoint; passing group tests alone does not authorize
review transitions, merging, issue closure, or acceptance.

Stop only processes owned by the task. Retain the revision record and evidence
needed to reproduce findings. Remove temporary integration resources only when
they are no longer needed, preserving issue branches and user work.
