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
```

每个 YAML schema 必须有 Rust 类型、schema version、migrator 和验证命令。

## Binary Container

Save 和 package 共用自描述容器结构：

```text
AstraContainerHeader
SectionTable[]
SectionPayload[]
FooterHash
```

Section payload 默认使用 `postcard` + serde。大型媒体 payload 可以用 raw compressed block，但必须在 section table 中记录 codec、hash、offset、length 和 migration policy。

## Save

Save 必须包含 Runtime state、Actor/Component、StateMachine、Blackboard、Director、AwaitToken、script snapshot、VN backlog、AudioGraph state、FilterGraph state、committed AI output、plugin opaque sections 和 migration manifest。

## Package

Package 必须包含 cooked assets、compiled `.astra` IR、Lua policy bundle、policy lock/vendor cache、schema registry、provider policy、module fingerprint、release report summary、test scenario references 和 platform eligibility。Runtime 不依赖源 YAML 启动。

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
