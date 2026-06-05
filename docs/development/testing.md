# 测试

## 1. 当前测试范围

当前维护 Phase 1 和 Phase 2 测试面：

- Core 基础行为
- Platform 基础服务初始化
- ModuleRuntime 注册、发现、加载
- PropertySystem schema 生成
- 示例模块 smoke
- ActorWorld / Component snapshot
- RuntimeEventBus
- StateMachineRuntime
- Blackboard / ControlPolicy / Director
- Save / Load / Replay
- Local ECS system pack 边界

## 2. 运行测试

```powershell
ctest --test-dir build -C Debug --output-on-failure
```

当前默认测试目标：

- `Astra_Phase1Tests`
- `Astra_Phase2Tests`

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

`Astra_Phase2Tests` 覆盖：

- `ActorWorld` 创建、组件默认值、snapshot/restore
- runtime event queue deterministic sequence
- Director 推进 actor-bound state machine
- WorldSnapshot 保存/恢复 actor、state machine、blackboard、control policy、event queue
- ReplayLog 序列化和逐帧回放
- ControlPolicy locked channel 仲裁
- EnTT-backed `Transform2DSystemPack` 只通过 `ActorId` 和 DTO 同步

## 4. Smoke 验证

除 `ctest` 外，还应直接运行：

```powershell
.\build\Bin\AstraPhase1Smoke.exe
.\build\Bin\AstraPhase2Smoke.exe
```

`AstraPhase1Smoke` 期望输出至少包含：

- `Loaded modules: 1`
- `phase1_example.service_extension`
- `phase1_example.property_type_provider`

`AstraPhase2Smoke` 期望输出至少包含：

- `Phase2 actors: 1`
- `Phase2 state: idle`
- `Phase2 snapshot version: astra.phase2.snapshot.v1`
- `Phase2 replay seed: 7`

## 5. 测试边界

当前还没有：

- ScriptRuntimeHost tests
- FilterGraph tests
- VN runtime tests
- Editor debugger tests
- Compat runtime tests

这些测试属于 Phase 3 以后或对应后续功能阶段。
