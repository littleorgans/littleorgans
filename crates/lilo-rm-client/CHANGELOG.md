# Changelog

All notable changes documented here.

## [0.7.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.6.3...lilo-rm-client-v0.7.0) - 2026-05-23

### Features

- move the client contract to the mount-capable `lilo-rm-core` release

### Breaking Changes

- `RuntimeCapability` adds `SpawnRequestMounts` for spawn request mounts. Downstream exhaustive matches must handle the new variant.
- `RuntimeCapability` is now `#[non_exhaustive]`. Downstream matches must include a wildcard arm.
- `SpawnRequest` adds the `mounts` field. Downstream positional construction sites must pass declared mounts or `Vec::new()`.
- session-matters tracks consumption in ALP-2798.

## [0.6.2](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.6.1...lilo-rm-client-v0.6.2) - 2026-05-21

### Miscellaneous

- *(internal)* release v0.6.1 ([#40](https://github.com/littleorgans/runtime-matters/pull/40))

## [0.6.1](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.6.0...lilo-rm-client-v0.6.1) - 2026-05-20

### Miscellaneous

- *(internal)* release v0.6.1 ([#39](https://github.com/littleorgans/runtime-matters/pull/39))

## [0.6.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.5.0...lilo-rm-client-v0.6.0) - 2026-05-19

### Features

- runtime-matters v0.6 client ergonomics ([#33](https://github.com/littleorgans/runtime-matters/pull/33))

## [0.5.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.4.0...lilo-rm-client-v0.5.0) - 2026-05-19

### Features

- *(rtm-cli)* [**breaking**] make session id positional for operator commands ([#30](https://github.com/littleorgans/runtime-matters/pull/30))

## [0.4.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.3.0...lilo-rm-client-v0.4.0) - 2026-05-19

### Features

- Linux runtime v0.4 support ([#27](https://github.com/littleorgans/runtime-matters/pull/27))

## [0.3.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.2.0...lilo-rm-client-v0.3.0) - 2026-05-19

### Miscellaneous

- *(internal)* release v0.3.0 ([#25](https://github.com/littleorgans/runtime-matters/pull/25))

## [0.2.0](https://github.com/littleorgans/runtime-matters/compare/lilo-rm-client-v0.0.0...lilo-rm-client-v0.2.0) - 2026-05-18

### Features

- add session-matters runtime contract ([#17](https://github.com/littleorgans/runtime-matters/pull/17))
