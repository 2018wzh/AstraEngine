# Product Open Work

## P0

- Stage 2 的 `astra-package`、`astra-asset`、`astra-cook`、`astra-media` 和 `astra-release` 已落地。后续工作不应绕过这些 contract 写私有 package、media 或 release report 格式。
- Runtime save 已迁移到 `astra-package` 共享 container。后续 save section 扩展应继续使用同一 header、section table、codec 和 footer hash 规则。
- Release Gate 独立 validator 已实现 Stage 2 package/media/scenario refs 基线；Stage 3/4/5 需要继续补 VN、Editor、AI/MCP、platform 和 EMU domain。
- 当前优先顺序转为 Stage 3 `.astra` parser/compiler、AstraVN Core 和 presentation model。

## P1

- `.astra` parser/compiler 到 CompiledStory IR。
- AstraVN presentation model、standard command library、system UI profile 和 advanced presentation opt-in scenario。
- headless YAML scenario runner 已存在；release report writer 已实现 package validate 基线，后续需要 VN full playthrough domain。
- `astra-media` 已实现 headless capture、cosmic-text layout contract、AudioGraph meter、FilterGraph validator 和 DecodeProvider policy。wgpu/FFmpeg 仍通过 explicit feature gate 接入。
- Qt/QML Editor shell、PIE bridge、Plugin Manager 和 extension diagnostics。

## P2

- Runtime AI committed output、Trusted session audit、MCP tool descriptor。
- AstraEMU Manager RuntimeWorld bridge、`LegacyRuntimeProvider` facade、auto probe、Trusted Luau、文本翻译、FilterGraph preset 和 Artemis full-flow。
- 移动/Web host module capability report。

完整状态表见 [implementation-plan](implementation-plan.md)。
