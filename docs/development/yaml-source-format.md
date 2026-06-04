# YAML 源格式

## 1. 当前范围

Phase 1 当前只实际使用了一种 YAML 源格式：plugin descriptor。

对应实现位于：

- [PluginDescriptor.cpp](/E:/Documents/AstraEngine/Engine/Runtime/ModuleRuntime/Private/PluginDescriptor.cpp)
- [Phase1ExampleModule.plugin.yaml](/E:/Documents/AstraEngine/Engine/Plugins/Examples/Phase1ExampleModule/Phase1ExampleModule.plugin.yaml)

## 2. 当前 descriptor 结构

最小示例：

```yaml
id: astra.plugin.phase1_example
display_name: Phase1 Example Module
version: 0.1.0
astra_api: ">=0.2.0 <0.3.0"
dependencies: []
modules:
  - id: phase1_example.runtime
    type: runtime
    entrypoint: Phase1ExampleModule.dll
    load_phase: runtime_startup
    capabilities:
      - service_extension
      - property_type_provider
    permissions: {}
    platforms:
      - windows-x64
```

## 3. 当前字段

顶层字段：

- `id`
- `display_name`
- `version`
- `astra_api`
- `dependencies`
- `modules`

`modules[]` 字段：

- `id`
- `type`
- `entrypoint`
- `load_phase`
- `capabilities`
- `permissions`
- `platforms`

## 4. 当前限制

当前 YAML 还不覆盖：

- project descriptor
- asset sidecar
- scene / actor source
- script source
- filter graph source
- compatibility profile

这些属于后续阶段的文本源格式，不应在现阶段文档里假装已经稳定。
