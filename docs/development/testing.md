# 测试

## 1. 当前测试范围

当前只维护 Phase 1 测试面：

- Core 基础行为
- Platform 基础服务初始化
- ModuleRuntime 注册、发现、加载
- PropertySystem schema 生成
- 示例模块 smoke

## 2. 运行测试

```powershell
ctest --test-dir build -C Debug --output-on-failure
```

当前默认测试目标：

- `Astra_Phase1Tests`

## 3. 当前用例

`Astra_Phase1Tests` 覆盖：

- `DiagnosticSink`
- `Path` UTF-8 roundtrip
- YAML 配置加载
- Platform service 初始化
- `ServiceRegistry` capability / permission 限制
- `ExtensionRegistry` 重复注册与 kind 过滤
- plugin descriptor 解析
- `PropertyRegistry` JSON schema 生成
- `ModuleManager` 加载 `Phase1ExampleModule`

## 4. Smoke 验证

除 `ctest` 外，还应直接运行：

```powershell
.\build\Bin\AstraPhase1Smoke.exe
```

期望输出至少包含：

- `Loaded modules: 1`
- `phase1_example.service_extension`
- `phase1_example.property_type_provider`

## 5. 测试边界

当前还没有：

- ActorWorld integration tests
- StateMachineRuntime tests
- Save/Load/Replay tests
- ScriptRuntimeHost tests
- FilterGraph tests
- VN runtime tests
- Compat runtime tests

这些测试属于 Phase 2 以后。
