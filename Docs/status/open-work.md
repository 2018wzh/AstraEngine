# Product Open Work

## P0

- Stage 2 的 `astra-package`、`astra-asset`、`astra-cook`、`astra-media` 和 `astra-release` 已落地。后续工作不应绕过这些 contract 写私有 package、media 或 release report 格式。
- Runtime save 已迁移到 `astra-package` 共享 container。后续 save section 扩展应继续使用同一 header、section table、codec 和 footer hash 规则。
- Release Gate 独立 validator 已实现 Stage 2 package/media/scenario refs、Target manifest、strict scenario runner、Windows product platform evidence 和 Web browser evidence；desktop-release/web-release 缺 platform report 或 required evidence 时阻断。
- AstraVN script frontend 标准化已重开 `S3-SCRIPT-01` 和 `S3-SCRIPT-02`。当前 line parser/compiler 仍是可运行 baseline，但 v1 目标需要 Lexer、TokenStream、Lossless CST、Typed AST、Semantic Passes、Command Registry、token-level source map、formatter/LSP adapter 和 release conformance。
- 当前优先顺序仍在 Stage 3 Windows/Web live player host acceptance 和 TsuiNoSora commercial gate。已有 player route report 只能证明 bundle route slice；`player.full_playable` report validator 已落地，但真实平台 run 仍需要 window/browser host evidence、平台输入 transcript、视觉变化、音频 meter 和同次 route evidence。Linux/macOS/iOS/Android 真实 host smoke 与 player automation 移到 Stage 6。

## P1

- AstraVN module layout 与 crate split，固定 facade-only `astra-vn` 的 `rlib`/`dylib` 输出形态和 Rust ABI 版本承诺。
- `.astra` compiler frontend 到 CompiledStory IR；保留 `compile_astra_sources`，逐步迁到 lossless CST、typed AST、semantic passes、command registry 和 token-level source map。
- AstraVN presentation model、standard command library 和 system UI profile 的剩余 migration/localization/replay UI 深化。
- headless YAML scenario runner 已存在；release report writer 已实现 package validate 基线，后续需要 VN full playthrough domain。
- `astra-media` 已实现 headless capture、cosmic-text layout contract、AudioGraph meter、FilterGraph validator、DecodeProvider policy、public media fixture integrity、Windows WMF MP3/MP4 decode 和 wasm-only WebCodecs token provider。wgpu/FFmpeg 仍通过 explicit feature gate 接入。
- Linux/macOS desktop host completion 已移到 Stage 6：补 windowed smoke、platform decode、audio、save store、IME/gamepad 和 release gate evidence。
- Qt/QML Editor shell、PIE bridge、Plugin Manager 和 extension diagnostics。

## P2

- Runtime Director committed output、AI provider profiles、Pack/VFS-backed ONNX ModelBundle、runtime memory、Trusted session audit、MCP Context Pack、command allowlist。
- AstraEMU Manager RuntimeWorld bridge、`LegacyRuntimeProvider` facade、auto probe、Trusted Luau、文本翻译、FilterGraph preset 和 Artemis full-flow。
- AstraRPG Stage 7：shared `astra-policy`、`AstraRpgRuntimeProvider`、RPG core/policy、AI Town 20 NPC gate、`rpg.trpg` profile、CP2020 local-private adapter 和 `rpg.*` release gate。
- AstraRPG Stage 8：`rpg.net.*` Server/Client protocol、seat sync、transcript sync、redacted network audit 和 provider-free network replay。
- iOS/Android 真实 SDK probe、launcher、platform decode、storage/package source、resume 和 no-JIT gate 已移到 Stage 6。

完整状态表见 [implementation-plan](implementation-plan.md)。
