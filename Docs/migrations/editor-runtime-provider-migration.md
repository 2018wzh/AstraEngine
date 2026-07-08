# Editor Runtime Provider Migration

本计划只迁移已经存在的 Editor 设计、手册和状态口径，使 AstraEditor 按 runtime-provider-aware shell 建设。当前 `Editor/Source` 尚不存在，因此本页不把 AstraEditor UI 或 bridge 代码写成可搬迁对象。

## 现有实现入口

- `Docs/implementation/editor-workflow.md`：Project Wizard、Plugin Manager、PIE、Debugger、Release Gate 和 AI Review Queue 的工作流设计。
- `Docs/modules/editor.md`：AstraEditor 模块边界和 v1 面板。
- `Docs/manual/creator-manual.md`：创作者从项目创建到发布的手册入口。
- `Docs/status/stages/stage-4-editor-ai-mcp.md`：Stage 4 Editor、AI/MCP 和 target 状态。
- `Docs/status/stages/stage-test-matrix.md`、`Docs/status/coverage-matrix.md`、`Docs/status/implementation-plan.md`：Editor runtime provider switching 的阶段、测试和 coverage 口径。

## 目标设计

Editor shell 固定承担窗口、layout、Content Browser、Plugin Manager、PIE lifecycle、Debugger host、Package/Release Gate 和 AI Review Queue。玩法相关创作面由 selected `ProductRuntimeProvider` 的 `RuntimeEditorMetadata` 决定：

- NativeVN：`.astra` Script Editor、VN Graph、Timeline、System UI、Luau policy、VN package/release checks。
- AstraEMU：planned case profile/probe、legacy pack VFS browser、family trace、text/translation overlay、Trusted Luau 和 FilterGraph preset。
- AstraRPG：planned Map、Quest、Battle/Party/Inventory、Behavior Graph 和 RPG Inspector。

`RuntimeEditorMetadata` 只能携带 template、surface id、schema、command id、source ref、VFS locator、diagnostic 和 release check id。Editor 不能接收插件 UI widget、`RuntimeWorld` 指针、legacy VM object、native renderer/audio handle、本地 root 或商业 payload。

## 分步迁移

1. 调整 Project Wizard 口径。
   Project Wizard 先列出 `ProductRuntimeProvider`，再根据 `RuntimeEditorMetadata.project_templates` 创建项目。NativeVN 是当前可用 provider；AstraEMU/AstraRPG 只显示 planned/unavailable diagnostic。
2. 调整 Project Settings 和 Plugin Manager。
   Project manifest 必须显式保存 runtime provider binding；Plugin Manager 显示 provider extension point、dependency graph、permission、packaged eligibility 和 conflict diagnostic，但不靠加载顺序选择玩法 runtime。
3. 调整 PIE 和 Debugger。
   PIE launch request 必须包含 Game target id、runtime provider id、profile id 和 package/VFS mount evidence；Debugger 从 selected provider 读取 source ref、state machine trace、await token 和 provider diagnostic。
4. 调整 Release Gate panel。
   Release Gate 面板按 provider metadata 展示 `runtime_provider.native_vn`、未来 `runtime_provider.astra_emu`、VFS mount 和 provider-specific checks，并保持 report 脱敏。
5. 调整手册与状态页。
   Creator manual、Stage 4、coverage matrix 和 stage test matrix 必须说明 Editor 可以切换 provider，但当前只把 NativeVN 写成可用实现。

## 验收命令

```bash
python Tools/check_docs.py
```

新增 Editor bridge 代码后再补：

```bash
cargo test -p astra-editor-bridge runtime_provider_switch
cargo test -p astra-editor-bridge project_wizard
cargo test -p astra-editor-bridge release_gate_panel
```

## 不得修改项

- 不把 AstraVN 写成 Editor 唯一玩法基类。
- 不把 AstraEMU/AstraRPG surface 写成已实现 UI。
- 不让 Editor 直接持有 runtime object、插件 widget、native handle、本地 root 或商业 payload。
- 不绕过 package/save/release gate；Editor 只能展示和发起检查，不能替代 runtime provider 或 release gate。
