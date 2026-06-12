# Asset Pipeline 设计

状态：Phase 6 implemented production Asset Pipeline slice  
定位：Astra 从 canonical source 到 cooked binary package 的完整内容管线，包括 AssetId、VFS、Importer、Cooker、DerivedDataCache、binary `.astrapkg`、Hot Reload rollback DTO 和 Asset Release Gate。Phase 7 仍负责真实 renderer/font/audio/filter 执行。

## 1. 目标

Asset Pipeline 必须支持可发布、可调试、可增量构建的 2D/VN 项目：

- Source content 可 validate、import、cook、package、inspect、run。
- Cook 产物 deterministic：相同 source、config、provider 和 release profile 生成相同 package hash。
- Runtime package 可脱离 Editor 启动和加载资产。
- Content Browser、CLI、MCP 和 Release Gate 使用同一 AssetRegistry、Importer、Cooker 和 diagnostics。
- AI draft、foreign mount、license、review、plugin provider、package eligibility 进入统一门禁。

## 2. Asset State Model

```text
External File
  -> Import Preset
  -> Source Asset + Sidecar
  -> AssetRegistry Entry
  -> Cooked Artifact
  -> Package Manifest Entry
  -> Runtime Resource Handle

AI Draft
  -> Review
  -> Accepted Source Asset
  -> Cooked Artifact

Foreign Asset
  -> Read-only Mount
  -> Foreign Registry Entry
  -> Runtime Reference
```

State flags：

- `draft`
- `review_pending`
- `accepted`
- `registered`
- `dirty`
- `cooked`
- `packaged`
- `missing`
- `orphaned`
- `foreign_mount_only`

## 3. AssetId And VFS

AssetId forms：

```text
native:/Characters/Alice/Normal
virtual:/current/character/alice
foreign-bgi:/data/fg.arc#alice_idle
package:/Characters/Alice/Normal
```

VFS mount descriptor：

```yaml
mount_id: project.content
scheme: native
root: Content
access: read_write
priority: 100
policy:
  allow_cook: true
  allow_package: true
```

Rules：

- `native:/` assets require sidecar and stable source path。
- `foreign-*` assets are read-only by default and cannot be copied unless release policy explicitly allows。
- `virtual:/` resolves at runtime/editor through resolver policy and never enters package as unresolved reference。
- Package reader mounts are read-only。

## 4. Sidecar And Registry

Sidecar is source of truth for binary asset metadata：

```yaml
id: native:/Characters/Alice/Normal
schema: astra.asset.image.v1
type: image
source_path: Characters/Alice/normal.png
display_name: Alice Normal
tags: [character, alice]
origin: HumanAuthored
license:
  owner: project
  usage: internal
review:
  status: accepted
cook:
  preset: sprite
dependencies:
  hard: []
  soft: []
```

AssetRegistry generated entry：

```yaml
id: native:/Characters/Alice/Normal
type: image
source_hash: sha256:...
sidecar_hash: sha256:...
dependencies:
  hard: []
  soft: []
importer: astra.import.image
cook_key: ddc:/image/sprite/...
diagnostics: []
```

Rules：

- AssetRegistry is generated; do not edit by human/AI/MCP。
- Registry includes dependency closure, diagnostics and cook keys。
- Duplicate ID is blocking diagnostic。
- Broken hard dependency blocks cook/package；broken soft dependency warns unless release profile escalates。

## 5. Importer Framework

`IAssetImporter` descriptor：

```yaml
importer_id: astra.import.image
contract: IAssetImporter
source_extensions: [.png, .jpg, .jpeg, .webp]
output_types: [image]
preset_schema: astra.import.image.preset.v1
capabilities:
  preview_metadata: true
  batch_import: true
  ai_draft_import: true
permissions:
  project_write: true
  foreign_read: false
```

Import flow：

```text
Select files
  -> Select import preset
  -> Preview metadata
  -> Generate AssetId and sidecar
  -> Validate license/review policy
  -> Copy or reference source according to policy
  -> Refresh AssetRegistry
  -> Emit diagnostics and undo transaction
```

