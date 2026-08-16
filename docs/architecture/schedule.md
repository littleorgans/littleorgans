# Schedule Architecture

**Status:** target context, reserved in v0.8. No crate, daemon, schema, or
command namespace exists yet.

Schedule is the local placement and topology reconciler for littleorgans. It
occupies the kube scheduler shaped boundary between Session and Runtime. The
signed pre monorepo Schedule Matters specification remains the source for its
ownership model. This document rebases that model onto the current monorepo.

## Ownership

Schedule owns:

1. Desired local topology for sessions, windows, and panes.
2. Stable Schedule identities for realized topology.
3. Opaque occupant token to pane bindings.
4. Placement decisions.
5. Reconciliation between desired topology, live topology, and Runtime
   lifecycle evidence.
6. Mechanical application of declared `Never`, `OnFailure`, and `Always`
   restart policies.
7. Placement evidence such as placed, orphaned, and replaced outcomes.

Schedule does not own:

1. Logical session meaning, roles, labels, selectors, mail, or nudge.
2. Authorization or audit policy.
3. Process launch internals, shim behavior, or tmux command execution.
4. Provider traffic, payload interpretation, overlays, or transcripts.
5. Workflow and orchestration policy.
6. Unspecified trigger policy. The signed model establishes placement and does
   not assign time based or event based triggering. A later product decision
   may place triggers here or in an upstream context without changing sole
   placement ownership.

## Sole Placement Authority

After Schedule activates, every session backed placement goes through it.
`lilo run` desugars to a one occupant placement request. `lilo create session`
applies declarative topology. Existing pane execution resolves and binds the
stable target through Schedule before Runtime starts the process.

The current direct Session to Runtime spawn path is the migration source. It
must be replaced rather than retained as a parallel session backed path.

Raw `lilo runtime spawn` remains the identity gated diagnostic exception. It
creates no Session record, Schedule topology, or occupant binding.

## Stable Identity Ladder

Durable identity resolves from the logical occupant toward current display
data:

```text
Session owned occupant token
    -> Schedule owned pane identity
    -> live tmux pane id
    -> positional address for display only
```

Window insertion, deletion, or renumbering changes display position without
changing the binding. Schedule never reconstructs identity from a positional
address.

Schedule will use the shared typed UUIDv4 family when implementation creates a
real Schedule id concept. The earlier UUIDv7 and SQLite designs are obsolete.
Do not introduce speculative id types before a stored field requires them.

## Thin Topology Intent

The topology manifest carries identity and topology only. Its occupant launch
spec is opaque to Schedule.

```text
manifest version
metadata
workspace
windows
  panes
    occupant token
    restart policy
    working directory
    occupant launch spec
```

Session will prepare the occupant launch spec defined by the [canonical launch
attachment contract](system.md#launch-attachment-contract). Schedule will
validate only the topology fields required to place and attach the occupant.
It will carry the occupant launch spec as a unit and deserialize the optional
outer typed launch attachment only to copy it unchanged. Only Transport will
interpret the attachment fields.

This seam allows Transport capture to work before and after the Schedule
cutover.

## Placement and Reconciliation

The target placement flow is:

1. Decode and validate topology intent.
2. Allocate stable topology identities.
3. Persist desired topology and pending bindings in the shared Postgres
   control plane.
4. Ask Runtime to create or resolve live topology.
5. Persist live tmux ids as realization evidence.
6. Ask Runtime to execute the occupant launch spec in the selected pane.
7. Bind the occupant token to the stable pane identity.
8. Return placement evidence to Session.

Schedule consumes one Runtime lifecycle and topology evidence path. It does not
create a second liveness poller.

On pane or occupant death:

- `Never` records the occupant as orphaned and stops.
- `OnFailure` replaces only when Runtime supplies terminal failure evidence.
- `Always` places a new pane and asks Runtime to perform native resume.

Backoff, retry budgets, role substitution, and workflow decisions belong above
Schedule.

## Transport Boundary

Schedule never sees provider request bytes, normalized payloads, overlays,
transcripts, or fidelity reports. It will copy the optional launch attachment
without interpreting `kind`, `version`, or `value`. See the [canonical launch
attachment contract](system.md#launch-attachment-contract).

Transport records remain joined through `SessionId`. Schedule stores only the
opaque fields required to place and resume the occupant. WebSocket behavior,
HTTPS fallback, and provider continuation rules remain wholly within Transport.

## Migration Notes

The pre monorepo Schedule Matters design remains authoritative for ownership,
stable identity, thin intent, placement, and restart reconciliation. The
following details must not be copied:

- separate repository and daemon assumptions;
- SQLite storage and pool design;
- UUIDv7 identifiers;
- historic crate names and wire paths;
- direct dependency on a Transport Matters launcher;
- positional tmux addresses as any form of stored identity.

Implementation remains outside v0.8 scope. The first Transport and Canvas
slice must preserve this boundary without activating Schedule prematurely.

## Source Decisions

The rebased source is
`~/.mdx/projects/littleorgans-schedule-matters-spec.md`, reviewed through the
May 2026 Schedule Matters consensus passes. Current source and the monorepo
instructions supersede its implementation details.
