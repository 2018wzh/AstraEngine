# 路线图

## 1. 路线原则

先建立通用 2D 引擎核心，再构建 VN 垂直模块；先确定性运行与存档，再引入 AI 和旧 VN 模拟器。

优先级：

1. Core + Platform + Module + Property。
2. Actor/Component + EventBus + StateMachineRuntime。
3. Media + Asset + FilterGraph。
4. ScriptRuntimeHost + Native VN。
5. Editor。
6. AI collaboration / runtime intent。
7. Legacy VN emulator / modernization plugins。

## 2. Phase 0：文档与工程基线

交付：

- 目标态设计文档和 ADR。
- 顶层 CMake / vcpkg / 目录结构。
- 编码规范。

验收：

- 新开发者能理解 AstraEngine 是模块化 2D 引擎，VN 是第一模块。
- 文档明确 Core 不绑定 VN、Live2D、AI 或旧 VM。

## 3. Phase 1：Core / Platform / Module / Property

实现：

- Core foundation、logging、error、config、time、path。
- Platform window/input/filesystem/timer/thread/dynamic library。
- ModuleManager、ServiceRegistry、ExtensionRegistry、C ABI。
- PropertySystem 和 schema generation。

验收：

- 示例模块可加载、注册服务和扩展、停用卸载。
- ABI 不暴露 C++ ownership、Actor 指针或 native handles。

## 4. Phase 2：Scene / Runtime

实现：

- ActorWorld、ActorId、ActorTypeId、ComponentDescriptor。
- EventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director。
- Save/Load/Replay 基础快照。
- 局部 ECS system pack API。

验收：

- Headless world 可创建 Actor、派发事件、推进状态机、保存恢复。
- 存档不保存 native pointer 或 ECS entity 原始值。

## 5. Phase 3：Asset / Media / FilterGraph

实现：

- AssetId、VFS、AssetRegistry、sidecar。
- Renderer2D、Text、Audio。
- FilterGraph、FilterProfile、layer-aware targets。

验收：

- 可渲染背景、角色、文本和 UI。
- FilterProfile 能应用到 background、character、ui、text、final 层。

## 6. Phase 4：ScriptRuntimeHost 与 AstraVN

实现：

- ScriptRuntimeHost、Astra Native Script、ScriptEventBridge。
- AstraVN DSL、VN Event、DialogueSM、ChoiceSM、CharacterPresentationSM、BackgroundSM、AudioSM。
- 最小 VN demo。

验收：

- 脚本通过事件驱动 Actor 状态机。
- Demo 能显示背景、立绘、对白、选择、音频并保存恢复。

## 7. Phase 5：Editor

实现：

- Project Browser、Asset Browser、Scene/Actor Inspector。
- Script/Graph/Timeline/FilterGraph Editor。
- StateMachine Debugger、Event Log、PIE。

验收：

- Editor 使用同一 Runtime，不走独立预览逻辑。
- 可查看 Actor、Component、StateMachine 和 EventBus 状态。

## 8. Phase 6：AI

实现：

- Boundary Manager、Context Builder、Review Queue、Diff/Patch。
- Provider interface、Agent Audit。
- Runtime AIIntent、IntentValidator、Runtime MCP Host。

验收：

- AI 建议进入 Review Queue。
- Runtime AI 只能提交受控 Intent，并可保存回放。

## 9. Phase 7：Legacy VN Emulator / Modernization

实现：

- CompatRuntimeProvider、PackageReader、Legacy VM、API Mapper。
- Compatibility Inspector。
- Modernization Profile、字体替换、UI 覆盖、FilterProfile。

验收：

- 至少一个 mock legacy runtime 可运行并输出 VN presentation。
- Mount-only 默认不复制外部原始资产。
