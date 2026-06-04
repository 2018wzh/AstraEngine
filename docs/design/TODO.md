# TODO

状态：Target Architecture Task List

## 1. Phase 0：文档与工程基线

- [x] P0 统一所有设计文档为模块化 2D 引擎目标态。
- [x] P0 重写 ADR，直接表达当前目标决策。
- [x] P0 保持 Runtime 不依赖 Editor 的工程边界。
- [x] P1 准备项目基础环境

## 2. Phase 1：Core / Platform / Module / Property

- [x] P0 Core foundation：logging、error、assert、config、time、path。
- [x] P0 Platform：window、input、filesystem、timer、thread、dynamic library。
- [x] P0 ModuleManager：descriptor、dependency、version、load phase、lifecycle。
- [x] P0 `AstraModule` C ABI：opaque handle、diagnostics、host api。
- [x] P0 ServiceRegistry：服务获取和权限边界。
- [x] P0 ExtensionRegistry：扩展点注册和重复诊断。
- [x] P1 PropertySystem：TypeId、PropertyId、schema、Inspector metadata、AI flags。

## 3. Phase 2：Scene / Runtime

- [ ] P0 ActorWorld、ActorId、ActorTypeId、ActorHandle。
- [ ] P0 ComponentDescriptor、Component serialization、ActorSnapshot。
- [ ] P0 EventBus：RuntimeEvent、PresentationEvent、ScriptEvent。
- [ ] P0 StateMachineRuntime、StateMachineComponent。
- [ ] P1 BlackboardComponent、ControlPolicyComponent、Director。
- [ ] P1 Save/Load/Replay：Actor、Component、StateMachine、Blackboard、event queue。
- [ ] P2 局部 ECS / Data-Oriented system pack API。

## 4. Phase 3：Asset / Media / FilterGraph

- [ ] P0 AssetId、ResourceHandle、VFS directory mount。
- [ ] P0 Asset sidecar schema、AssetRegistry generation。
- [ ] P1 Renderer2D、Text、Audio 最小实现。
- [ ] P1 RenderGraph 和 Presentation extraction。
- [ ] P1 FilterGraph、FilterProfile、layer-aware targets。
- [ ] P2 Hot reload：asset、script、filter profile。

## 5. Phase 4：ScriptRuntimeHost / AstraVN

- [ ] P0 `IScriptRuntime`、ScriptRuntimeHost、ScriptEventBridge。
- [ ] P0 Astra Native Script parser 和 runtime。
- [ ] P1 VN DSL：bg、show、say、choice、timeline、filter。
- [ ] P1 AstraVN 预定义 Actor：Scene、StoryDirector、DialogueSystem、ChoiceSystem、AudioSystem、FilterSystem、Character、Camera。
- [ ] P1 VN 预定义状态机：Dialogue、Choice、CharacterPresentation、Background、Audio、Timeline、FilterProfile。
- [ ] P2 LuaRuntime。

## 6. Phase 5：Editor

- [ ] P1 Project Browser、Asset Browser、Scene Tree。
- [ ] P1 Script Editor、Graph Editor、Timeline Editor。
- [ ] P1 Inspector：Actor、Component、StateMachine。
- [ ] P1 PIE 和 Runtime Debugger。
- [ ] P2 FilterGraph Editor。
- [ ] P2 StateMachine visual debugger。

## 7. Phase 6：AI

- [ ] P1 Boundary Manager、Context Builder、Diff/Patch、Review Queue。
- [ ] P1 IAIProvider、Provider permissions、SecretProvider。
- [ ] P1 Agent Audit：Operation Log、Generation Audit Log。
- [ ] P2 Runtime AIIntent、IntentValidator、Director integration。
- [ ] P2 Runtime MCP Host 和 runtime-safe tools。

## 8. Phase 7：Legacy VN Emulator / Modernization

- [ ] P1 CompatRuntimeProvider、ForeignProjectProbe、PackageReader。
- [ ] P1 Legacy VM state、opcode/timeline adapter、API Mapper。
- [ ] P1 Save extension state。
- [ ] P2 Compatibility Inspector。
- [ ] P2 Modernization Profile、font replacement、UI overlay、FilterProfile。
- [ ] P2 Mock legacy runtime fixture。
- [ ] P3 BGI、Kirikiri、Ren'Py、NScripter prototype。

## 9. 横向测试

- [ ] Unit：Core、PropertySystem、AssetId、EventBus、StateMachineRuntime。
- [ ] Integration：ActorWorld、ScriptRuntimeHost、FilterGraph、Save/Replay。
- [ ] Headless：VN demo、AI Intent、legacy runtime playback。
- [ ] Release Gate：schema、plugin、AI、compat、package policy。
