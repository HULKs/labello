# Review shield icon corrections

Before: native-before.png, commit fa2ef1e25958696eaf3cc49b9ee8f4939f2036c1.
After: native-after.png, commit b03134a093133e77d9abe60aa1c26c35b39db7d0.

Native inspector, synthetic workflow-availability preset, 1440x1000, scale 1. Review disabled is the second row, Review complete is second from the bottom. Both now use bright inset symbols with 2-point strokes: an x for disabled and a check for complete. The symbols have space from the shield outline.

The inspector build passed and the resulting rendering was inspected. Local verification is skipped at the user's request. Hosted CI remains enabled on the final commit; its outcome is recorded in the PR. Earlier browser captures belong to the preceding revision. No browser behavior claims are made for this visual correction.
