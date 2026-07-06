# Implementation Plan Status

本页只记录当前代码完成度和下一步实施顺序。设计规格放在 `Docs/product`、`Docs/contracts`、`Docs/modules` 和 `Docs/implementation`；本页不把已规划内容写成已实现能力。

## 状态规则

| Status | 含义 | 允许标记条件 |
| --- | --- | --- |
| `DONE` | 已实现并有可运行证据 | 目标路径存在，关联测试通过，状态页写明证据命令 |
| `IN_PROGRESS` | 正在实现 | 已有代码落地，但关联 gate 未全部通过 |
| `SPEC_READY` | 设计可开工 | 文档、schema、gate 和测试映射已写清，代码未落地 |
| `RESEARCH_READY` | 研究资料可用于实现 | 仅限 AstraEMU family 研究和 probe 工具，不代表 runtime 已实现 |
| `NOT_STARTED` | 尚未开工 | 目标 crate、scenario 或 gate 仍不存在 |

以后实现某个工作项时，同步改本页、对应 `stage-*.md`、[stage-test-matrix.md](stages/stage-test-matrix.md) 和 [coverage-matrix.md](coverage-matrix.md)。没有测试或 release report 证据，不标 `DONE`。

## 当前代码快照

| Area | Code status | Evidence |
| --- | --- | --- |
| Stage 1 EngineCore | `DONE` | `cargo test --workspace` 通过；覆盖 core、runtime、plugin、property、Target manifest、headless scenario |
| Stage 2 Media + Package | `DONE` | Asset/Cook/Package、headless media、release report、Target manifest、strict scenario runner、flat StateMachine、Await/Fence、Windows product host evidence 和 Web browser evidence 已落地。完成边界只覆盖 Windows/Web；Linux/macOS/iOS/Android 移到 Stage 6。已验证 `cargo test -p astra-runtime --test state_machine_tick`、`cargo test -p astra-runtime --test await_token`、`cargo test -p astra-test --test native_smoke`、`cargo test -p astra-platform-windows`、`cargo test -p astra-platform-web`、`cargo test -p astra-platform-web --target wasm32-unknown-unknown --no-run`、`wasm-pack test --headless --chrome Engine/Source/Platform/astra-platform-web`、`cargo test -p astra-media decode_provider`、`cargo test -p astra-release release_report` 和 `cargo test -p astra-cli --test target_platform`；Stage2 `DONE` 绑定 `Engine/Fixtures/PublicDomainMedia/manifest.json`、`decode.wmf.audio`、`decode.wmf.video_first_frame`、`renderer.wgpu_surface`、`save.known_folder_rw`、`decode.browser_media`、`save.web_storage_rw` 和 `package.web_source_read` evidence |
| Stage 3 AstraVN | `SPEC_READY` | `.astra`、Luau、presentation、standard commands、system UI、advanced sample 和 TsuiNoSora 商业验证门禁已写入文档；`astra-vn` crate、TsuiNoSora 转换器、Asset analysis gate、player 自动化和 release gate 实现尚不存在 |
| Stage 4 Editor + AI/MCP | `SPEC_READY` | Editor workflow、Plugin Manager、AI provider profile、Runtime Director、memory、MCP context 和 AI/MCP gate 已写入文档；`Editor/Source` 尚不存在 |
| Stage 5 AstraEMU | `SPEC_READY` | `Docs/contracts/astraemu-ipc.md`、`Docs/implementation/astraemu-legacy-runtime-framework.md` 和 `Docs/emu` 已写清 `LegacyRuntimeProvider` facade、auto probe、Trusted Luau、文本翻译和 FilterGraph preset；`AstraEMU/Source` 尚不存在 |
| Stage 6 Platform Completion | `SPEC_READY` | Linux、macOS、iOS 和 Android 的真实 host smoke、launcher、platform decode、save/resume 和 release evidence 已移到 [stage-6-platform-completion](stages/stage-6-platform-completion.md)；当前只保留 capability crate 和 planned gate |