Importer errors：

- unsupported extension。
- decode metadata failure。
- duplicate target AssetId。
- missing license。
- forbidden foreign copy。
- invalid preset schema。

当前内置 importer 覆盖 image、audio、font、text、filter profile、script source；它们共用 `ImportRequest`/`ImporterDescriptor`/`IAssetImporter` 合同，写入 accepted sidecar、source copy 和 import audit。Importer 负责生产 source/sidecar，不直接写 Cooked、DDC 或 package。

## 6. Cooker Framework

`ICookProcessor` descriptor：

```yaml
processor_id: astra.cook.image.sprite
contract: ICookProcessor
input_types: [image]
output_artifacts: [texture]
ddc_key_schema: astra.ddc.image.sprite.v1
package_eligible: true
requires:
  renderer_features: [texture_2d]
```

Cook key：

```text
hash(
  asset id,
  source hash,
  sidecar hash,
  cook preset,
  processor id/version,
  provider feature hash,
  target platform,
  release profile
)
```

Cook flow：

```text
Load AssetRegistry
  -> Resolve dependency graph
  -> Build cook plan
  -> Check DDC
  -> Run processors
  -> Validate artifacts
  -> Write cook manifest
  -> Emit diagnostics
```

Cook must be incremental and deterministic. Processor output must not depend on wall clock, random temp paths, provider response time or Editor state.

当前内置 cook processors 覆盖 image texture、audio stream、font runtime、filter profile、native script、Lua script、timeline/text 和 generic asset binary。Phase 6 生成确定性的 binary payload metadata 与 payload hashes，并记录 provider feature hash；真实 GPU texture upload、font atlas execution、mixer playback 和 GPU filter execution 属于 Phase 7。

## 7. DerivedDataCache

DDC entry：

```yaml
key: ddc:/image/sprite/sha256...
processor: astra.cook.image.sprite
input_hash: sha256:...
output_hash: sha256:...
platform: win64
profile: Release
created_by:
  engine_version: 0.1.0
  module_versions:
    astra.media: 1
artifacts:
  - path: cache:/ddc/image/sha256.bin
```

Rules：

- DDC is cache, not source。
- Corrupt DDC entry is discarded and rebuilt。
- DDC clean policy can remove unused entries。
- AI/MCP cannot write DDC directly。
- DDC key version changes invalidate compatible entries。

Phase 6 local DDC stores artifact bytes under stable derived keys, reports rebuilt/reused/corruption_recovered evidence, and is reused by `astra cook`/`astra package` through `AstraAsset::CookAssetRegistry` rather than duplicated CLI logic。

## 8. Binary Package Format

Phase 6 `.astrapkg` is a binary read-only container:

```text
magic      8 bytes  "ASTRAP6\0"
version    u32      currently 1
manifest   u64      canonical JSON byte length
json       bytes    astra.package.manifest.v1
payloads   bytes    zstd-compressed chunks referenced by payload table
```

The embedded manifest stores `package_hash`, `project_hash`, `cook_manifest`, `runtime_evidence`, module evidence, provider feature hash/profile evidence, and payload table entries with offset, uncompressed size, compressed size, compression, streaming mode, and SHA-256 hash.

`PackageReader` validates header, manifest schema/hash, payload encoding/compression, payload size, and SHA-256 before returning bytes. It supports random-access payload reads, chunked reads, text reads, and a read-only `PackageMountPolicy`/`PackageMount` DTO. Legacy JSON/base64 manifests are accepted for migration diagnostics only; new packages are written as binary zstd containers by `PackageWriter`.

## 9. Package Manifest

Package manifest：

