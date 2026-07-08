# Package And Save

Package and save share one self-describing container implemented by `astra-package`. Rust types are schema source; section payload defaults to `serde` + `postcard`, with `Raw` and `Zstd` available for cooked assets.

`.astrapkg` 是控制面和证据面容器。它保存 target、provider policy、schema、compiled IR、cooked asset、ModelBundle、scenario refs、release summary 和脱敏 report section；legacy pack reader 不能替代 `.astrapkg`，只能作为 Asset VFS 的 `legacy_pack` mount source。

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

Stage 3 NativeVN 还会写入 `vn.policy_bundle_manifest` 和 `vn.policy_bundle_source_cache`。前者记录 policy id、相对 entry、capability、dependency、lock hash、source hash、byte size 和 source cache section；后者保存包内可执行的官方 Luau source。Release report 只输出 hash、size、section id 和 diagnostic，不输出 Luau source payload。

Project-level `package_sections` descriptor 可在 cook 阶段复制额外脱敏 section。每个条目包含 `id`、`schema`、相对 `path`、`codec`，并可用 `targets` 和 `profiles` 限定写入范围。cook 会阻断绝对路径、`..`、重复 section id 和 target/profile 不匹配的误写；package 里只保留 section payload、hash、codec 和 schema，不记录源文件绝对路径。

TsuiNoSora Stage 3 只通过这个机制写入 sanitized report sections。真实源文件、解包产物、商业截图、文本、音频、影片和本地路径不能作为 `package_sections` payload 进入仓库或公开 package。

AI ModelBundle 是 package 的一等 cook artifact，不使用 project-level `package_sections` 携带模型 payload。后续 ONNX provider 设计新增以下 section 族，状态为 `SPEC_READY`：

- `ai.model_bundle_manifest`：`postcard` 编码 manifest，记录 bundle id、pipeline kind、model family、model fingerprint、license/provenance、fine-tune provenance、redistribution、voice authorization、profile budget、platform targets、VFS mount id、runtime section refs、model section refs 和 custom op sidecar refs。
- `ai.model_artifact.*`：模型权重、external data、tokenizer、sampler、scheduler、vocoder、pre/post-process config 等 payload section。大型 payload 使用 `Raw` 或 `Zstd`；section table 记录 hash、stored hash、decoded length、codec、migration 和可选 encryption。
- `ai.onnx_runtime.*`：按 target/profile 锁定的 reduced ONNX Runtime、Web runtime adapter 或 runtime dependency section。开发期可以下载，release package 只能引用 Engine recipe 产出的 vendor cache。
- `ai.custom_op_sidecar.*`：项目自管 ORT custom op sidecar。每个 sidecar 必须声明平台、hash、license、加载策略和目标运行证据；缺失声明或试图暴露 Engine object/native handle 时阻断。

ModelBundle manifest 只保存 package/VFS section ref，不记录源文件绝对路径。Bundled、on-demand 和 external 模型分发都必须落成 `.astrapkg`、patch package、DLC package 或受控 package source，由 VFS mount 解析；不允许 Shipping provider 直接读取 loose sidecar。

Project-level `package_sections` descriptor 只适合脱敏 report 或 manifest。它不能写入模型权重、tokenizer、runtime binary、custom op、商业文本、截图、音频、影片或任何可复原源数据。

## Package-backed VFS

`astra-package` 暴露 package-backed mount source。VFS 层通过 section id、offset、length、codec、hash 和 schema 读取 package entry；package reader 本身不解释 VN、AI、EMU 或 legacy pack 语义。

VFS mount family 包括 package mount、local authorized mount、legacy pack mount 和 overlay mount。`astra-package` 只实现 package mount；FVP `.bin`、XP3、PFS、Scene.pck、PAC/DAT、PAZ 等旧引擎资源包必须由 `VfsMountProvider` 解析后挂载为 `legacy_pack`，并把 reader identity、entry table hash、entry offset、size、hash、media kind 和 diagnostic 写入 release report。

Package validation report 可以引用 VFS resolve report，但只记录 alias、relative key、section or entry id、offset、size、hash、codec、media kind 和 diagnostic。它不能写本地 root、payload bytes、商业文本、截图、音频、影片、bytecode、provider secret 或 native handle。

## Save Sections

Minimum save sections:

- `runtime.world`
- `migration.manifest`

Runtime save 可以携带模块 extra sections。Stage 3 AstraVN 当前写入：

- `vn.runtime_state`：`postcard` 编码的 `VnRuntimeStateSave`，包含 VN runtime state、state hash、backlog、read-state、voice replay、route flags、变量、system state 和 pending wait。
- `vn.policy_state`：`postcard` 编码的 `VnPolicyStateSave`，包含 Luau policy state、mutation trace、rollback scope、replay event metadata 和 serializable snapshots。

以下 section 是后续模块接入点，缺 provider 时不得伪造 payload：

- `audio.graph_state`
- `filter.graph_state`
- `ai.committed_output`
- `plugin.opaque`

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
cargo test -p astra-vn --test vn_save_container
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game
```

Expected: invalid offset, hash mismatch and missing migration produce blocking diagnostics.
