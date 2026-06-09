# Editor

Status: Phase 0 scaffold. Editor UI and PIE are planned, not implemented.

## Overview

This section will document Editor, PIE, Inspector, Debugger, Review Queue, Content Browser, Graph/Timeline tools, and Cook/Package panels.

## Key Concepts

- Editor is an authoring and debugging tool, not a packaged runtime dependency.
- Editor observes and commands runtime through public inspector/debugger APIs.
- Editor AI writes must pass Review Queue or an explicit trusted session.
- Canonical source remains text-first and reviewable.

## Architecture

Primary design references:

- [Editor and Pipeline](../../design/editor-and-pipeline.md)
- [Editor UI AI Collaboration Prototype](../../design/editor-ui-ai-collaboration-prototype.md)
- [AI Collaboration](../../design/ai-collaboration.md)
- [MCP Integration](../../design/mcp-integration.md)

## Programming Guide

Future pages should cover Project Wizard, Content Browser, Inspector metadata, Runtime Debugger, Review Queue, PIE, Save/Replay Inspector, and profiling panels.

## API Reference

Planned APIs include `IEditorPanelProvider`, inspector snapshots, command descriptors, review item descriptors, and runtime debugger commands.

## Examples

Planned examples include opening a project, inspecting an Actor, reviewing an AI content draft, and debugging a replay mismatch.

## Troubleshooting

- Do not document Editor-only state as runtime state.
- Do not allow Editor panels or widgets through runtime/module ABI.
- Mark screenshots and workflows as planned until the Editor exists.
