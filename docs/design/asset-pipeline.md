# Asset Pipeline 设计

状态：Target Architecture  
定位：Astra 从 canonical source 到 cooked package 的完整内容管线，包括 AssetId、VFS、Importer、Cooker、DerivedDataCache、Package、Hot Reload 和 Release Gate。

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

## 8. Package Manifest

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

## 9. Hot Reload

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

## 10. Release Gate

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

## 11. Tool Integration

CLI：

- `astra validate <project>`
- `astra import <project> <files> --preset <preset>`
- `astra cook <project> --config <profile>`
- `astra package <project> --profile deterministic`
- `astra inspect <package-or-asset>`

Editor：

- Content Browser。
- Asset Import Wizard。
- Dependency Inspector。
- Cook/Package panel。

MCP：

- inspect only in read-only sessions。
- draft/review in editor sessions。
- no direct Cooked/DDC/package manifest writes。

## 12. Tests

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

## 13. 验收

- NativeVN source project can validate、cook、package、inspect and launch the current source-sidecar package without Editor。
- Cook twice with identical inputs produces identical package hash。
- Content Browser and CLI produce identical diagnostics for broken assets。
- AI draft cannot enter Cook until accepted review。
- Foreign assets remain mount-only by default。
- Runtime package loads assets through package manifest/payload evidence and package mount DTOs; production VFS-backed binary package loading remains future work。
