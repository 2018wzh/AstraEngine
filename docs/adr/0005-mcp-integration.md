# ADR 0005: MCP Integration for Editor and Developer Tools

Status: Accepted

## Context

AstraEngine is designed for AI-assisted VN production. External agents need a stable way to inspect project context, modify source data, run validation, execute headless tests, cook content, package builds, inspect compatibility reports, and generate audit output.

The existing architecture already has useful boundaries: RuntimeCommand, Runtime Services facades, Review Queue, Audit Log, AssetRegistry, VFS, build tools, and dynamic modules. MCP should use these boundaries instead of reaching into runtime internals.

## Decision

AstraEngine will provide MCP integration as an Editor/Developer plugin capability.

The first MCP server is a trusted local development interface:

- It is disabled by default.
- It is enabled explicitly by the user for a project session.
- It is not included in packaged runtime builds by default.
- It may directly write workspace/project source files in a trusted session.
- It must record every mutating operation in an MCP Operation Log.
- It must not expose plaintext API keys, unauthorized external paths, or EnTT/ECS internals.

MCP is not an AI provider. It is a protocol layer that lets an external host or agent call AstraEngine tools and read project resources.

## Consequences

- MCP tools can cover the full development toolchain: project inspection, asset sidecar editing, script validation, story graph validation, localization editing, compatibility probing, headless tests, cook, package, release gates, and audit reports.
- Trusted direct write is a deliberate exception to the default AI Review Queue path. It is allowed only for explicit MCP trusted sessions.
- Review Queue remains available as an optional MCP tool, but is not mandatory for trusted direct writes.
- MCP resources and tools must return stable engine DTOs and text source data, not internal EnTT handles or editor UI objects.
- If packaged runtime MCP support is ever needed, it requires a new ADR because it changes security, privacy, distribution, and network assumptions.
