# Editor UX 与 AI 协作设计

状态：Target UX Spec

## 1. 目标

编辑器 UI 面向可定制 2D/VN 创作，不做营销式界面。第一屏是实际工作台：项目树、场景/Actor 视图、脚本/Graph/Timeline、Inspector、Runtime Debugger 和 AI Review Queue。

## 2. 主布局

```text
Top Bar
  Project / Build / Run / Package / Release Gate

Left Dock
  Asset Browser / Asset Editor / Scene Tree / Actor Tree

Center
  Scene View / Script Editor / Graph Editor / Timeline / FilterGraph

Right Dock
  Inspector / Component Properties / StateMachine Debug

Bottom Dock
  Output Log / Event Log / Runtime Debugger / Review Queue / Compatibility Diagnostics (expansion)
```

## 2.1 UX Contracts

- Docking：所有 panel 支持停靠、浮动、隐藏、恢复默认布局；布局保存为 editor layout preset。
- Command Palette：所有菜单命令、MCP 工具入口、Editor action 都可搜索执行。
- Context Menu：Asset、Actor、Component、Script、Graph node、Timeline track 都有上下文操作。
- Asset Picker：支持搜索、tag、type filter、recent、missing reference repair。
- Property Details Panel：使用 PropertySystem metadata，支持 category、tooltip、validation、diff、review flag。
- Undo/Redo：所有 canonical source 修改进入 transaction log；preview/runtime-only 操作不写源数据。
- Dirty State：区分 source dirty、preview dirty、runtime PIE dirty、generated draft dirty。
- Preview State：asset/script/filter/timeline preview 必须可取消、可回滚、可与 PIE 状态分离。
- Accessibility：所有核心命令可通过键盘触发；错误状态提供文本 diagnostics。

## 3. Editor Copilot MCP

Editor Copilot MCP 面向全流程创作辅助，不直接覆盖正式内容：

- 读取上下文：角色、设定、当前脚本、Graph、Timeline、资产 metadata。
- 输出 inline suggestion、patch proposal、localization draft、diagnostics explanation。
- 支持 chat-driven patch、batch refactor、schema fix、release gate explanation。
- 所有正式内容变更进入 Review Queue，除非是显式 trusted MCP direct write。
- 每次生成写 Generation Audit；每次工具副作用写 Operation Log。

## 4. Editor Content Generation MCP

Editor Content Generation MCP 面向创作期内容生成、修改和增强，而不是独立聊天：

- 参数面板：asset type、prompt、reference assets、target folder、license/profile、output constraints。
- 生成预览：文本、图像、音频、语音、视频、动画、filter profile 或 UI overlay draft，支持 variants 对比。
- 修改/增强：upscale、denoise、style transfer、voice cleanup、localization rewrite、timeline suggestion。
- Sidecar draft：显示稳定 ID 候选、tags、origin、license、review 状态和 audit 链接。
- Review Queue：导入、编辑、拒绝、重新生成；默认不直接写正式 Content。
- 审计状态：显示 provider、session、prompt/context hash、output hash 和 trusted write operation。

## 5. Runtime Debugger

Runtime Debugger 必须能查看：

- Actor tree、Component data、StateMachine 当前状态。
- EventBus 最近事件、queued events、ControlPolicy lock。
- ScriptRuntimeHost 当前 runtime、entry、call stack 或 VM 状态。
- FilterGraph active profile。
- AI committed intent。
- Legacy compat VM state；仅在 Legacy expansion track 模块启用时显示。

## 6. AstraEmu Boundary

旧 VN 运行、TextCapture、翻译、增强和 Runtime Inspector 属于独立 AstraEmu Toolkit。
AstraEditor 不提供旧 VN 兼容检查面板，也不把旧 VN 内容纳入 NativeVN 制作界面。

## 7. Qt 实现映射

第一阶段可用 Qt 实现 dockable editor shell。Scene View 通过引擎渲染输出嵌入；脚本、Graph、Timeline、FilterGraph、Inspector 均调用 Runtime public DTO，不访问内部 native object。


