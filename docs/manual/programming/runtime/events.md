# Runtime Event Guide

Status: Phase 2 Foundation.

## Overview

Runtime events are the stable communication DTO between future Script, Presentation, AI intent, Editor debugging, and Runtime systems.

## Key Concepts

- Events use stable IDs and monotonically assigned sequences.
- Queued events run in sequence order during `Tick()`.
- Deferred events become queued on the next tick.
- Immediate events share the same storage path in the foundation implementation.

## Architecture

Design reference: [Runtime Core](../../../design/runtime-core.md).

## Programming Guide

The foundation event fields are `event_id`, `type`, `category`, `sequence`, `frame_index`, `source`, `target`, `payload_schema`, `payload`, and `trace`.

## API Reference

- `Astra::Runtime::RuntimeEvent`
- `Astra::Runtime::RuntimeEventBus`

## Examples

See `Runtime world orders events advances state machine and saves loads` in `Engine/Tests/Phase1Tests.cpp`.

## Troubleshooting

- Unknown payload schemas are not production-validated yet.
- Use stable actor IDs in endpoints; do not pass actor pointers.
