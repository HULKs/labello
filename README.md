# Issue #172 visual evidence

These two captures use the repository-owned `migration-companion-annotation`
native inspector preset and contain synthetic content only. The user explicitly
approved publishing these captures. No runtime data, request payloads, or
credentials are included.

Both captures: native MCP inspector, 1440×1000, 1 pixel per point, base
`a2c502949e465109f8180aa86651ed766658feb1` plus the issue #172 implementation
on `feat/keypoint-box-guides` before its publication commit. The PR records the
resulting exact commit. The later startup-persistence guard does not change these
pictured states.

- `native-guide-wide.png`: selected untouched migration companion appears as its
  read-only source keypoint, with instructions to draw the box.
- `native-drawn-wide.png`: dragging from that source point creates the human box
  revision for the existing companion; the source point remains visible.

No before capture was available: the baseline inspector binary was not retained.
The issue documents the former generated-box display and 12× zoom limit; the PR
records regression tests and browser checks separately.
