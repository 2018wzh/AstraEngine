# Product Open Work

## P0

- Stage 2 的 `astra-package`、`astra-asset`、`astra-cook`、`astra-media` 和 `astra-release` 已落地。后续工作不应绕过这些 contract 写私有 package、media 或 release report 格式。
- Runtime save 已迁移到 `astra-package` 共享 container。后续 save section 扩展应继续使用同一 header、section table、codec 和 footer hash 规则。
- Runtime determinism 修复已落地：snapshot 保存 stable id generator 与完整 EventQueue，Await replay policy、run-to-quiescence transaction、typed component mutation、serialized effect 和 provider-free replay transcript 都有回归测试。后续 provider 不得另建私有 tick/save/replay 管线。
- Release Gate 独立 validator 已实现 Stage 2 package/media/scenario refs、Target manifest、strict scenario runner、Windows product platform evidence 和 Web browser evidence；desktop-release/web-release 缺 platform report 或 required evidence 时阻断。
- Migration 11 的 `S2-HEADLESS-CONTRACT-01` 已 `DONE`，其余 `S2-HEADLESS-*` 为 `IN_PROGRESS`。完整 host、严格 provider binding、物理输入 JSONL、真实 PNG/WAV/image/audio/video decode、统一测试 context/doctest gate、完整音频比较、tolerance approval、review 与 release preflight 已落地；Stage 2 当前只缺 Windows CI、正式具名 artifact review 和真实 Windows/Web preflight link evidence。Linux/macOS Headless 本机 CI 与 portability 已转入 Stage 6，不再阻断 Migration 11。
- Migration 12 已完成文档设计。`S2-UI-BACKEND-01`、`S3-UI-SCRIPT-01`、`S3-UI-EXT-01` 均为 `SPEC_READY`；当前没有 Yakui adapter、UI Blueprint/Controller、`CompiledVnProject`、component ABI 或 Windows/Web UI E3，`SystemUiModel` 固定 hit-test 仍未删除。
- Migration 6 frontend focused implementation 已完成：`logos`/`chumsky`/`rowan`/`text-size`、CST-backed Typed AST、固定 semantic passes、Command Registry、token-level source-map hash、formatter 与 language-service adapter 已落地。`S3-SCRIPT-01/02` 仍等待同 package Windows/Web formal Player evidence。
- `S3-FLAGSHIP-DEMO-01` 保持 `IN_PROGRESS`；15–20 分钟三终局、中英双语、中文全配音和正式原创资产见 `Docs/migrations/nativevn-flagship-demo-migration.md`，本轮不实现。
- 当前优先顺序仍在 Stage 3 Windows/Web live player host acceptance 和 TsuiNoSora commercial gate。已有 player route report 只能证明 bundle route slice；`player.full_playable` report validator 已落地，但真实平台 run 仍需要 window/browser host evidence、平台输入 transcript、视觉变化、音频 meter 和同次 route evidence。Linux/macOS/iOS/Android 真实 host smoke 与 player automation 移到 Stage 6。

## P1

- AstraVN module layout、crate split 和 facade-only `astra-vn` 输出已落地。后续插件发布工作还需补外部 dylib 分发、签名和跨版本协商，不再修改 gameplay provider 的 RuntimeWorld/FFI lifecycle 边界。
- `.astra` story frontend 当前已到 `CompiledStory` IR；Migration 12 将直接迁到 `compile_astra_project`/`CompiledVnProject` 和独立 UI AST，完成后删除 `compile_astra_sources` 双轨与旧 package/target reader。
- AstraVN presentation model、standard command library 和 system UI profile 的剩余 migration/localization/replay UI 深化。
- YAML 产品 scenario runner 已删除。平台无关 Runtime/Player/full-flow 测试统一使用 `HeadlessTestContext` 与序列化物理输入；旧 `--headless` 只返回稳定迁移 diagnostic。
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
