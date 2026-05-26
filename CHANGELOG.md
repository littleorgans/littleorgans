# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Features

- Add experimental Docker isolation diagnostics, documentation, and Claude image contract example.

### Notes

- Docker isolation is experimental. Host execution remains the default, Docker is selected per spawn, and Docker absence does not block host use.
- Docker support covers headless execution and host tmux attach behavior. Multiplexers inside the container, Kubernetes, SandboxClaim, injected sidecars, reconnecting PTY, credential volume management, first class firewall UX, privileged execution, and aggressive capability hardening remain out of scope.
- `rtm doctor --format json` no longer reports the operator-facing internal jargon key `docker.pattern_e`. There were no other `docker.pattern_*` keys.
