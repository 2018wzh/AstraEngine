# ADR 0006: Text-First Source Data

Status: Accepted

## Context

AstraEngine is intended for human authors, AI agents, MCP clients, Git workflows, review queues, build tools, and release gates. Binary-only or tool-private project data would make AI editing brittle, make diffs hard to review, and make automated validation weaker.

Visual novels still need binary media assets such as images, audio, fonts, Live2D files, and Spine files. Those binaries need AI-readable metadata without embedding semantic data into binary formats.

## Decision

AstraEngine source project data will be text-first.

The canonical source format is YAML validated through JSON Schema:

- YAML is the human and AI editable source format.
- JSON Schema validates parsed data nodes.
- Comments are allowed in YAML but are not part of schema validation.
- Long text should use YAML block scalars.
- Lists should prefer stable IDs over positional semantics.

Binary assets use sidecar metadata:

- Each binary source asset has a sibling `.asset.yaml` file where practical.
- Example: `alice.png` uses `alice.png.asset.yaml`.
- Sidecar metadata is the canonical source for asset ID, type, tags, origin, dependencies, description, AI notes, license, and cook settings.
- `AssetRegistry` is generated from sidecars and is not the primary hand-edited source.

## Consequences

- Git diffs and reviews operate on text source data.
- AI and MCP tools edit YAML source files instead of generated registry files.
- Cooked content and DerivedDataCache remain generated outputs.
- Release Gate must validate YAML syntax, schema conformance, duplicate IDs, broken dependencies, missing sidecars, and invalid AI-editable fields.
- Tools may generate compact JSON or binary runtime registries from text sources during cook.

