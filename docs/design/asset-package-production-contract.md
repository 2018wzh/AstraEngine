# Asset / Package Production Contract

状态：Production contract draft / not yet fully implemented  
定位：定义 Importer、Cooker、DDC、package streaming、hot reload rollback 和 Asset Release Gate 的生产接口。本文补足 `asset-pipeline.md` 的实现级边界。

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
  astra.renderer2d: astra.renderer2d.sdl_gpu
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

## 4. DDC And Package

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

## 6. Release Gate

Release Gate blocks:

- missing sidecar or duplicate asset id.
- broken hard dependency.
- unsupported cook target format.
- unreviewed AI asset.
- illegal foreign copy.
- package payload hash mismatch.
- selected provider not packaged eligible.

`PackageSmoke` acceptance：

- run from `.astrapkg` with no source Content reads.
- random-access and chunked PackageReader paths work.
- corrupt DDC rebuilds; corrupt package blocks launch.

