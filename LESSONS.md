# Lessons

- Do not keep `async-trait` in this repo unless a trait needs dynamic dispatch. Prefer native trait futures for static dispatch, using explicit `impl Future + Send` return types for public traits.
- For any failed gate, inspect the failing test and its fixture path before classifying it as a flake. Isolation reruns are evidence, not root-cause analysis.
- In code review, do not downgrade a semantic gap between spec and implementation to "non-blocking" just because the test suite passes. Tests not covering a failure mode is not evidence the failure mode is acceptable. Shutdown ordering and swallowed error results are exactly what tests miss until production — flag them as potential blockers.
- Before suggesting a rollback or error-path change in a transaction, verify what has actually been written into the transaction at the point of failure. Suggesting a rollback on an empty transaction introduces a defect, not a fix.
