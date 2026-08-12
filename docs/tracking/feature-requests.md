# Feature requests

> **Status:** Tracking backlog; not a current behavior contract
> **Owner:** Product maintainers
> **Audience:** Product owners and contributors

This file tracks product capabilities that are absent end to end. Partial
implementations, defects, verification gaps, and disagreements with
[`labello.md`](../../labello.md) belong in [`issues.md`](issues.md).

## Target-Product Capabilities

- [ ] Browser offline annotation mode.
  - Filed as [#45](https://github.com/HULKs/labello/issues/45) on 2026-08-12.
  - Download a bounded assigned workspace containing images, tasks, tutorials,
    permissions, current state, and event fragments.
  - Author and retain local mutations without network access.
  - Synchronize through the existing offline API and present conflicts for
    correction or adjudication.
- [ ] Independent multi-annotator labeling and adjudication.
  - Filed as [#46](https://github.com/HULKs/labello/issues/46) on 2026-08-12.
  - Keep annotators' submissions isolated until the required independent
    labels have been collected.
  - Calculate configured IoU or keypoint-distance agreement, automatically
    accept agreement, and route disagreement to an authorized adjudicator.
  - Enable the complete Adjudicate UI and assignment workflow.
- [ ] Production prelabel model execution.
  - Filed as [#47](https://github.com/HULKs/labello/issues/47) on 2026-08-12.
  - Execute configured server-side models.
  - Execute compatible browser-local models through WebGPU with CPU/WASM
    fallback.
  - Apply configured confidence and overlap processing and retain exact model
    provenance when a suggestion is accepted.
- [ ] Supported native desktop/offline client.
  - Filed as [#48](https://github.com/HULKs/labello/issues/48) on 2026-08-12.
  - Reuse the shared Rust UI and domain behavior where practical.
  - Define its authentication, filesystem, synchronization, packaging, update,
    and support contracts separately from the development inspector.

## Other Known Unsupported Capabilities

- [ ] Native snapshot restore with validation of every required omitted or
  externally supplied artifact.
  - Filed as [#49](https://github.com/HULKs/labello/issues/49) on 2026-08-12.
- [ ] Import into or merge with an existing dataset without weakening
  no-replace publication and audit-history guarantees.
  - Filed as [#50](https://github.com/HULKs/labello/issues/50) on 2026-08-12.
- [ ] Prediction or prelabel import as non-authoritative suggestions with model
  provenance.
  - Filed as [#51](https://github.com/HULKs/labello/issues/51) on 2026-08-12.
- [ ] Segmentation annotation and import.
  - Filed as [#52](https://github.com/HULKs/labello/issues/52) on 2026-08-12.
- [ ] Remote and archive import sources with bounded, authenticated,
  traversal-safe acquisition.
  - Filed as [#53](https://github.com/HULKs/labello/issues/53) on 2026-08-12.
- [ ] Round-trip dataset export with an explicit supported-format and
  provenance contract.
  - Filed as [#54](https://github.com/HULKs/labello/issues/54) on 2026-08-12.
- [ ] Multi-process coordination for one datasets root.
  - Filed as [#55](https://github.com/HULKs/labello/issues/55) on 2026-08-12.
- [ ] Add a low-resolution image mode to preserve mobile data.
  - Filed as [#56](https://github.com/HULKs/labello/issues/56) on 2026-08-12.
