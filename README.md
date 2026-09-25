# Review-complete checkmark correction

Before: native-before.png, commit fa2ef1e25958696eaf3cc49b9ee8f4939f2036c1.
After: native-after.png, commit 92b3f20c634cb9e5eb120382e9ae0f3e22b2f3b4.

Both show the shared production workflow selector through the native inspector's synthetic workflow-availability preset, 1440x1000, native scale 1. The Review complete row is second from the bottom. Its checkmark now fits inside the shield with a gap from the outline, using a brighter 2-point stroke. The tooltip and selection behavior are unchanged.

Captured after the icon edit with no other source changes. The inspector build passed. At the user's request, verification was interrupted and no new hosted CI was run; this capture proves native appearance, not browser behavior or a new CI result. Earlier browser captures belong to the preceding revision.
