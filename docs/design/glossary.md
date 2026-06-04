# 术语表

## Actor
运行时公开对象模型，拥有 `ActorId`、`ActorTypeId`、Component 集合、生命周期和状态机。

## ActorId
稳定、可存档、可回放的 Actor 身份，不等同于 ECS entity 或 C++ 指针。

## ComponentDescriptor
组件 schema、默认值、Inspector metadata、序列化和 AI 编辑边界描述。

## EventBus
运行时事件分发系统，传递 RuntimeEvent、VNEvent、PresentationEvent、ScriptEvent 和 AIIntentEvent。

## StateMachineRuntime
驱动 Actor-bound 状态机的运行时。状态机可作为 Component 挂载在 Actor 上。

## ControlPolicy
Actor 控制权组件，处理优先级、锁定 channel、打断、排队和拒绝。

## Director
全局叙事仲裁者，管理剧情阶段、Timeline lock、AI 可用范围和 legacy VM 同步约束。

## PresentationCommand
状态机内部输出到底层表现服务的命令，例如创建文本框、播放语音、切换表情、启动滤镜。

## AstraVN / Presentation.VN
视觉小说垂直模块，提供 VN DSL、VN Event、Dialogue、Choice、Character、Background 和预定义状态机。

## ScriptRuntimeHost
管理 Astra Native、Lua、BGI、Kirikiri、Custom 等脚本运行时的宿主。

## FilterGraph
统一后处理和现代化管线，支持 per-layer filter 和 final-screen filter。

## FilterProfile
文本源资产，描述滤镜 pass、目标层、参数和现代化配置。

## AIIntent
AI 运行时输出的结构化意图，只能经 Validator、ControlPolicy 和 Director 审核后执行。

## RuntimeGenerationOrchestrator
运行时生成编排器。它构建 runtime context，调用 Provider，生成 `AIIntent`，并把结果交给 `IntentValidator`、ControlPolicy 和 Director。

## IntentValidator
校验 `AIIntent` 是否满足角色在场、剧情阶段、Canon、权限、分级、Timeline lock 和 ControlPolicy 约束。

## Agent Audit
记录工具副作用和生成来源的审计系统，分 Operation Log 与 Generation Audit Log。

## AgentAudit
Agent Audit 的运行时/模块接口名，用于注册 audit sink、写入 Operation Log 和 Generation Audit Log。

## Legacy VM
旧 VN 引擎脚本或 bytecode 的模拟运行时，例如 BGI VM、Kirikiri runtime。

## API Mapper
把 legacy VM 图像、文本、音频、变量和系统调用映射为 Astra RuntimeEvent 或 PresentationCommand 的适配层。

## Modernization Profile
旧游戏现代化配置，包括字体替换、UI 覆盖、FilterProfile、缩放策略、高清资源覆盖和本地化覆盖。

## ServiceRegistry
模块获取引擎服务的注册表，返回最小 public service interface 或 opaque handle。

## ExtensionRegistry
模块注册扩展能力的注册表，例如 Actor type、StateMachine、ScriptRuntime、Filter、Provider、CompatRuntime 和 Editor panel。

## PropertySystem
轻量类型和属性描述系统，用于 schema、Inspector、MCP 字段编辑、序列化和插件配置。

## Text-First Source
以 YAML + JSON Schema 为 canonical source 的项目格式；二进制资产语义写在 `.asset.yaml` sidecar 中。
