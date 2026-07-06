# Product Open Work

## P0

- Stage 2 的 `Engine/Source/Runtime/astra-package`、`Engine/Source/Runtime/astra-asset`、`Engine/Source/Developer/astra-cook`、`Engine/Source/Runtime/astra-media` 和 `Engine/Source/Developer/astra-release` 尚未实现。
- Stage 1 的 runtime save container 已在 `astra-runtime` 内实现；Stage 2 需要抽出或复用到 package container。
- Release Gate 独立 validator 尚未实现；当前 Stage 1 只有 `astra.scenario_report.v1` 和 plugin descriptor/load tests。

## P1

- `.astra` parser/compiler 到 CompiledStory IR。
- headless YAML scenario runner 和 release report writer。
- wgpu Renderer2D provider、headless capture provider、cosmic-text provider。
- Qt/QML Editor shell 与 PIE bridge。

## P2

- Runtime AI committed output、Trusted session audit、MCP tool descriptor。
- AstraEMU Manager RuntimeWorld bridge、family plugin API 和 Artemis engine-native full-flow。
- 移动/Web host module capability report。
