# Canvas Architecture

**Status:** product direction. No Canvas or Desktop implementation exists in
the monorepo yet.

Canvas is the human workspace for Little Organs. Desktop is its native host.
They form one product surface with one navigation model, state model, and
release experience.

Transport Matters developed Canvas and Inspector as separate experimental
packages. That separation is evidence about useful capabilities, not a package
boundary to transplant.

## Ownership

Canvas owns:

1. Human navigation across sessions and captured turns.
2. The first turn report and payload inspection experience.
3. Local edit interactions and review decisions.
4. Presentation of original, curated, forwarded, response, and audit evidence.
5. Product level empty, loading, failure, and recovery states.

Canvas does not own provider parsing, request serialization, overlay identity,
placement, process execution, authorization policy, or persistence internals.

Desktop owns native window and application lifecycle concerns. It does not
define a second product domain or duplicate Canvas state.

## Service Boundary

Canvas consumes stable `lilod` read and command contracts:

- Session lists and logical session state;
- Schedule placement status when that context activates;
- Runtime lifecycle evidence where the product needs it;
- Transport capture, interpretation, edit, forward, and audit operations;
- Identity authorization outcomes.

Canvas must not read Transport or Session storage directly. The same report
model may render to standalone HTML for diagnostics and tests, but the HTML
artifact is not a separate product surface.

## First Turn Report

The first report needs four judgeable states:

1. What the harness attempted to send.
2. How Transport interpreted it.
3. What Little Organs forwarded after any edit.
4. What the provider returned.

The initial edit affordance should target one tool description by tool name.
Advanced positional editing, registry browsing, overlay distribution, and
breakpoint stacks are deferred.

Raw wire data remains available as a drill down. The default hierarchy between
the curated view and raw wire view requires Stuart's product decision.

## Interaction States

Canvas must represent:

- waiting for the first request;
- request held for review;
- unchanged pass through;
- edit validation failure;
- request forwarded;
- provider response streaming or complete;
- capture or forwarding failure;
- resumed or continued session evidence.

No state may imply that an edit reached the provider until Transport records
the forwarded payload.

The first request blocking model remains open. If blocking is selected, the UI
must make the held state, timeout behavior, cancellation, and failure posture
explicit. If passive capture is selected, Canvas must not present an edit as
affecting traffic that has already been forwarded.

## Security and Evidence

Provider payloads may contain source, prompts, credentials, tool arguments, and
personal data. Canvas follows the redaction, retention, and export policy owned
by the platform contract. It never performs lossy redaction that changes the
stored evidence without a separate provenance record.

HTML diagnostics must escape embedded payloads, preserve long content, and
avoid executable provider content. Transport Matters HTML export tests are the
behavioral reference.

## Deferred Product Surfaces

The first slice excludes a separate Inspector application, overlay registry,
accepted cache, entitlement, signing, remote sharing, multiuser collaboration,
eval and compare, Activity dashboards, and distributed refresh.

## Acceptance Loop

The first Canvas proof is complete when an operator can:

1. launch Claude through `lilo run`;
2. see the first captured request;
3. edit one named tool description;
4. forward a provider valid request;
5. receive the provider response in the harness;
6. inspect original, forwarded, response, and audit evidence in Canvas.

The renderer should also produce a standalone local HTML report from the same
read model for deterministic tests and diagnostics.
