# Asset VFS Contract

Asset VFS 是 runtime、cook、package、player host、Editor 和 AstraEMU 共用的资产定位契约。它解决三件事：用统一 URI 找到 bytes，证明 bytes 来自授权 source，并把 report 控制在可发布的证据范围内。它不替代 `.astrapkg`，也不把 legacy pack 变成 package 容器。

`astra-asset` 持有 VFS public contract。`astra-package` 只是其中一种 package-backed source；本地授权目录、legacy pack、overlay patch 和后续 remote/cache source 都挂到同一 VFS mount graph。

## Public URI

唯一公开 locator 是 `VfsUri`：

```text
<prefix>:/<normalized/path>
```

示例：

```text
package:/native-assets/bg.png
local:/probe/manifest.json
fvp:/graph_bg/BG001_000
artemis:/script/main.ast
```

`prefix` 是 VFS namespace，不等同于插件 id。它必须由 project/profile 或 provider 显式注册，并满足 safe symbol 规则。path 统一使用 `/`，禁止空路径、宿主绝对路径、drive prefix、`..`、控制字符、内联 payload 和 URI scheme 嵌套。大小写策略由 prefix config 声明：`case_sensitive`、`case_insensitive` 或 `preserve_with_folded_lookup`。

## Manifest Sections

package 必须写入两个独立 section：

| Section | 职责 |
| --- | --- |
| `asset.vfs_manifest` | prefix registry、mount layer graph、VFS entry table 和 overlay whiteout |
| `asset.catalog` | gameplay/editor 可见 asset id、`vfs_uri`、media kind、tags、bundle/chunk/profile eligibility |

`asset.catalog` 不参与 bytes lookup，也不在 v1 承担 UE `PrimaryAsset` dependency management。bytes lookup 只看 `asset.vfs_manifest` 和 provider capability。

`asset.vfs_manifest` 的核心结构：

```rust
pub struct VfsManifest {
    pub schema: String,
    pub prefixes: Vec<VfsPrefixDescriptor>,
    pub layers: Vec<VfsLayerDescriptor>,
    pub entries: Vec<VfsManifestEntry>,
    pub whiteouts: Vec<VfsWhiteoutEntry>,
}

pub struct VfsPrefixDescriptor {
    pub prefix: String,
    pub provider_id: String,
    pub backend: VfsBackendKind,
    pub case_policy: VfsCasePolicy,
    pub mode: VfsReadWriteMode,
    pub redaction: String,
    pub capabilities: Vec<String>,
}

pub struct VfsManifestEntry {
    pub uri: VfsUri,
    pub layer_id: String,
    pub source: VfsSourceRef,
    pub offset: u64,
    pub size: u64,
    pub hash: Hash256,
    pub codec: Option<String>,
    pub media_kind: String,
    pub diagnostics: Vec<Diagnostic>,
}
```

`VfsSourceRef` 只允许引用 package section、本地授权 alias、overlay source、memory object 或 legacy pack entry。host root、native handle、payload bytes、商业正文、bytecode 和 provider secret 不能进入 manifest、catalog、save、package、report 或日志。

## Mount Graph

VFS 使用 UE 风格 mount graph，而不是一组分散 provider slot：

| Backend | 用途 | 读写边界 |
| --- | --- | --- |
| `package` | 读取 `.astrapkg` 内 cooked asset、manifest、policy bundle、ModelBundle 和 report section | 只通过 section id、offset、size、hash 和 codec 读取 |
| `local_authorized` | 本地合法数据根、开发期 import source、私有 acceptance root | host 进程持有 root capability；manifest/report 只写 prefix、URI、hash、size 和 diagnostic |
| `legacy_pack` | FVP `.bin`、KrKr XP3、BGI PackFile、Artemis PFS、Siglus Scene.pck、SoftPAL PAC/DAT、Minori PAZ 等旧引擎资源包 | reader 输出 entry table hash、entry id、offset、size、hash、media kind 和 diagnostic |
| `overlay` | patch、mod、翻译覆盖、调试替换和迁移期 NativeVN asset 覆盖 | 同一 `VfsUri` namespace 覆盖 lower layer；priority、allowlist、base hash 和 reason 必须显式声明 |
| `memory` | Editor/Tools 的短生命周期 workspace object | 不进入 shipping package；只记录 stable object id、hash 和诊断 |

解析按固定 `(priority, prefix, vfs_uri, layer_id)` 决定，不能依赖插件加载顺序。多个 layer 命中同一 `VfsUri` 时，高 priority 覆盖低 priority；没有 overlay/whiteout 授权的冲突必须 blocking。

## Provider Slot

所有 VFS backend provider 都注册到同一个 slot：

```text
slot = "vfs_provider"
```

provider 注册时声明 `provider_id`、可服务的 prefix pattern、backend capability、phase、packaged eligibility 和依赖。`asset.vfs_manifest.prefixes[*].provider_id` 必须绑定到已注册、可打包、capability 匹配的 `vfs_provider`。同一 slot 允许多个 provider 共存；只有同一 prefix 被多个 selected provider 竞争且 manifest 没有显式选择时才 blocking。

`game_runtime_provider` 仍是单 provider 显式绑定，不复用 VFS 多 provider 规则。

## Overlayfs

Overlay 只在同一 `VfsUri` namespace 内生效。典型 patch：

```text
lower: fvp:/graph_bg/BG001_000
upper: fvp:/graph_bg/BG001_000
whiteout: fvp:/graph_bg/OLD_BG
```

`VfsWhiteoutEntry` 必须包含 `vfs_uri`、`layer_id`、`base_hash`、`allowlist_id` 和 `reason`。Shipping profile 默认只读；Editor/Tools 可以写入 host capability 指向的 VFS workspace，但 package/report 仍只记录 `VfsUri`、hash、change op 和 diagnostic。

## Release Gate

VFS gate 至少检查：

- `asset.vfs_manifest` 和 `asset.catalog` 必须存在，`asset.registry` 一律 blocking。
- `VfsUri` 格式、prefix registry、case policy、provider binding 和 capability 匹配。
- package mount 的 section id、bounds、codec、schema 和 hash。
- local authorized mount 不泄露 host root，且所有 report 只含 URI、hash、size 和 diagnostic。
- overlay priority、whiteout allowlist、base hash、target/profile eligibility 和 reason。
- legacy pack 声明必须有 provider/report；真实 reader 实现留在 Stage 5。
- manifest、catalog、package section 和 report 不得出现本地 root、payload-like 字段、商业文本、截图、音频、影片或 bytecode。

缺 manifest、旧 `asset.registry`、URI 非法、prefix/provider 未注册、capability mismatch、hash mismatch、bounds invalid、overlay 未授权、root leak 或 payload leak 都必须 blocking。