## Stage 1 完成项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S1-BOOT-01` | `DONE` | workspace、toolchain、CI 配置存在 |
| `S1-CORE-01` | `DONE` | `cargo test -p astra-core core_types` |
| `S1-RUNTIME-01` | `DONE` | `cargo test -p astra-runtime world_actor` |
| `S1-RUNTIME-02` | `DONE` | `cargo test -p astra-runtime state_machine_tick` |
| `S1-RUNTIME-04` | `DONE` | `cargo test -p astra-runtime state_machine_tick` and `cargo test -p astra-runtime delayed_event` |
| `S1-RUNTIME-03` | `DONE` | `cargo test -p astra-runtime await_token` |
| `S1-SAVE-01` | `DONE` | `cargo test -p astra-runtime save_replay` |
| `S1-DYLIB-01` | `DONE` | `cargo test -p astra-engine dylib_facade` |
| `S1-PLUGIN-01` | `DONE` | `cargo test -p astra-plugin descriptor_gate` and `cargo test -p astra-plugin load_unload` |
| `S1-PLUGIN-02` | `DONE` | `cargo test -p astra-plugin ffi_action_provider` |
| `S1-PLUGIN-03` | `DONE` | `cargo test -p astra-plugin extension_registry` |
| `S1-PROP-01` | `DONE` | `cargo test -p astra-property --test property_metadata` and `cargo test -p astra-property --test expand_smoke` |
| `S1-TEST-01` | `DONE` | `cargo test -p astra-test native_smoke` |
| `S1-OBS-01` | `DONE` | `cargo test -p astra-cli --test logging` |
| `S1-TARGET-01` | `DONE` | `cargo test -p astra-target` and `cargo test -p astra-cli --test target_platform` |

