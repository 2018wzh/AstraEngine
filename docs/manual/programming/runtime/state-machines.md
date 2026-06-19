# StateMachine Guide

Status: Phase 2 Foundation.

## Overview

StateMachine foundation supports actor-bound state transitions driven by `RuntimeEvent` type IDs.

## Key Concepts

- Definitions are registered on `RuntimeWorld`.
- Actor instances store state in an `astra.state_machine` component.
- Transitions are deterministic because they use event type and current state.

## Architecture

Design references:

- [Runtime Core](../../../design/runtime-core.md)
- [Actor / Component / ECS Hybrid](../../../design/actor-component-ecs-hybrid.md)

## Programming Guide

Attach a component with `state_machine_id` and `current_state`, register matching transitions, then emit an event.

## API Reference

- `Astra::Runtime::StateMachineDefinition`
- `Astra::Runtime::StateTransition`

## Examples

Compiled transition coverage lives in `Engine/Tests/PhaseTests.cpp`.

## Troubleshooting

- Guard/action APIs, delayed event migration, hot reload validation, and debugger hooks remain later production work.


