# Asset VFS Contract

Asset VFS 是 runtime、cook、package、player host 和 AstraEMU 共用的资产定位契约。它解决“从哪里读 payload”和“如何证明读到的是授权内容”，不改变 package/save 容器格式，也不把旧引擎 pack 当成 `.astrapkg` 的替代品。

`astra-asset` 持有 VFS public contract：asset id、mount descriptor、locator、resolve report、权限和诊断都在这里定义。`astra-package` 只是 package-backed mount source；legacy pack reader、authorized local source 和 overlay 都是同一 VFS 解析层下的 mount family。

## Mount Family

| Mount kind | 用途 | 读写边界 |
| --- | --- | --- |
| `package` | 读取 `.astrapkg` 内的 cooked asset、manifest、policy bundle、ModelBundle 和 report section | 只通过 section ref、offset、size、hash 和 codec 读取；不能绕过 section table |
| `local_authorized` | 本地合法数据根、开发期 import source、私有 acceptance root | 只记录 mount alias 和相对 key；report 不写本地 root |
| `legacy_pack` | FVP `.bin`、KrKr XP3、BGI PackFile、Artemis PFS、Siglus Scene.pck、SoftPAL PAC/DAT、Minori PAZ 等旧引擎资源包 | pack reader 只输出 entry map、offset、size、hash、media kind 和诊断；不写 payload |
| `overlay` | patch、mod、翻译覆盖、调试替换和迁移期间的 NativeVN asset 覆盖 | 必须声明 base mount、priority、允许覆盖的 key pattern 和 redaction policy |

Mount 顺序由 project profile 或 case profile 显式声明。解析器按固定 `(priority, mount_id, key)` 顺序查找，不能依赖插件加载顺序。多个 mount 命中同一 key 时，只有 profile 明确允许 overlay 才能覆盖；否则输出 blocking diagnostic。

## Locator

```rust
pub struct VfsLocator {
    pub alias: VfsAlias,
    pub key: RelativeAssetKey,
    pub expected_hash: Option<Hash256>,
    pub media_kind: Option<MediaKind>,
}

pub struct VfsMountDescriptor {
    pub mount_id: StableId,
    pub kind: VfsMountKind,
    pub alias: VfsAlias,
    pub profile: ProfileId,
    pub provider: Option<ProviderId>,
    pub redaction: RedactionPolicyId,
}
```

`alias` 是 package、local root 或 legacy pack 的公开名字；`key` 永远是相对路径或 pack entry key。契约禁止把本地绝对路径、native file handle、commercial payload、未授权截图或原始脚本文本写入 locator、save、package section 或 report。

## Read Result

```rust
pub struct VfsResolvedEntry {
    pub locator: VfsLocator,
    pub mount_id: StableId,
    pub source: VfsSourceRef,
    pub entry: Option<PackEntryRef>,
    pub offset: u64,
    pub size: u64,
    pub hash: Hash256,
    pub codec: Option<SectionCodec>,
    pub media_kind: MediaKind,
    pub diagnostics: Vec<Diagnostic>,
}
```

`VfsSourceRef` 只允许引用 package section、local authorized alias、legacy pack alias 或 overlay source。实际 bytes 通过 bounded reader 流出，reader 必须在读取前校验 bounds 和 hash policy。VFS report 只记录 alias、relative key、pack id、entry id、offset、size、hash、media kind 和 diagnostic code。

## Pack Reader Provider

旧引擎 pack reader 是 VFS mount provider，不是 package reader replacement。

```rust
pub trait VfsMountProvider: StableProvider {
    fn capability(&self) -> VfsMountCapabilityReport;
    fn probe(&self, request: VfsProbeRequest) -> ProviderResult<VfsProbeReport>;
    fn open_mount(&self, request: VfsOpenMountRequest) -> ProviderResult<VfsMountHandle>;
    fn resolve(&self, handle: VfsMountHandle, locator: VfsLocator) -> ProviderResult<VfsResolvedEntry>;
    fn read(&self, handle: VfsMountHandle, entry: VfsResolvedEntry, range: ByteRange) -> ProviderResult<BoundedBytes>;
    fn close_mount(&self, handle: VfsMountHandle) -> ProviderResult<VfsCloseReport>;
}
```

Provider 只接收 ABI-safe value、capability ref、section ref 和 mount alias。它不能接收 `RuntimeWorld`、Actor 指针、Editor widget、native file descriptor、renderer/audio handle 或本地绝对路径。需要访问本地授权数据时，host 先把 root 变成 capability，再把 alias 交给 provider。

## Release Gate

VFS gate 至少检查：

- package mount section hash、codec、schema 和 bounds。
- local authorized mount 的 alias、profile、相对 key 和 redaction policy。
- legacy pack mount 的 entry table hash、offset/size bounds、duplicate key、unsupported compression、media kind 和 reader identity/hash。
- overlay mount 的 priority、base mount、allowlist 和 source hash。
- report 和 package section 中不得出现本地 root、payload-like 字段、商业文本、截图、音频、影片或 bytecode。

缺 mount、hash mismatch、entry 越界、reader identity 缺失、overlay 未授权、payload 泄露或路径泄露都必须 blocking。
