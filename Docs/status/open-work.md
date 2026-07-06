# Product Open Work

## P0

- Stage 2 的 `Engine/Source/Runtime/astra-package`、`Engine/Source/Runtime/astra-asset`、`Engine/Source/Developer/astra-cook`、`Engine/Source/Runtime/astra-media` 和 `Engine/Source/Developer/astra-release` 尚未实现。
- Stage 1 的 runtime save container 已在 `astra-runtime` 内实现；Stage 2 需要抽出或复用到 package container。
- Release Gate 独立 validator 尚未实现；当前 Stage 1 只有 `astra.scenario_report.v1` 和 plugin descriptor/load tests。
- 当前优先顺序是 package container、asset/cook、release report、media provider。Stage 3/4/5 不应先写绕过这些 contract 的私有格式。

## P1

- `.astra` parser/compiler 到 CompiledStory IR。
- AstraVN presentation model、standard command library、system UI profile 和 advanced presentation opt-in scenario。
- headless YAML scenario runner 和 release report writer。
- wgpu Renderer2D provider、headless capture provider、cosmic-text provider。
- Qt/QML Editor shell、PIE bridge、Plugin Manager 和 extension diagnostics。

## P2

- Runtime AI committed output、Trusted session audit、MCP tool descriptor。
- AstraEMU Manager RuntimeWorld bridge、`LegacyRuntimeProvider` facade 和 Artemis full-flow。
- 移动/Web host module capability report。

完整状态表见 [implementation-plan](implementation-plan.md)。
