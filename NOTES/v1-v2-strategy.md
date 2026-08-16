---
title: "littleorgans v1 and v2 strategy: local first laboratory and Kubernetes target"
type: design-note
tags: [littleorgans, helioy, v1, v2, kubernetes, knative, crds, design-strategy, scope]
status: active
created: 2026-05-25
source: conversation with Stuart 2026-05-25 during synthesis rev07 and the R11 phase
---

## Two stages, one design

| Stage | Name | Scale | Role |
| --- | --- | --- | --- |
| **v1** | **littleorgans** | One operator, one host, and one `lilod` process | Local design laboratory for the bounded contexts, command model, and data model. |
| **v2** | To be decided | Cluster scale | The proven v1 boundaries mapped to Kubernetes services, CRDs, and controllers. |

The v1 and v2 split is a product strategy. v1 provides the conditions for
fast local refactoring. v2 applies the proven design to distributed operation.
The v2 mapping remains outside v0.8.0.

## Current v1 operating model

The current v1 system uses these local simplifications:

- `internal/session/app/src/compose.rs` builds one `lilod` process with one
  socket, one `SessionService`, and one `RuntimeService`.
- Session calls Runtime through `InProcessRuntime`. `RtmdDriver` retains socket
  and conformance coverage for the same `RuntimePort` contract.
- One Postgres database holds Session, Runtime lifecycle, Identity audit, mail,
  and spawn intent records. `LiloDb` owns the shared `sqlx::PgPool`.
- Runtime events remain in the local JSONL event log. Postgres does not replace
  that event file.
- A two transaction intent pattern brackets the Runtime process side effect.
- `~/.lilo/` holds config, run files, events, logs, cache, and temporary files.
  `LILO_DATABASE_URL` selects Postgres.
- Identity authorizes as a library inside Session and Runtime.

Schedule, Transport, Canvas, and Desktop remain reserved. Schedule will own
placement. Transport will own provider wire evidence and interpretation. Canvas
and Desktop will form one product surface through `lilod` read and command
models.

## Kubernetes vocabulary is a mapping contract

| v1 design | v2 Kubernetes mapping |
| --- | --- |
| `SessionService` and `RuntimeService` inside composed `lilod` | Separate services with explicit network contracts when distribution requires them. |
| `session_spawn_intents` plus startup reconciliation | A CRD and controller reconciliation loop. |
| `lilo run`, `lilo create session`, and `lilo get session` | A kubectl shaped command model over the cluster control plane. |
| Identity authorization and audit at the library boundary | API admission, service identity, and RBAC. |
| `RuntimePort` between Session and Runtime | The execution contract that Schedule can mediate without changing Session meaning. |
| Typed `SessionId` shared by Session and Runtime | The stable join key across Session, Runtime, Transport, and Canvas. |
| One Postgres database and one `LiloDb` pool | Context owned stores and distributed reconciliation. |
| Local JSONL Runtime events | A durable cluster event stream. |

The bounded contexts survive the topology change. Their current process layout
does not define their future deployment layout.

## The current transaction case

Session backed launch uses two Postgres transactions with the Runtime process
side effect between them:

1. Transaction A records the authorization audit, the pending
   `session_spawn_intents` row, and the Runtime `Forking` lifecycle.
2. Session calls Runtime. Runtime starts the shim and performs the first
   lifecycle transition to `Running`.
3. Transaction B inserts the `Running` Session row and its labels, persists the
   returned Runtime lifecycle, and resolves the spawn intent.
4. Session appends the Runtime `Running` event after Transaction B commits.

This sequence gives startup reconciliation enough evidence to finish or abort a
launch after a process failure. In v2, a controller and distributed store will
provide the same intent and observed state relationship. A local transaction
cannot span the cluster process side effect.

## Local choices that v2 replaces

Several v1 choices belong to a single operator and host:

- `~/.lilo/` and the local Unix socket become cluster configuration and API
  endpoints.
- Direct `InProcessRuntime` calls become a Schedule mediated execution path.
- Force pushed generated mirrors remain part of the v1 release process. A
  hosted product needs its own release controls.
- Local Identity authorization becomes service identity, admission, and RBAC.

The command model and bounded context ownership can remain stable while those
mechanisms change.

## Historical SQLite design

The May 2026 R11 design used one shared SQLite file, WAL, one
`sqlx::SqlitePool`, and `BEGIN IMMEDIATE` for v1. The Postgres cutover
superseded those storage and locking choices before release. The intent record,
the two transaction sequence, and reconciliation remain part of the current
design.

## Source

Stuart set this strategy on 2026-05-25 during synthesis rev07 and the Phase 6
R11 lock. The active architecture now reflects later monorepo composition and
Postgres decisions.

- Synthesis: `~/.mdx/projects/littleorgans-monorepo-migration--synthesis.md`
- Direction document: `~/.mdx/projects/helioy-product-direction.md`
- Kubernetes layout research: `~/.mdx/research/kubernetes-monorepo-layout-patterns.md`
