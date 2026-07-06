# Product Open Work

## P0

- Stage 2 的 `astra-package`、`astra-asset`、`astra-cook`、`astra-media` 和 `astra-release` 已落地。后续工作不应绕过这些 contract 写私有 package、media 或 release report 格式。
- Runtime save 已迁移到 `astra-package` 共享 container。后续 save section 扩展应继续使用同一 header、section table、codec 和 footer hash 规则。
- Release Gate 独立 validator 已实现 Stage 2 package/media/scenario refs、Target manifest、strict scenario runner、Windows product platform evidence 和 Web browser evidence；desktop-release/web-release 缺 platform report 或 required evidence 时阻断。
- 当前优先顺序转入 Stage 3 `astra-vn` Rust dylib facade、`.astra` parser/compiler、AstraVN Core 和 presentation model。Linux/macOS/iOS/Android 真实 host smoke 移到 Stage 6。

## P1

- `astra-vn` Rust dylib facade，固定 `rlib`/`dylib` 输出形态和 Rust ABI 版本承诺。
- `.astra` parser/compiler 到 CompiledStory IR。
- AstraVN presentation model、standard command library、system UI profile 和 advanced presentation opt-in scenario。
- headless YAML scenario runner 已存在；release report writer 已实现 package validate 基线，后续需要 VN full playthrough domain。
- `astra-media` 已实现 headless capture、cosmic-text layout contract、AudioGraph meter、FilterGraph validator、DecodeProvider policy、public media fixture integrity、Windows WMF MP3/MP4 decode 和 wasm-only WebCodecs token provider。wgpu/FFmpeg 仍通过 explicit feature gate 接入。
- Linux/macOS desktop host completion 已移到 Stage 6：补 windowed smoke、platform decode、audio、save store、IME/gamepad 和 release gate evidence。
- Qt/QML Editor shell、PIE bridge、Plugin Manager 和 extension diagnostics。

## P2

- Runtime Director committed output、AI provider profiles、runtime memory、Trusted session audit、MCP Context Pack、command allowlist。
- AstraEMU Manager RuntimeWorld bridge、`LegacyRuntimeProvider` facade、auto probe、Trusted Luau、文本翻译、FilterGraph preset 和 Artemis full-flow。
- iOS/Android 真实 SDK probe、launcher、platform decode、storage/package source、resume 和 no-JIT gate 已移到 Stage 6。

完整状态表见 [implementation-plan](implementation-plan.md)。
