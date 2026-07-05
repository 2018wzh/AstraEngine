# Package And Save

Package and save share one self-describing container. Rust types are schema source; section payload defaults to `serde` + `postcard`.

## Container

```rust
pub struct AstraContainerHeader {
    pub magic: [u8; 8],
    pub version: SchemaVersion,
    pub section_count: u32,
    pub flags: ContainerFlags,
    pub endian: Endian,
    pub alignment: u32,
}

pub struct SectionEntry {
    pub id: SectionId,
    pub schema: SchemaId,
    pub version: SchemaVersion,
    pub offset: u64,
    pub length: u64,
    pub hash: Hash256,
    pub codec: SectionCodec,
    pub encryption: Option<EncryptionDescriptor>,
    pub migration: MigrationPolicy,
}
```

Section ids are stable strings hashed to `SectionId`. v1 uses little-endian and 8 byte payload alignment. Readers validate magic、version、bounds、alignment、hash、codec、schema id and footer hash before decoding payload.

## Section ABI

```rust
pub enum SectionCodec {
    Postcard,
    Raw,
    Zstd,
}

pub struct EncryptionDescriptor {
    pub provider: ProviderId,
    pub method: String,
    pub key_ref: ExternalKeyRef,
}
```

Encryption descriptor 只记录 provider 能力和外部 key reference。Package writer 不内置商业 key，不提供访问控制绕过。

## Package Sections

Minimum package sections:

- `schema.registry`
- `asset.registry`
- `compiled_story`
- `luau.policy_bundle`
- `luau.policy_lock`
- `media.manifest`
- `provider.policy`
- `module.fingerprint`
- `release.summary`
- `scenario.refs`
- `platform.eligibility`

## Save Sections

Minimum save sections:

- `runtime.world`
- `runtime.events`
- `runtime.await_queue`
- `vn.core_state`
- `vn.backlog`
- `vn.read_state`
- `vn.voice_replay`
- `luau.policy_snapshot`
- `audio.graph_state`
- `filter.graph_state`
- `ai.committed_output`
- `plugin.opaque`
- `migration.manifest`

## Migration

```rust
pub trait SchemaMigrator {
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, bytes: &[u8]) -> Result<Vec<u8>, MigrationError>;
}
```

Release Gate validates a complete chain from `minimum_supported_version` to current version. Missing migrator blocks package and save import.

## Report Evidence

Package validation report includes section table hash, schema registry hash, policy lock hash and migration chain status. It never includes raw media payload or full localized text.

## Tests

```bash
cargo test -p astra-package package_roundtrip
cargo test -p astra-runtime save_replay
astra package validate target/nativevn.astrapkg --profile desktop-release
```

Expected: invalid offset, hash mismatch and missing migration produce blocking diagnostics.
