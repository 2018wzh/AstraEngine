# Package And Save

Package and save share one self-describing container implemented by `astra-package`. Rust types are schema source; section payload defaults to `serde` + `postcard`, with `Raw` and `Zstd` available for cooked assets.

`.astrapkg` 保存 target、provider policy、schema、compiled IR、cooked asset、scenario refs、release summary、`asset.vfs_manifest`、`asset.catalog` 和脱敏 report section。资源工具的 legacy pack reader 不能替代 package 容器；AstraEMU Family 使用自己的原生文件和存档，不经过 product package。

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
- `cook.summary`
- `asset.vfs_manifest`
- `asset.catalog`
- `compiled_story`
- `luau.policy_bundle`
- `luau.policy_lock`
- `media.manifest`
- `provider.policy`
- `plugin.extension_registry`
- `plugin.dependency_graph`
- `module.fingerprint`
- `target.manifest`：只包含当前包内单一 packaged `Game` target
- `release.summary`
- `scenario.refs`
- `platform.eligibility`

`scenario.refs` 使用 `astra.scenario_refs.v2`，绑定规范化 bundle 相对 `path`、独立 `section_id`、payload SHA-256 与 byte size。Cook 与 reader 继续校验双向唯一、section 存在和 hash/size 一致；真实 Player 自动化仍消费对应输入。它不是旧 YAML VM runner 的兼容入口，也不允许仅列路径或把文件路径直接当 section id。

`provider.policy` 使用 `astra.provider_policy.v3`，`plugin.extension_registry` 使用 `astra.plugin_extension_registry.v2`。Builder 和 reader 都会校验两者 binding 集合、binding hash、provider capability/fingerprint、package/profile/target identity、target manifest 与 VFS backend binding。`target.manifest` 中必须存在 binding 指向的 packaged `Game` target；每个 VFS prefix 必须绑定 `vfs_provider`，backend capability 必须精确匹配，并在当前 target/profile 下唯一 resolve。旧 policy v1/v2、缺 binding 或互相矛盾的控制面 section 不能进入 Player。

NativeVN 还会写入 `vn.policy_bundle_manifest` 和 `vn.policy_bundle_source_cache`。前者记录 policy id、相对 entry、capability、dependency、lock hash、source hash、byte size 和 source cache section；后者保存包内可执行的 Luau source。Release report 只输出 hash、size、section id 和 diagnostic，不输出 source payload。

Project-level `package_sections` descriptor 可在 cook 阶段复制额外脱敏 section。每个条目包含 `id`、`schema`、相对 `path`、`codec`，并可用 `targets` 和 `profiles` 限定写入范围。cook 会阻断绝对路径、`..`、重复 section id 和 target/profile 不匹配的误写；package 里只保留 section payload、hash、codec 和 schema，并在 `asset.vfs_manifest.entries` 中生成 `package:/...` URI，不记录源文件绝对路径。

终之空工具仅通过这个机制写入 sanitized report sections。真实源文件、解包产物、商业截图、文本、音频、影片和本地路径不能作为公开 report payload。

Project-level `package_sections` descriptor 用于脱敏 report 或 manifest，不能作为商业 payload 或未经审计数据的通用打包通道。

## Package-backed VFS

`astra-package` 暴露 package-backed source。VFS 层通过 `package:/...` URI、section id、offset、length、codec、hash 和 schema 读取 package entry；package reader 本身不解释 VN、AI、EMU 或 legacy pack 语义。

VFS backend 包括 package、local authorized、legacy pack、overlay 和 memory/workspace。`astra-package` 只实现 package backend；FVP `.bin`、XP3、PFS、Scene.pck、PAC/DAT、PAZ 等旧引擎资源包必须由注册在 `vfs_provider` slot 的 provider 解析后挂载为 `legacy_pack` prefix，例如 `fvp:/graph_bg/BG001_000`，并把 reader identity、entry table hash、entry offset、size、hash、media kind 和 diagnostic 写入 release report。

Package validation report 可以引用 VFS resolve report，但只记录 `vfs_uri`、prefix、section or entry id、offset、size、hash、codec、media kind 和 diagnostic。它不能写本地 root、payload bytes、商业文本、截图、音频、影片、bytecode、provider secret 或 native handle。`asset.registry` 不再是 package section；release gate 看到它必须 blocking。

## Save Sections

Minimum save sections:

- `runtime.world`
- `migration.manifest`

Engine/VN 存档由 typed session 与 Runtime 容器保存显式状态，Player envelope 不复制权威数据。当前 section/schema 以 Rust reader 为准，不沿用已删除 `astra-vn-save`、gameplay provider 或历史版本号的文档矩阵。普通 tick 不因保存能力而全量编码；恢复预检失败保留当前状态与原文件。

仅为实际模块消费者注册 extra sections，不为延后的 AI output、通用 replay 或不透明插件状态预设 section 族。AstraEMU Family 的原生存档不进入本容器，见 [数据契约](../contracts/data-formats.md#save)。

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

`schema.registry` 当前使用 `astra.schema_registry.v2`，由 `PackageBuilder` 从实际 section table 派生每个 `section_id/schema/version`，不接受调用方提供的空成功 registry。`PackageReader` 要求 registry 与除自身外的 section table 双向一一对应，并在任何 release/runtime reader 读取 policy 或 payload 前验证 required section schema、package identity、container version 和 registry binding。旧 v1、缺项、多项、unknown section 或 schema/version drift 都是 package integrity blocker。

Container 已硬切 v2。128-byte footer 绑定 file length、kind/version、header hash、section-table hash 与 ordered section Merkle root；leaf 包含 section id、schema/version、codec/encryption、offset、stored/decoded length 和双 hash。runtime 通过 `PackageReader::open_source` 提供 expected content root，只读取 header、footer、table 和实际访问的 section；footer 自报但未被 launch identity 绑定的 root 不能作为可信 package identity。`package_hash` 现指 canonical content root，不再指整个容器文件的 SHA-256。v1 container 返回稳定迁移阻断诊断，不提供 runtime migrator。

`cook.summary` 使用 `astra.cook_batch_summary.v1`，从 `astra.cook_manifest.v2` 复制 content-bound graph hash、artifact/cache-hit/cooked count 和 concurrency limit。Reader 要求计数闭合且 artifact count 与 package 内 `astra.cooked_asset.v1` section 数量相同；Release Gate 输出 `package.cook_graph`，只记录摘要字段，不记录 DDC root、源路径或 payload。

## Tests

```bash
cargo test -p astra-package package_roundtrip
cargo test -p astra-runtime --test save_load
cargo test -p astra-vn-save --test vn_save_container
astra package validate target/nativevn.astrapkg --profile desktop-release --target nativevn-game
```

Expected: invalid offset, hash mismatch and missing migration produce blocking diagnostics.
