# Lessons

- Do not keep `async-trait` in this repo unless a trait needs dynamic dispatch. Prefer native trait futures for static dispatch, using explicit `impl Future + Send` return types for public traits.
- For any failed gate, inspect the failing test and its fixture path before classifying it as a flake. Isolation reruns are evidence, not root-cause analysis.
