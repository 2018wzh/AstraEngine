# Stage 8 AstraRPG Server/Client Protocol

Stage 8 adds AstraRPG's extensible Server/Client protocol after Stage 7 local runtime semantics are stable. The protocol is part of AstraRPG, but it is not a Stage 7 completion requirement.

## Exit Criteria

- `rpg.net.*` DTO define handshake、session、seat、action transcript、state sync cursor、privacy label、audit event and replay envelope.
- Server and client crates exchange deterministic action transcripts without leaking provider secrets、local roots、private GM data or payload bodies.
- Seat authority and transcript redaction match Stage 7 local behavior.
- Replaying a network session from synced transcript does not contact live AI provider and yields matching state/event/provider hashes.
- Release Gate blocks protocol version mismatch、seat authority mismatch、unredacted transcript、missing audit and replay divergence.

## Work Items

### S8-RPG-NET-CONTRACT-01 Network Protocol Contract

**Goal:** Define `rpg.net.*` DTO and package/report redaction rules.

**Depends On:** `S7-RPG-TRPG-01`.

**Target Paths:** `Docs/contracts/rpg-trpg.md`, `Engine/Source/Modules/AstraRPG/astra-rpg-net/` planned.

**Steps:** Add handshake、session、seat sync、action transcript sync、state cursor、audit and replay envelope DTO.

**Done Evidence:** `cargo test -p astra-rpg-net protocol_schema`.

**Linked Test IDs:** `T-S8-RPG-NET-CONTRACT-01`.

### S8-RPG-NET-SERVER-01 Server Runtime

**Goal:** Add server-side session authority for RPG/TRPG seats and action transcripts.

**Depends On:** `S8-RPG-NET-CONTRACT-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-server/` planned.

**Steps:** Implement handshake、seat assignment、permission validation、action transcript append and redacted audit report.

**Done Evidence:** `cargo test -p astra-rpg-server server_session`.

**Linked Test IDs:** `T-S8-RPG-NET-SERVER-01`.

### S8-RPG-NET-CLIENT-01 Client Runtime

**Goal:** Add client-side transcript sync and privacy-aware local view.

**Depends On:** `S8-RPG-NET-CONTRACT-01`.

**Target Paths:** `Engine/Source/Modules/AstraRPG/astra-rpg-client/` planned.

**Steps:** Implement handshake validation、seat permissions、local transcript view、redaction boundary and reconnect cursor.

**Done Evidence:** `cargo test -p astra-rpg-client client_session`.

**Linked Test IDs:** `T-S8-RPG-NET-CLIENT-01`.

### S8-RPG-NET-REPLAY-01 Network Replay Gate

**Goal:** Ensure network sessions replay deterministically without live providers.

**Depends On:** `S8-RPG-NET-SERVER-01`, `S8-RPG-NET-CLIENT-01`.

**Target Paths:** `Engine/Source/Developer/astra-release/tests/rpg_network_gate.rs` planned.

**Steps:** Validate transcript sync、seat authority、redaction、state/event/provider hash and provider-free replay.

**Done Evidence:** `cargo test -p astra-release rpg_network_gate`.

**Linked Test IDs:** `T-S8-RPG-NET-REPLAY-01`.

## Status

Stage 8 is `SPEC_READY` as protocol design. It has no implementation and must not block Stage 7 completion.
