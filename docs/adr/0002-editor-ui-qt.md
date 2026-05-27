# ADR 0002: Qt for First-Phase Editor UI

Status: Accepted

## Context

AstraEditor needs a productive desktop UI for project browsing, asset tools, script editing, scene preview, review queues, and build/package workflows. Candidate UI technologies include Dear ImGui, Qt, WebView, native custom UI, and hybrid approaches.

## Decision

The first-phase editor UI direction is Qt. Phase 0 records this design decision only; it does not add Qt dependencies or editor implementation code.

## Consequences

- Editor architecture should assume a retained desktop UI toolkit can host complex panels, document views, trees, tables, and inspectors.
- Runtime modules remain independent of Qt and Editor code.
- Build scripts should keep editor dependencies behind `Astra_BUILD_EDITOR`.
- If Qt integration proves too heavy, a later ADR must supersede this decision before switching UI stacks.
