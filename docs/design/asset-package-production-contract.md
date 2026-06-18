# Asset / Package Production Contract

状态：Phase 6 implemented / Phase 7 media execution evidence implemented  
定位：定义并记录已实现的 Importer、Cooker、DDC、binary `.astrapkg` streaming、hot reload rollback DTO 和 Asset Release Gate 生产接口。本文补足 `asset-pipeline.md` 的实现级边界。

## 1. 目标

- 从 canonical source 到 cooked package 的数据流可审计、可增量、可复现。
- Importer、Cooker、DDC 和 PackageReader 可由 provider 扩展，但不能绕过 diagnostics、review、license 和 release gate。
- Package 可脱离 Editor 运行；Runtime 只读 cooked package、user save 和允许的 runtime cache。
- Hot reload 先验证、准备、frame boundary 切换，失败回滚。

非目标：

- DDC 不是 source of truth。
- AI/MCP 不允许直接写 Cooked、DDC 或 package manifest。
- Legacy foreign asset 默认 mount-only，不能成为 native runtime package 前置。

## 2. Import Contract

Import request：

```yaml
schema: astra.asset.import_request.v1
source_path: Imports/Alice.png
target_asset_id: native:/Characters/Alice/Normal
asset_type: image.sprite
preset: astra.import.sprite.default
origin: user_imported
review_state: accepted
license: project_local
```

Importer descriptor：

```yaml
provider_id: astra.importer.image
contract: IAssetImporter
source_extensions: [".png", ".jpg", ".webp"]
output_asset_types: ["image.sprite", "image.background"]
sidecar_schema: astra.asset.sidecar.v1
diagnostics_prefix: ASTRA_IMPORT_IMAGE
```

准接口：

```cpp
class IAssetImporter {
public:
    virtual ImporterDescriptor Describe() const = 0;
    virtual Result<ImportPreview> Preview(const ImportRequest&, DiagnosticSink&) = 0;
    virtual Result<ImportedAsset> Import(const ImportRequest&, DiagnosticSink&) = 0;
};
```

规则：

- Preview 可读取 source，但不能写 Content。
- Import 成功必须写 sidecar、source copy/mount policy 和 audit record。
- AI draft 必须先进入 Review Queue，accepted 后才可 Import。
- Built-in importers cover image、audio、font、text、filter profile、native `.astra` script source、Lua extension schema packages and timeline-style text descriptors. They are source/sidecar producers; media execution remains out of scope。

## 3. Cook Contract

Cook request：

```yaml
schema: astra.asset.cook_request.v1
asset_id: native:/Characters/Alice/Normal
asset_type: image.sprite
source_hash: "..."
target_platform: windows
release_profile: deterministic
selected_providers:
  astra.image_decode: astra.decode.image.libpng
  astra.renderer2d: astra.renderer2d.bgfx
```

Cook artifact：

```yaml
schema: astra.asset.cook_artifact.v1
artifact_id: cooked:/Characters/Alice/Normal.texture
asset_id: native:/Characters/Alice/Normal
format: astra.texture.rgba8
ddc_key: "..."
payload_hash: "..."
runtime_dependencies: []
```

Cook processor descriptor：

```yaml
provider_id: astra.cook.image.texture
contract: ICookProcessor
input_asset_types: ["image.sprite", "image.background"]
output_formats: ["astra.texture.rgba8", "astra.texture.bc7"]
requires_providers: ["astra.image_decode", "astra.renderer2d"]
packaged_eligible: true
```

规则：

- Cook key includes source hash、processor version、selected provider ids、target platform、release profile。
- Cook may use DDC, but package manifest records cooked artifact hash, not DDC trust.
- Unsupported source or target format is cook-time blocking diagnostic.
- Built-in cook processors write deterministic binary payloads and metadata for image/font/audio/script/filter/timeline assets using existing library evidence paths where available. They do not upload textures, execute glyph atlases, play audio, or run GPU filters。

## 4. DDC And Binary Package

DDC key：

```yaml
schema: astra.asset.ddc_key.v1
asset_id: native:/Backgrounds/Room
source_hash: "..."
processor_id: astra.cook.image.texture
processor_version: 3
platform: windows
profile: deterministic
provider_feature_hash: "..."
```

Package payload ref：

```yaml
schema: astra.asset.package_payload_ref.v1
asset_id: native:/Backgrounds/Room
artifact_id: cooked:/Backgrounds/Room.texture
offset: 1024
size: 4096
hash: "..."
compression: zstd
streaming: chunked
```

Package mount policy：

```yaml
schema: astra.asset.package_mount_policy.v1
mount: package:/
read_only: true
allow_random_access: true
allow_chunked_read: true
foreign_copy_allowed: false
```

规则：

- Package manifest stores selected EngineModuleSlot providers and provider feature hash.
- PackageReader validates manifest hash, payload hash, payload size and compression before returning bytes.
- Runtime package mount is read-only.
- New `.astrapkg` files use the `ASTRAP6\0` binary header, version `1`, embedded canonical JSON manifest, SHA-256 payload table, and zstd-compressed payload bytes.
- `PackageReader` supports random-access reads, chunked reads, text reads, mount summaries and blocking diagnostics for corrupt headers, unsupported encodings/compression, and payload hash mismatch.
- `PackageWriter` is the only production writer; CLI `package` must call `AstraAsset` APIs instead of emitting ad-hoc package JSON.

## 5. Hot Reload

Hot reload stages：

```text
Detect -> Validate -> CookTemp -> PrepareProviderResource -> SwitchAtFrameBoundary -> RetireOldResource
```

Rollback rules：

- Validation failure keeps old resource and emits diagnostic.
- Provider prepare failure keeps old resource and records provider id.
- Switch must be atomic at Runtime frame boundary.
- Save stores logical asset id and presentation state, not provider resource ids.
- Current Phase 6 implementation exposes transaction planning and rollback DTOs. Source/sidecar hash changes can switch at frame boundary after validation; removed or diagnostically invalid assets require rollback and retain the old resource.

## 6. Release Gate

Release Gate blocks:

- missing sidecar or duplicate asset id.
- broken hard dependency.
- registry asset missing cook artifact.
- unsupported cook target format.
- unreviewed AI asset.
- illegal foreign copy.
- unresolved virtual refs.
- DDC hash mismatch.
- package payload hash mismatch.
- selected provider not packaged eligible.
- non-runtime-safe module evidence.

`PackageSmoke` acceptance：

- run from `.astrapkg` with no source Content reads.
- random-access and chunked PackageReader paths work.
- corrupt DDC rebuilds; corrupt package blocks launch.

NativeVN Phase 6 acceptance:

- `validate -> import -> cook -> package -> inspect -> run --headless-smoke -> replay --compare` uses `AstraAsset` APIs for production package evidence.
- Save/replay reports include package manifest hash, package profile and selected provider feature hash.
- Replay mismatch reports localize to frame, record kind, expected/actual hash, nearest event sequence, source object, and package manifest hash.
