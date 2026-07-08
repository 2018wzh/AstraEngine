# Package And Save

Package and save share one self-describing container implemented by `astra-package`. Rust types are schema source; section payload defaults to `serde` + `postcard`, with `Raw` and `Zstd` available for cooked assets.

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

Encryption descriptor 只记录 provider 能力、AAD hash 和外部 key reference。Package writer 不内置商业 key，不提供访问控制绕过；没有匹配 crypto provider 时，reader 只报告 blocking diagnostic。

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
- `target.manifest`：只包含当前包内单一 packaged `Game` target
- `release.summary`
- `scenario.refs`
- `platform.eligibility`

AI ModelBundle 是 package 的一等 cook artifact，不使用 project-level `package_sections` 携带模型 payload。后续 ONNX provider 设计新增以下 section 族，状态为 `SPEC_READY`：

- `ai.model_bundle_manifest`：`postcard` 编码 manifest，记录 bundle id、pipeline kind、model family、model fingerprint、license/provenance、fine-tune provenance、redistribution、voice authorization、profile budget、platform targets、VFS mount id、runtime section refs、model section refs 和 custom op sidecar refs。
- `ai.model_artifact.*`：模型权重、external data、tokenizer、sampler、scheduler、vocoder、pre/post-process config 等 payload section。大型 payload 使用 `Raw` 或 `Zstd`；section table 记录 hash、stored hash、decoded length、codec、migration 和可选 encryption。
- `ai.onnx_runtime.*`：按 target/profile 锁定的 reduced ONNX Runtime、Web runtime adapter 或 runtime dependency section。开发期可以下载，release package 只能引用 Engine recipe 产出的 vendor cache。
- `ai.custom_op_sidecar.*`：项目自管 ORT custom op sidecar。每个 sidecar 必须声明平台、hash、license、加载策略和目标运行证据；缺失声明或试图暴露 Engine object/native handle 时阻断。

ModelBundle manifest 只保存 package/VFS section ref，不记录源文件绝对路径。Bundled、on-demand 和 external 模型分发都必须落成 `.astrapkg`、patch package、DLC package 或受控 package source，由 VFS mount 解析；不允许 Shipping provider 直接读取 loose sidecar。

Project-level `package_sections` descriptor 只适合脱敏 report 或 manifest。它不能写入模型权重、tokenizer、runtime binary、custom op、商业文本、截图、音频、影片或任何可复原源数据。

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

ONNX Runtime generated output 复用 Runtime save extra section，不创建独立存档格式：

- `ai.generated_artifact.*`：流式提交后的文本、图像、语音或多模态 chunk。每个 section 记录 content type、model fingerprint、provider profile、validator result、chunk hash、codec、migration 和可选 encryption。
- `ai.generated_artifact_manifest`：`postcard` 编码索引，记录 committed output 到 artifact section 的映射、VFS/package source、save budget warning 和 replay policy。

正式 replay 读取 save payload，不请求 provider。save 体积超过 AI profile 预算只产生 warning；缺失 section hash、migration、crypto provider 或 committed output 映射时阻断。

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
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game
```

Expected: invalid offset, hash mismatch and missing migration produce blocking diagnostics.
