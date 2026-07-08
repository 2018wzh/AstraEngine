# Data, Save, Package Contract

Astra 源数据 text-first，运行时数据 binary-first。YAML 适合人写，二进制容器适合发布和加载。

## Source Descriptors

项目、插件、资产 sidecar 和测试 scenario 使用 YAML：

```yaml
schema: astra.project.v1
id: com.example.nativevn
runtime: astra-vn
engine_modules:
  renderer2d: astra.renderer.wgpu
  text_layout: astra.text.cosmic
  audio: astra.audio.platform
platforms: [windows, linux, macos, ios, android, web]
targets:
  - id: nativevn-game
    kind: game
    crate: astra-vn
    default_profile: desktop-release
    platforms: [windows, linux, macos, ios, android, web]
    packaged: true
```

每个 YAML schema 必须有 Rust 类型、schema version、migrator 和验证命令。

AI ModelBundle source descriptor 也是 YAML，但只作为 cook 输入。Cook 后，Shipping Runtime 只能通过 package/VFS section 读取模型资源：

```yaml
schema: astra.ai_model_bundle.v1
id: com.example.model.local_director
provider: astra-ai-onnx
distribution: bundled
pipeline: llm
platforms: [windows, linux, macos, ios, android, web]
```

descriptor 可以引用项目内相对模型资源、tokenizer、runtime recipe 和 custom op sidecar；不得引用本地绝对路径。Cook 产物必须写入 `ai.model_bundle_manifest` 和对应 package/VFS content entry，不通过 project-level `package_sections` 携带模型 payload。

## Binary Container

Save 和 package 共用自描述容器结构：

```text
AstraContainerHeader
SectionTable[]
SectionPayload[]
FooterHash
```

Section payload 默认使用 `postcard` + serde。大型媒体 payload 可以使用 `Raw` 或 `Zstd` section codec；section table 必须记录 codec、hash、stored hash、offset、length、decoded length 和 migration policy。

容器 ABI 在 [Package And Save](../implementation/package-save.md) 中锁定：little-endian、8 byte alignment、header magic、section table、schema id、codec、hash、optional encryption descriptor、migration policy 和 footer hash。Encryption descriptor 只描述 provider 能力，不提供 DRM 或访问控制绕过方案。

## Save

Save 必须包含 Runtime state、Actor/Component、StateMachine、Blackboard、Director、AwaitToken、script snapshot、VN backlog、AudioGraph state、FilterGraph state、committed AI output、plugin opaque sections 和 migration manifest。

AI Runtime 生成的文本、图像和语音结果是 save 数据，不是 package 数据。流式 chunk 通过 `ai.generated_artifact.*` extra section 固化；manifest 记录 model fingerprint、provider profile、validator result、content type、hash、codec 和可选 encryption。正式 replay 只读 save payload，不重跑 provider。

## Package

Package 必须包含 cooked assets、compiled `.astra` IR、Luau policy bundle、policy lock/vendor cache、schema registry、provider policy、module fingerprint、target manifest、release report summary、test scenario references 和 platform eligibility。Runtime 不依赖源 YAML 启动。

ONNX ModelBundle package section 复用同一个 `AstraContainerHeader + SectionTable[] + SectionPayload[] + FooterHash` 容器。`ai.model_bundle_manifest` 保存模型族、pipeline、license/provenance、fine-tune provenance、redistribution、voice authorization、profile budget、platform targets、VFS mount id、section refs、EP policy 和 runtime fingerprint。模型权重、external data、tokenizer、sampler、scheduler、vocoder、reduced ONNX Runtime、Web runtime adapter 和 custom op sidecar 都作为普通 package content section 存放，可用 `Raw`、`Zstd` 和 `EncryptionDescriptor`。

Bundled、on-demand 和 external 分发只改变 package source，不改变读取接口。Provider 通过 Package reader 和 VFS mount 获取 section ref；Release Gate 校验 mount、hash、codec、encryption、runtime vendor cache 和平台 profile，不允许 Shipping provider 读取 loose file 或绝对路径。

## Migration

每个 schema 使用显式 migrator：

```rust
pub trait SchemaMigrator {
    fn from_version(&self) -> SchemaVersion;
    fn to_version(&self) -> SchemaVersion;
    fn migrate(&self, bytes: &[u8]) -> Result<Vec<u8>, MigrationError>;
}
```

Release Gate 验证 `minimum_supported_version -> current_version` 的迁移链完整。
