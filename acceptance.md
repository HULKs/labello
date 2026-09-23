# Issue 155 acceptance record

Base: 535775927541e904b2a3a6991006d6c1898ae846, origin/main.
Worktree: /home/alexschmander/labello/.worktrees/stable-loading-bars.
Branch: fix/stable-loading-bars. Original issue status Ready, verified In progress before edits.

| Contract | Owner / runtime | Evidence and boundaries |
| --- | --- | --- |
| All bar-bearing views | Shared app shell, app bar, workspace and inspection panels; WASM and native inspector call the same LabelloApp renderer | Global header hides unresolved first-load contents; Inspect reserves its existing fixed context geometry until gallery resolves. Setup/Admin/Stats share the header without image-dependent workflow bars. Annotation, review and all migration variants covered by the preset matrix. |
| First load blank, stable space | loading_bars, review measurement, shell minimum sizes | first_load_and_changed_scope_have_blank_bars; blank_bars_reserve_loaded_geometry_including_frame_margins. Native first-load screenshot and delayed Chromium claim. |
| Same-view next/previous retains contents | view/epoch/task-scoped bar presentation; migration display descriptions | image_loading_retains_bar_controls_and_geometry_across_workflows, 14 variants x 6 sizes. Production clear_current_image removes all work but retained bar controls keep exact labels and rectangles. Existing delayed previous-review tests exercise live async command scheduling and success/failure. |
| No loading-only reflow | common renderers and corrected border/margin reservation | Exact before/loading and blank/ready panel rectangle assertions; larger 36-point text and long labels; native and Chromium captures. |
| No stale actions | disabled bar UI plus central shortcut guard, original request ownership | AccessKit controls disabled; attempted Next/Previous/Save do not enqueue commands. Browser Space/Skip/Previous and pointer activation checked while responses held. |
| Coherent completion | shell sync after accepted messages and navigation | Same-view cache contains display data only. Live assignment, image state and commands never restored from cache. Existing delayed previous success and failure tests retained. |
| Invalidation and obsolete responses | auth/workspace epoch plus view/dataset/task identity; existing live ownership | Scope-change matrix exercises all five dimensions. Existing delayed_obsolete_load_does_not_release_the_current_assignment and stale-response suites in full UI run. |
| Empty/failure/retry | cache discarded when loading ends without current work; existing canvas states | empty_and_failed_loads_drop_retained_bars_and_retry_starts_blank; delayed_previous_review_failure_preserves_current_canvas. |
| Native/browser/accessibility | actual shared inspector and locked release WASM | Native first/next review and migration at wide/mobile/short; Chromium actual API delays, responsive/DPR/zoom, keyboard attempts and browser AX inspection. Browser canvas does not expose the full egui AccessKit tree; deterministic/native tests cover widget labels and disabled state. |
| Documentation | ui-design-guidelines, ui-ownership, inspector README | Updated loading policy, presentation ownership, deterministic loading presets; Last verified markers unchanged. |

Fail-before evidence: /tmp/labello-155/fail-before.log. All five new tests failed against the original production renderer from the recorded base, with only the new tests and loading-preset setup retained. Original source was temporarily substituted only for task-owned edited files, backed up and restored byte-for-byte. No unrelated changes existed. Baseline native screenshot uses that original renderer and synthetic review-next-image setup. No production fix lives in presets.
