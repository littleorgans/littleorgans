# CLIProxyAPI lessons for the first Transport proof

- Status: assessment supporting issue #37
- Reviewed commit: `eac686d0384546b53dfa73e7f89a0206dd4403eb`
- Assessment date: 2026-08-14
- Primary research:
  [CLIProxyAPI lessons for local development and Transport Matters](/Users/alphab/.mdx/research/cliproxyapi-lessons-for-local-dev-and-transport-matters.md)
- Research path:
  `~/.mdx/research/cliproxyapi-lessons-for-local-dev-and-transport-matters.md`

This assessment maps lessons from CLIProxyAPI onto the first Little Organs
Transport and Canvas proof. It refines the evidence contract and acceptance
work. It does not change the bounded contexts or the delivery order.

## Boundary

CLIProxyAPI remains an external case study. Little Organs will not import its
code, dependencies, services, fixtures, package boundaries, process topology,
credential machinery, or architecture.

The case study contributes failure modes and vocabulary. Little Organs will
implement original behavior inside the existing Session, Runtime, Transport,
and Canvas owners. Original fixtures will use synthetic data, Little Organs
captures, or provider contracts.

## Current contract

The current architecture already carries the main evidence chain:

- Transport owns exact request and response bytes, interpretation, authorized
  transformations, provider-valid serialization, and fidelity evidence.
- An unchanged request forwards its original bytes.
- A changed request passes provider validation before Transport forwards it.
- Unknown provider fields survive interpretation and serialization.
- `SessionId` remains the platform join key across Session, Runtime,
  Transport, and Canvas.
- Canvas presents original, interpreted, forwarded, response, and audit
  evidence through `lilod`.
- Canvas does not claim that an edit reached the provider before Transport
  records the forwarded payload.

These decisions remain valid. The case study adds detail where the first proof
still lacks an explicit contract.

## Contracts to lock before implementation

### Field-level interpretation evidence

Every meaningful adapter transformation needs an explicit field fact:

- source field;
- destination field, when one exists;
- disposition: `preserved`, `normalized`, `synthesized`, `reordered`, or
  `dropped`;
- reason;
- adapter revision.

Exact byte equality proves an unchanged pass-through request. The audit does
not need a redundant preservation fact for every field in that case.

For a changed request, the audit must identify every normalization, synthesis,
reordering, and drop. The first proof must also prove that one named tool
description edit changes only the intended field.

### Identity evidence

`SessionId` remains the authoritative platform identity. Transport may add a
request identifier and a turn identifier for captured records.

Transport records these values as observed evidence:

- harness identity;
- client protocol;
- provider;
- model;
- upstream account pseudonym, when observable;
- source and precedence;
- confidence, ambiguity, and collisions.

Observed provider or gateway values do not replace `SessionId`. Transport does
not own credentials, account selection, affinity, or retry policy.

### Canonical and projected evidence

Transport preserves immutable canonical provider bytes under the access and
retention policy selected in issue #37.

One Transport-owned disclosure policy derives payload projections for Canvas,
HTML, logs, diagnostics, exports, MCP resources, and events. Each projection
records:

- the canonical payload digest;
- the disclosure policy revision;
- each redaction fact.

A redacted projection cannot replace or silently modify canonical evidence.
An authorized raw read remains an explicit operation.

### Downstream commitment

The first proof distinguishes these outcomes:

- failure before downstream commitment;
- failure after partial output or downstream commitment;
- terminal success.

Transport records commitment evidence. Transport does not decide whether to
retry. This boundary lets Canvas explain whether provider output escaped and
whether a retry by another component could duplicate an effect.

## First implementation proof

The first Transport implementation issue should include four original Little
Organs cases:

1. An unchanged Claude request forwards byte for byte and preserves an unknown
   sentinel field.
2. One named tool description edit changes only that field.
3. Streaming events retain order and produce one terminal outcome.
4. Failure before commitment remains distinct from failure after partial
   output.

The verifier should check:

- path confinement for evidence artifacts;
- artifact byte lengths and digests;
- exact unchanged forwarding;
- the allowed mutation;
- field interpretation facts;
- identity provenance;
- audit presence;
- commitment consistency;
- deliberately corrupted evidence fails for the expected reason.

The evidence index may use relative paths, byte lengths, and SHA-256 digests.
The implementation language and final storage shape remain decisions in issue
#37.

## Lesson disposition

| CLIProxyAPI lesson | Little Organs disposition |
| --- | --- |
| Protocol compatibility is behavior | Add the four executable cases to the first Transport issue. Expand the matrix after the visible proof. |
| Record interpretation loss | Lock the field fact contract in issue #37. |
| Model downstream commitment | Lock the two failure outcomes in issue #37. Keep retry ownership outside Transport. |
| Preserve an identity evidence chain | Lock observed identity, source, precedence, and confidence in issue #37. |
| Preserve attempts around one exchange | Defer until captured traffic proves multiple attempts or another context owns retry. |
| Add usage quality states | Defer until a read model exposes normalized usage. Preserve raw provider values now. |
| Publish related live state as one generation | Defer until related mutable Transport state exists. |
| Serialize reload work | Defer until Transport supports live reload. |
| Treat model metadata as an observed claim | Record observed model identity now. Defer capability probes and certification. |
| Bound caches and registries | Apply when the first cache or registry is proposed. Neither belongs in the first proof. |
| Apply one sanitizer to every sink | Adapt as one Transport-owned disclosure policy over immutable canonical evidence. |
| Keep raw evidence beside interpretation | Already required by the Transport and Canvas architecture. |

## Delivery effect

The attack order in issue #42 remains unchanged:

```text
#35 launch payload decision
  |
  v
#41 typed launch contract and optional payload
  |
  +---- #37 first proof and evidence decisions
  |
  v
first Transport vertical slice with executable evidence cases
  |
  v
first Canvas report slice
```

This assessment adds no pre-Transport issue. Issue #37 owns the decisions. The
first Transport issue owns the executable cases and their verifier.

## Explicit deferrals

The case study does not justify these additions before the visible proof:

- retry ownership or account selection;
- usage aggregation;
- configuration generations;
- reload queues and debounce;
- model capability registries;
- caches and registry capacity mechanisms;
- credentials, affinity, or cooldown;
- plugins, management interfaces, or gateway infrastructure.

Add one of these only when a measured Little Organs requirement has an existing
owner or requires a deliberate new context decision.
