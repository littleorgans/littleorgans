# Imported Repositories

Phase 1 records the pre-migration source repositories without importing their source history.

| Substrate | Remote | Branch | Frozen SHA | Tags at frozen SHA |
| --- | --- | --- | --- | --- |
| identity | `git@github.com:littleorgans/identity-matters.git` | `main` | `e01affa2a6400f3194e1ae236aee04019c1dd3e6` | `lilo-im-core-v0.1.1`, `lilo-im-store-v0.1.1`, `lilo-im-stub-v0.1.1` |
| runtime | `git@github.com:littleorgans/runtime-matters.git` | `main` | `dad5f09c058ef2269de86b7925540b7a3d11bf9c` | `lilo-rm-client-v0.7.1`, `lilo-rm-core-v0.7.1` (at parent commit `1be2beccad2a8509f74764707a8f5b0aa7bd7d41`) |
| session | `git@github.com:littleorgans/session-matters.git` | `main` | `3a2af7ed65fffbf9080d0c5f770c8ae9edb79716` | `v0.2.8` |

Runtime architecture source artifacts at the frozen SHA included `runtime-matters/PROJECT.md` and the untracked `runtime-matters/MAP.md`. Phase 3/W4 merged them into `docs/architecture/runtime.md`.