```yaml
schema: astra.package.manifest.v1
package_id: package:/NativeVN
package_hash: sha256:...
project_hash: sha256:...
release_profile: deterministic
assets:
  - id: native:/Characters/Alice/Normal
    type: texture
    artifact: assets/characters/alice_normal.texture
    hash: sha256:...
dependencies: []
modules:
  - id: astra.renderer2d.default
    version: 1
    abi: astra.module.abi.v1
    binary_hash: sha256:...
engine_modules:
  astra.renderer2d: astra.renderer2d.default
diagnostics_report: reports/release_diagnostics.json
```

Package rules：

- Package includes runtime-safe modules only。
- Editor/developer/debug MCP/authoring-only modules excluded by default。
- Package has manifest hash and asset table hash。
- Runtime loads only package manifest, cooked artifacts, runtime config and allowed module binaries。

## 10. Hot Reload

Hot reload invalidation：

```text
Source file changed
  -> Recompute source/sidecar hash
  -> Mark registry entry dirty
  -> Recompute dependency closure
  -> Validate affected assets/scripts/timelines
  -> Re-cook affected artifacts
  -> Swap at frame boundary
  -> Rollback on failure
```

Hot reload levels：

- asset source。
- script/graph/timeline。
- filter profile。
- presentation library。
- development module reload。

Release builds disable source hot reload unless explicit dev package profile.

Phase 6 exposes `HotReloadTransaction` rollback DTOs with stages from Detect through SwitchAtFrameBoundary/RolledBack. Current reload planning is source/sidecar hash driven and retains old resources on validation failure; provider resource preparation/execution hooks deepen in Phase 7/10.

## 11. Release Gate

Asset release gate checks：

- schema valid。
- sidecar present。
- duplicate AssetId absent。
- hard dependencies resolved。
- soft dependencies policy satisfied。
- license/review accepted。
- AI draft accepted or excluded。
- foreign assets mount-only unless policy allows copy。
- import/cook processors packaged eligible。
- package manifest complete。
- DDC artifacts hash verified。
- package payload hash verified。
- registry assets have package-eligible cook artifacts。
- unresolved virtual refs blocked。
- non-packaged provider/module blocked。

Blocking output example：

```yaml
code: ASTRA_RELEASE_ASSET_004
severity: blocking
message: Unreviewed AI asset cannot be packaged
objects:
  - kind: AssetId
    id: native:/Backgrounds/RainyStreet/Draft01
suggested_fixes:
  - open_review_queue
  - reject_draft
```

## 12. Tool Integration

CLI：

- `astra validate <project>`
- `astra import <project> <source-file> --asset-id <native:/...> [--type <type>] [--preset <preset>]`
- `astra cook <project> --config <profile>`
- `astra package <project> --profile deterministic`
- `astra inspect <package-or-asset>`
- `astra run <package> --headless-smoke`
- `astra replay <replay> --compare`

CLI orchestration now calls `AstraAsset` importer/cooker/package APIs for import, cook, package, inspect, package-only run evidence, and replay/package hash evidence instead of owning a parallel package pipeline。

Editor：

- Content Browser。
- Asset Import Wizard。
- Dependency Inspector。
- Cook/Package panel。

MCP：

- inspect only in read-only sessions。
- draft/review in editor sessions。
- no direct Cooked/DDC/package manifest writes。

## 13. Tests

Required tests：

- AssetId parse/normalize。
- sidecar schema validation。
- importer preset validation。
- dependency graph hard/soft references。
- incremental cook invalidation。
- DDC corruption rebuild。
- package manifest deterministic hash。
- release gate blocking for missing dependency、unreviewed AI draft、invalid license、illegal foreign copy。
- package reader loads without Editor。

## 14. 验收

- NativeVN source project can validate、import selected sources、cook、package、inspect and launch a binary `.astrapkg` without Editor。
- Cook twice with identical inputs produces identical package hash。
- Content Browser and CLI produce identical diagnostics for broken assets。
- AI draft cannot enter Cook until accepted review。
- Foreign assets remain mount-only by default。
- Runtime package reads assets through `PackageReader` random-access/chunked paths, save/replay evidence records package manifest hash/profile/provider feature hash, and release gate blocks corrupt package/DDC/payload mismatches。