## Stage 2 平台修复项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S2-PLATFORM-01` | `DONE` | `cargo test -p astra-platform`；共享 report schema、SDK 分层和 required smoke validation 已落地 |
| `S2-WINDOWS-HOST-01` | `DONE` | `cargo test -p astra-platform-windows`；Windows probe 输出 hidden window、`renderer.wgpu_surface`、`audio.wasapi`、`save.known_folder_rw`、XInput 和 SDK 状态 |
| `S2-WINDOWS-WMF-01` | `DONE` | `cargo test -p astra-media decode_provider`；public media manifest 校验固定 sha256，WMF `decode.wmf.audio` 输出 MP3 bounded PCM CPU buffer，`decode.wmf.video_first_frame` 输出 MP4 BGRA 首帧，invalid video 返回 `ASTRA_WMF_DECODE` |
| `S2-WINDOWS-GATE-01` | `DONE` | `cargo test -p astra-release release_report` and `cargo test -p astra-cli --test target_platform`；缺 required Windows smoke 会阻断 release check |
| `S2-PLUGIN-GATE-01` | `DONE` | `cargo test -p astra-package package_roundtrip` and `cargo test -p astra-release release_report`；package 写入 `plugin.extension_registry` 和 `plugin.dependency_graph`，release gate 阻断 unresolved conflict、missing binding 和 unresolved dependency |
| `S2-RUNTIME-FSM-01` | `DONE` | `cargo test -p astra-runtime --test state_machine_tick`；flat FSM validation、terminal/completed、priority conflict diagnostic 和 Always tick trigger 已落地 |
| `S2-RUNTIME-AWAIT-01` | `DONE` | `cargo test -p astra-runtime --test await_token`；Await timeout materialization、unknown/duplicate result diagnostic 和 pending token serialization 已落地 |
| `S2-SCENARIO-GATE-01` | `DONE` | `cargo test -p astra-test --test native_smoke`；unknown VN action/assertion blocked、declared package missing blocked、Stage 1 native smoke pass |
| `S2-WEB-HOST-01` | `DONE` | `cargo test -p astra-platform-web`、`cargo test -p astra-platform-web --target wasm32-unknown-unknown --no-run`、`wasm-pack test --headless --chrome Engine/Source/Platform/astra-platform-web`、`cargo test -p astra-media decode_provider --target wasm32-unknown-unknown --no-run`、`cargo test -p astra-release release_report` 和 `cargo test -p astra-cli --test target_platform`；Web required smoke 覆盖 `renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 和 `package.web_source_read` |

## Stage 6 平台完成项

| Work ID | Status | Evidence |
| --- | --- | --- |
| `S6-LINUX-HOST-01` | `SPEC_READY` | 真实 Linux host smoke、platform decode、audio、save store、IME/gamepad 和 release evidence 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-MACOS-HOST-01` | `SPEC_READY` | 真实 macOS AppKit/winit、Metal/wgpu、CoreAudio、AVFoundation、App Support 和 notarization capability 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-IOS-HOST-01` | `SPEC_READY` | Swift/SwiftUI launcher、Metal surface、safe area/touch、AVAudio/AVFoundation、app container save 和 no-JIT Luau gate 见 [Stage 6](stages/stage-6-platform-completion.md) |
| `S6-ANDROID-HOST-01` | `SPEC_READY` | Kotlin/Java launcher、Vulkan/wgpu surface、AAudio、MediaCodec、SAF/package import、activity resume 和 no-JIT Luau gate 见 [Stage 6](stages/stage-6-platform-completion.md) |

## 下一步实施顺序

| Order | Work | Status | Why now |
| --- | --- | --- | --- |
| 1 | `S2-PACKAGE-01` package container | `DONE` | `astra-package` 提供共享 container、Zstd codec、crypto descriptor、bounded reader；Runtime save 已迁移 |
| 2 | `S2-ASSET-01` + `S2-ASSET-02` asset/import/cook | `DONE` | `astra-asset` 和 `astra-cook` 提供 sidecar、registry、metadata import、DDC key 和 cook audit |
| 3 | `S2-GATE-01` release report | `DONE` | `astra-release` 和 `astra package validate` 输出 `astra.release_report.v1`；release profile 缺 `compiled.project` cook/project artifact 时阻断 |
| 4 | `S2-MEDIA-01` 到 `S2-MEDIA-05` media providers | `DONE` | `astra-media` 提供 headless renderer、TextLayout、AudioGraph、FilterGraph、DecodeProvider 和 optional native feature gates |
| 5 | `S2-WINDOWS-HOST-01` + `S2-WINDOWS-WMF-01` + `S2-WINDOWS-GATE-01` Windows platform repair | `DONE` | Windows host probe、WMF DecodeProvider 和 release gate evidence 已落地 |
| 6 | `S3-DYLIB-01` AstraVN Rust dylib target | `SPEC_READY` | 先固定 `astra-vn` 的 `rlib`/`dylib` 输出形态和 Rust ABI 承诺，避免后续 VN API 反向污染 EngineCore |
| 7 | `S3-SCRIPT-01` + `S3-SCRIPT-02` `.astra` parser/compiler | `SPEC_READY` | AstraVN Core 和 Editor visual model 的前置 |
| 8 | `S3-GAME-TARGET-01` NativeVN Game target | `SPEC_READY` | Game target 需要随 AstraVN sample 和 full playthrough 一起落地 |
| 9 | `S3-TSUI-GATE-01` TsuiNoSora commercial validation gate | `SPEC_READY` | 随 Stage 3 统一实现本地完整转换包、Asset analysis gate、视觉参考报告、全路线 player 自动化和补丁式挂载 target；当前只存在文档计划，不作为代码证据 |
| 10 | `S4-PLUGIN-01` Plugin Manager UI | `SPEC_READY` | Editor 只显示和修改 Stage 1/2 产出的 enablement、dependency graph 和 extension diagnostics |
| 11 | `S4-AI-01` 到 `S4-GATE-01` AI/MCP closure | `SPEC_READY` | Runtime Director、provider profile、memory、Context Pack、AI Control 和 release gate 需要一起落地 |
| 12 | `S4-EDITOR-TARGET-01` AstraEditor Editor target | `SPEC_READY` | Editor target 需要 Qt/QML shell 和 PIE bridge |
| 13 | `S5-MANAGER-01` + `S5-PROGRAM-TARGET-01` + `S5-FAMILY-01` + `S5-AUTOPROBE-01` + `S5-SCRIPT-01` + `S5-TEXT-01` + `S5-FILTER-01` | `SPEC_READY` | AstraEMU Manager 作为 Program target 驱动 RuntimeWorld、family plugin，并复用 Stage 4 provider、MCP 和 memory |
| 14 | `S6-LINUX-HOST-01` + `S6-MACOS-HOST-01` + `S6-IOS-HOST-01` + `S6-ANDROID-HOST-01` platform completion | `SPEC_READY` | Windows/Web 之外的平台完成从 Stage 2 移出，等 VN/Core/Editor 发布路径稳定后集中接入真实 SDK evidence |

## 验证命令

```bash
python Tools/check_docs.py
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected output: docs check reports checked markdown files；fmt/clippy/workspace tests pass；diff check has no whitespace errors。
