# Changelog

All notable changes documented here.

## [0.7.1](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.7.0...lilo-rm-core-v0.7.1) - 2026-05-24

### Features

- *(rtm-core)* expose MountSpec parser to consumers ([#50](https://github.com/littleorgans/runtime-matters/pull/50))

## [0.7.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.6.3...lilo-rm-core-v0.7.0) - 2026-05-23

### Features

- add spawn request bind mount support through `MountSpec`, `SpawnRequest.mounts`, and the `RuntimeCapability::SpawnRequestMounts` capability

### Breaking Changes

- `RuntimeCapability` adds `SpawnRequestMounts` for spawn request mounts. Downstream exhaustive matches must handle the new variant.
- `RuntimeCapability` is now `#[non_exhaustive]`. Downstream matches must include a wildcard arm.
- `SpawnRequest` adds the `mounts` field. Downstream positional construction sites must pass declared mounts or `Vec::new()`.
- session-matters tracks consumption in ALP-2798.

## [0.6.3](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.6.2...lilo-rm-core-v0.6.3) - 2026-05-21

### Miscellaneous

- *(internal)* release v0.6.2 ([#42](https://github.com/littleorgans/runtime-matters/pull/42))

## [0.6.2](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.6.1...lilo-rm-core-v0.6.2) - 2026-05-21

### Miscellaneous

- *(internal)* release v0.6.2 ([#41](https://github.com/littleorgans/runtime-matters/pull/41))

## [0.6.1](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.6.0...lilo-rm-core-v0.6.1) - 2026-05-20

### Bug Fixes

- *(tmux)* handle pane loss after repeated manual interrupts (ALP-2597) ([#37](https://github.com/littleorgans/runtime-matters/pull/37))

## [0.6.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.5.0...lilo-rm-core-v0.6.0) - 2026-05-19

### Features

- runtime-matters v0.6 client ergonomics ([#33](https://github.com/littleorgans/runtime-matters/pull/33))

## [0.5.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.4.0...lilo-rm-core-v0.5.0) - 2026-05-19

### Features

- *(rtm-cli)* [**breaking**] make session id positional for operator commands ([#30](https://github.com/littleorgans/runtime-matters/pull/30))

## [0.4.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.3.0...lilo-rm-core-v0.4.0) - 2026-05-19

### Features

- Linux runtime v0.4 support ([#27](https://github.com/littleorgans/runtime-matters/pull/27))

## [0.3.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.2.0...lilo-rm-core-v0.3.0) - 2026-05-18

### Features

- runtime contract v0.3 for session-matters integration ([#23](https://github.com/littleorgans/runtime-matters/pull/23))

## [0.2.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-core-v0.0.0...lilo-rm-core-v0.2.0) - 2026-05-18

### Bug Fixes

- forward caller env and cwd to spawned runtimes ([#11](https://github.com/littleorgans/runtime-matters/pull/11))

### Features

- add session-matters runtime contract ([#17](https://github.com/littleorgans/runtime-matters/pull/17))
- introduce strict deterministic spawn API ([#9](https://github.com/littleorgans/runtime-matters/pull/9))
- ship runtime-matters v1 (per-host kubelet + container runtime) ([#2](https://github.com/littleorgans/runtime-matters/pull/2))
