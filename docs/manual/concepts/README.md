# Concepts

Status: Phase 0 scaffold.

## Overview

This section explains AstraEngine's boundaries, non-goals, and how it compares to UE-class runtime completeness within a 2D / VN-first scope.

## Key Concepts

- UE-class means engineering completeness for Astra's scope, not UE feature parity.
- Core is deliberately small and does not own VN, AI, Editor, renderer backend, Lua, or legacy VM concepts.
- Actor/Component plus StateMachineRuntime is the public runtime model.
- Dynamic modules, provider contracts, and release gate evidence are central to customization.

## Architecture

Read [Goals](../../design/goals.md), [Architecture](../../design/architecture.md), [Roadmap](../../design/roadmap.md), and [Glossary](../../design/glossary.md) for the complete target model.

## Programming Guide

Use these questions before adding a system:

- Which layer owns it?
- Does it introduce a forbidden dependency into Core or Runtime?
- Can it be represented by diagnostics, schema, tests, and manual docs?
- Does it require save/replay, review, audit, or release-gate evidence?

## API Reference

Concept pages do not define API by themselves. Stable API must be indexed in [API Reference](../api/README.md) once headers exist.

## Examples

Examples of out-of-scope work for Core:

- AI provider integration.
- Legacy VM instruction semantics.
- Editor widget ownership.
- SDL, renderer, audio, or OS handles in public ABI.

## Troubleshooting

- If a feature seems useful but breaks a boundary, update design/ADR first.
- Keep expansion-track legacy work behind stable native runtime APIs.


