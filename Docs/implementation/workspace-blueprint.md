# Workspace Blueprint

AstraEngine v1 采用 UE 风格顶层代码分区和 Rust workspace。顶层目录按 `Engine/`、`Editor/`、`AstraEMU/`、`Examples/`、`Docs/`、`Tools/` 组织；Rust 内部仍按小 crate 开发。系列仓库可以拆分实现，但 public contract、schema 和测试命令以本蓝图为准。

## Layout

| 路径 | 状态 | 职责 |
| --- | --- | --- |
| `Engine/Source/Runtime/` | Stage 1/2 implemented, Stage 3 in progress, Stage 7 planned | EngineCore runtime crate：`astra-core`、`astra-runtime`、`astra-engine` Rust dylib facade、`astra-plugin-abi`、`astra-plugin`；Stage 2 `astra-media-core` 提供轻量 media contract，`astra-asset` 已成为 Provider URI VFS contract owner；Stage 3 `astra-player-core` automation report contract 已开始落地；Stage 7 planned `astra-policy` 承载通用 Luau policy runtime |
| `Engine/Source/Modules/` | Stage 3 implemented for AstraVN split, Stage 7 planned for AstraRPG | 产品垂直模块 crate。AstraVN 已位于 `Engine/Source/Modules/AstraVN/`，其中 `astra-vn` 只作为 facade，具体实现拆到 `astra-vn-script`、`astra-vn-core`、`astra-vn-policy`、`astra-vn-presentation`、`astra-vn-commands`、`astra-vn-system`、`astra-vn-save`、`astra-vn-package`、`astra-vn-plugin`、`astra-vn-editor` 和 `astra-vn-runtime-provider`；AstraRPG planned 路径为 `Engine/Source/Modules/AstraRPG/`，TRPG 作为 `astra-rpg-trpg` 子 crate 存在 |
| `Engine/Source/Platform/` | Stage 1/2 implemented, Headless/UI backend planned | Target 与 Platform crate：`astra-target`、`astra-platform`、六个平台 host crate；Migration 11 planned `astra-platform-headless` 只供测试使用，Migration 12 planned Scene2D/Mesh2D UI consumer |
| `Engine/Fixtures/PublicDomainMedia/` | Stage 2 implemented | CC0 public media fixture：`flower.mp4`、`flower.webm`、`t-rex-roar.mp3` 和 manifest，用于真实 decode/browser media evidence |
| `Engine/Source/Developer/` | Stage 1 implemented, Stage 3 in progress | 开发期工具 crate：`astra-property`、`astra-property-derive`、`astra-test`、共享 host 日志与 fatal ring `astra-observability`；Stage 3 已接入 VN scenario slice 和 VN release gate |
| `Engine/Source/Programs/` | Stage 1 implemented, Stage 3 in progress | CLI 和独立程序：`astra-cli` 提供 cook/package/test/report；`astra-player` 提供 live automation report 校验入口；Windows `astra-crash-reporter` 提供进程外 minidump helper；Stage 3 已接入 NativeVN sample cook/package |
| `Engine/Plugins/Fixtures/` | Stage 1/3 implemented | 测试插件 fixture，覆盖真实 load/unload；`headless-presentation-provider` 覆盖 Stage 1 presentation/action provider，`vn-extension-provider` 覆盖 Stage 3 VN extension provider slots |
| `Engine/Plugins/Providers/` | Stage 2 implemented, Stage 4 reopened | 通用 provider 插件由 Stage 1/2 registry 和 gate 管理；VFS backend provider 统一走 `vfs_provider` slot，NativeVN runtime provider 位于 AstraVN module，第三方 gameplay runtime provider、OpenAI、Ollama、ComfyUI 和 ONNX 仍是后续 provider |
| `Editor/Source/` | Stage 4 not implemented | Qt/QML editor bridge 和应用入口 |
| `AstraEMU/Source/` | Stage 5 reopened, not implemented | Manager、`AstraEmuRuntimeProvider`、RuntimeWorld bridge、EmulatorCore 状态机映射、legacy pack VFS、auto probe、Trusted Luau、文本管线、FilterGraph preset、family plugin 和 LegacyRuntimeProvider facade |
| `Examples/` | Stage 3 in progress | 产品样例和发布样例；`Examples/NativeVN` 是可提交 commercial baseline sample，`Examples/TsuiNoSora/Docs/Title.png`、`Game.png` 作为 TsuiNoSora 视觉参考证据 |
| `Tools/TsuiNoSora/` | Stage 3 in progress | 本地合法数据的 inventory、visual reference report 和 Asset analysis helper；输出脱敏 report，不提交商业 payload |

crate 内部按 module 拆分，`lib.rs` 只做 facade。单文件接近 400-600 行时优先拆分，避免把 world、scheduler、save、loader 或 runner 堆在一个文件里。

## Toolchain

```toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
targets = [
  "x86_64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-ios",
  "aarch64-linux-android",
  "wasm32-unknown-unknown",
]
```

## Crate Graph

| Crate | Stage | 公开职责 | 主要依赖 | 禁止依赖 |
| --- | --- | --- | --- | --- |
| `astra-core` | 1 | id、diagnostic、schema、hash、source span | `serde`, `schemars`, `thiserror`, `miette`, `uuid`, `sha2`, `blake3` | Runtime、Editor、Luau、GPU/audio |
| `astra-runtime` | 1 | World、Actor/Component、StateMachine、EventBus、AwaitToken、Save/Replay facade | `astra-core`, `postcard`, `indexmap` | Editor UI、Luau host、renderer backend |
| `astra-engine` | 1 | Rust ABI dynamic library facade，re-export EngineCore public API | `astra-core`, `astra-runtime`, `astra-package`, `astra-plugin` | second runtime, C ABI promise |
| `astra-plugin-abi` | 1/3 | portable LoadPhase、extension/dependency/runtime provider DTO；native `ffi` feature 追加 `abi_stable` RootModule 与 FFI entrypoints | `serde`, `schemars`；native `ffi` 使用 `abi_stable` | host loader、RuntimeWorld |
| `astra-plugin` | 1/2/3 | descriptor gate、registry、module slot、真实 loader、action adapter、extension registry backend、single-slot `vfs_provider` registry 和 gameplay runtime provider selection backend | `astra-core`, `astra-plugin-abi`, `astra-runtime`, `libloading`, `semver` | Editor widget、native handles in public API |
| `headless-presentation-provider` | 1 | public plugin fixture，真实 cdylib 注册 presentation/action provider | `astra-plugin-abi`, `astra-runtime`, `postcard` | product runtime state、commercial payload |
| `vn-extension-provider` | 3 | public plugin fixture，真实 cdylib 注册 VN policy/command/presentation/editor metadata/release check provider slots | `astra-plugin-abi`, `abi_stable` | Luau VM handle、renderer/audio native handle、commercial payload |
| `astra-target` | 1 | Editor/Game/Program Target schema、manifest validation、Target report | `astra-core`, `serde`, `schemars` | platform native handle、Editor UI |
| `astra-property` | 1 | PropertySystem metadata、Inspector/save glue、derive re-export | `astra-core`, `astra-property-derive` | hidden inheritance、global object system |
| `astra-property-derive` | 1 | derive macro | `syn`, `quote`, `proc-macro2` | runtime state、global object system |
| `astra-test` | 1/3 | YAML scenario runner、report compare、VN player route assertions | `astra-core`, `astra-runtime`, `astra-vn`, `serde_yaml` | platform UI |
| `astra-cli` | 1/3 | `astra cook`、`astra package build`、`astra package bundle`、`astra package validate`、`astra test run`、`astra report explain` | `astra-test`, `astra-package`, `astra-release`, `astra-vn`, `astra-cook`, `astra-player-core`, `clap` | platform UI、商业 payload |
| `astra-observability` | cross-stage | host observability config、`astra.log_event.v1`、reload、限额 file/ring、critical mirror、fatal bundle、Windows crash client | `tracing`, `tracing-subscriber`, `file-rotate`, `serde` | deterministic state、report authority、remote telemetry |
| `astra-crash-reporter` | cross-stage Windows | 独立 helper、shared request handshake、filtered `MiniDumpWriteDump`、hash/size manifest 和 retention | `astra-observability`, Windows DbgHelp API | remote upload、full-memory dump、非 Windows native crash |
| `astra-asset` | 2 | AssetId、sidecar schema、`VfsUri`、prefix/layer/entry/whiteout contract、`AssetCatalog`、host-only `LocalMountRootSet` 和 bounded local reader | `astra-core`, `serde`, `schemars` | decoder native handle, package semantics, serialized local root |
| `astra-cook` | 2 | Importer/CookProcessor、DDC key、cook audit | `astra-asset`, `astra-package`, `image` | Editor UI |
| `astra-package` | 2 | binary package/save container、section reader/writer、Zstd codec、crypto descriptor、plugin registry sections、`asset.vfs_manifest`、`asset.catalog` 和 package-backed VFS source | `astra-core`, `postcard`, `serde`, `zstd` | story/runtime semantics, legacy pack reader |
| `astra-media-core` | 2/3 | Renderer2D/FilterGraph serde contract、headless CPU frame、deterministic filter executor，可被 AstraVN presentation/runtime provider crate 依赖 | `astra-core`, `serde`, `schemars` | decode/text/audio/native provider |
| `astra-media` | 2 | TextLayout/Decode/AudioGraph providers，并 re-export `astra-media-core` 的 Renderer2D/FilterGraph traits and headless providers | `astra-media-core`, `astra-core`, `image`, `symphonia`, `cosmic-text`, optional `ffmpeg-next` via `ffmpeg-vcpkg` | VN state |
| `astra-release` | 2/3 | Release Gate validators、plugin/package/VFS/provider/VN/player checks、report writer | `astra-core`, `astra-asset`, `astra-package`, `astra-media`, `astra-vn`, `astra-player-core` | Editor-only state |
| `astra-platform` | 2 | PlatformHost trait、PlatformCapabilityReport、SDK 状态和 token DTO | `astra-core`, `serde`, `schemars` | Runtime state、Actor 指针、native handle ownership |
| `astra-platform-windows` | 2 | Windows capability probe 和 host adapter | `astra-platform` | non-Windows private API leaking into shared crate |
| `astra-platform-linux` | 2 | Linux capability probe 和 host adapter | `astra-platform` | distro-specific state leaking into shared crate |
| `astra-platform-macos` | 2 | macOS capability probe 和 host adapter | `astra-platform` | AppKit object crossing public API |
| `astra-platform-ios` | 2 | iOS capability probe 和 host adapter | `astra-platform` | JIT requirement、UIKit object crossing public API |
| `astra-platform-android` | 2 | Android capability probe 和 host adapter | `astra-platform` | JVM object crossing public API |
| `astra-platform-web` | 2 | canvas/DOM、WebGPU、WebAudio、WebCodecs、OPFS 与 fetch/File host adapter | `astra-platform`、`astra-platform-common` | browser object crossing public API |
| `astra-platform-headless` | 2 planned | `publish = false` 的完整测试 host，执行 surface/audio/decode/save/package/input/artifact lifecycle | `astra-platform`、`astra-platform-common`、显式绑定的 Media providers | shipping target、cooked platform profile、AstraPlayer dependency |
| `astra-player-web` | 2/3 | 独立 WASM Player；读取 config、package 与 cooked platform profile，通过 shared Player host executor 驱动 AstraVN runtime 和 canvas/WebGPU surface | `astra-package`、`astra-platform-web`、`astra-player-core`、`astra-player-vn` | JavaScript route/runtime implementation；正式 Chrome evidence |
| `astra-player-vn` | 3 in progress | AstraVN `VnStepOutput` 到平台无关 Player host command 的适配器；Windows/Web 共用 runtime input、deterministic CPU frame 和 present command | `astra-player-core`、`astra-vn-core`、`astra-media-core` | platform handle、AstraRPG rules、正式平台 evidence |
| `astra-vn-script` | 3 implemented | `.astra` source、parser/compiler、`CompiledStory`、source map、debug symbol、route graph、story/variable/command manifest | `astra-core`, `serde`, `schemars`, `miette` | RuntimeWorld, Luau, Editor UI |
| `astra-vn-presentation` | 3 implemented | StageModel、layer/camera/video/audio/timeline/fallback、headless presentation execution、presentation provider manifest | `astra-core`, `astra-media-core`, `serde`, `schemars` | decode/text/audio native provider, VN policy VM |
| `astra-vn-core` | 3 implemented | `VnRuntime`、command cursor、runtime state、choice、call/return、backlog、read-state、voice replay、wait state、system state、replay UI | `astra-core`, `astra-runtime`, `astra-vn-script`, `astra-vn-presentation`, `postcard`, `serde` | Luau VM handle, Editor UI, package writer |
| `astra-vn-policy` | 3 implemented | Luau sandbox、policy state、mutation/query/trace、policy bundle manifest、source cache、`standard_policy.luau` | `astra-core`, `astra-vn-core`, `mlua`, `serde`, `schemars` | fs/network/system capability, native handle |
| `astra-vn-commands` | 3 implemented | standard command library、command schema、usage validation、command manifest | `astra-core`, `astra-vn-script`, `astra-vn-presentation`, `serde`, `schemars` | runtime provider selection |
| `astra-vn-system` | 3 implemented | system stories、save/config/backlog/gallery/replay/route chart/localization profile | `astra-core`, `astra-vn-script`, `astra-vn-core`, `serde`, `schemars` | Editor UI |
| `astra-vn-save` | 3 implemented | 局部/reference VN state blob、hash、migration glue；产品 save authority 已归并到完整 `runtime.world` snapshot | `astra-core`, `astra-vn-core`, `astra-vn-policy`, `astra-package`, `postcard` | Luau function/thread/userdata |
| `astra-vn-package` | 3 implemented | `vn.*` package section plans、profile manifest、commercial baseline、advanced presentation manifest、package evidence | `astra-core`, `astra-package`, `astra-vn-script`, `astra-vn-policy`, `astra-vn-presentation`, `astra-vn-system` | release gate ownership |
| `astra-vn-plugin` | 3 implemented | VN extension points、extension manifest、provider slot ids | `astra-core`, `astra-plugin-abi`, `astra-vn-package`, `serde`, `schemars` | plugin loader internals |
| `astra-vn-editor` | 3 implemented, Stage 4 bridge planned | Graph/Timeline authoring metadata、source round-trip metadata、NativeVN `RuntimeEditorMetadata` | `astra-core`, `astra-vn-script`, `astra-vn-presentation`, `astra-vn-plugin`, `serde`, `schemars` | Qt/QML widget, Editor native handle |
| `astra-vn-runtime-provider` | 3 implemented | `NativeVnRuntimeProvider` session-owned RuntimeWorld、`astra.vn.step` action、prepare/probe/open/step/save/restore/package/release/editor metadata 和真实 FFI instance/session lifecycle | `astra-runtime`, `astra-target`, `astra-plugin-abi`, `astra-vn-core`, `astra-vn-policy`, `astra-vn-save`, `astra-vn-package`, `astra-vn-editor` | RPG/EMU base class, RuntimeWorld/Actor pointer across ABI |
| `astra-vn` | 3 implemented | Rust ABI dylib facade 和兼容 re-export，不承载 parser/runtime/policy/package 实现 | AstraVN 子 crate | platform-native handles, second runtime, C ABI promise, heavy decode/text provider dependencies in facade, RPG/EMU base class |
| `astra-player-core` | 3 in progress | automation contracts、平台无关 `PlayerHostCommand`、ordered executor 和 `PlatformCommandSink`；logical resource id 不暴露 native handle | `astra-core`, `astra-platform`, `serde`, `schemars`, `serde_json` | gameplay-provider 私有类型、local path 或 native handle serialization |
| `astra-player` | 3 in progress | Windows bundled AstraVN runtime host，以及 `sendinput.*`/Web `cdp.*` automation transcript/report 校验入口 | `astra-player-core`, `astra-player-vn`, `astra-platform-windows`, `serde_json`, `serde_yaml` | package cooking、commercial payload、route-scenario/DOM/JS command bypass |
| `astra-headless` | 2 planned | Developer-only JSONL file/stdio runner，写 Headless artifact manifest、run report、review/preflight inputs | `astra-platform-headless`、test harness | shipping Player、release package、平台 profile |
| `astra-policy` | 7 planned | 通用 Luau sandbox、policy value、snapshot、command/query/trace record、manifest lock/source-cache 和 diagnostic helpers | `astra-core`, `mlua`, `serde`, `schemars` | product-specific VN/RPG/EMU state, filesystem/network/system capability, native handle |
| `astra-rpg-core` | 7 planned | `RpgSession`、`RpgIntent`、`RpgEffect`、`RpgSheet`、`CommittedAgentOutput`、ruleset/save/package DTO | `astra-core`, `astra-runtime`, `serde`, `schemars`, `postcard` | Editor UI, live AI provider secret, direct RuntimeWorld pointer |
| `astra-rpg-policy` | 7 planned | `astra.rpg.*` Luau host API、rule policy manifest、capability、intent/effect bridge | `astra-policy`, `astra-rpg-core` | direct world mutation, filesystem/network/system capability |
| `astra-rpg-trpg` | 7 planned | `rpg.trpg` ruleset/profile、character sheet schema、dice/check/ruling/transcript/seat/privacy DTO | `astra-rpg-core`, `astra-policy` | standalone runtime provider, top-level `trpg.*` package namespace, copyrighted rule payload |
| `astra-rpg-runtime-provider` | 7 planned | `AstraRpgRuntimeProvider` lifecycle、package sections、release checks、editor metadata | `astra-runtime`, `astra-plugin-abi`, `astra-target`, `astra-rpg-core`, `astra-rpg-policy`, `astra-rpg-trpg` | RuntimeWorld pointer ownership, AI provider secret in replay |
| `astra-rpg-editor` | 7 planned | Map/Quest/Party/Inventory/Encounter/Behavior Graph/TRPG sheet metadata | `astra-core`, `astra-rpg-core`, `astra-rpg-trpg` | Qt/QML widget, packaged runtime state |
| `astra-rpg` | 7 planned | AstraRPG facade 和兼容 re-export，不承载 runtime/provider 实现 | AstraRPG 子 crate | second runtime, independent TRPG provider, native handles |
| `astra-rpg-net` | 8 planned | `rpg.net.*` protocol DTO、handshake、seat sync、action transcript、network replay envelope | `astra-core`, `astra-rpg-core`, `astra-rpg-trpg` | unredacted transcript, provider secret, local root |
| `astra-rpg-server` | 8 planned | Server-side session authority、seat assignment、action transcript append、redacted audit | `astra-rpg-net`, `astra-rpg-runtime-provider` | direct platform UI, private GM payload leak |
| `astra-rpg-client` | 8 planned | Client-side handshake、seat permission、local transcript view、reconnect cursor | `astra-rpg-net` | authoritative world mutation, private transcript leak |
| `astra-editor-bridge` | 4 | Qt/Rust bridge、PIE/debug API、runtime provider editor metadata handoff | `astra-runtime`, `astra-vn-editor`, `astra-vn-runtime-provider`, `astra-release` | packaged runtime dependency, direct product runtime internals |
| `astra-ai` | 4 reopened | Runtime Director、provider profile、Asset VFS-backed ONNX ModelBundle、memory ledger、Editor AI audit | `astra-core`, `astra-runtime`, `astra-package`, `astra-asset` | provider secret in replay, loose shipping sidecar |
| `astra-mcp` | 4 | MCP tool descriptor、Context Pack、permission、audit、command allowlist | `astra-core`, `astra-plugin` | Editor widget in Runtime tools |
| `astra-emu-manager` | 5 reopened | Manager app、`AstraEmuRuntimeProvider`、RuntimeWorld bridge、plugin enablement、auto probe、Trusted Luau host、text pipeline、filter preset binding、report | `astra-core`, `astra-runtime`, `astra-plugin`, `astra-media`, `astra-asset` | family VM internals |
| `astra-emu-family-api` | 5 reopened | LegacyFamilyPlugin descriptor, LegacyRuntimeProvider session/effect/snapshot DTO, family scheduler/context trace, legacy pack VFS DTO | `astra-core`, `astra-runtime`, `astra-package`, `astra-asset` | Manager UI |
| `astra-emu-artemis` | 5 reopened | Artemis v1 family plugin | `astra-emu-family-api`, `astra-plugin` | EngineCore private state |

## Binaries

| Binary | Crate | Command |
| --- | --- | --- |
| `astra` | `astra-cli` | `astra cook`, `astra package build`, `astra package bundle`, `astra package validate`, `astra test run`, `astra report explain` |
| `astra-headless` | planned `astra-headless` | `astra-headless run`, `astra-headless serve --stdio`；Migration 11 实现前不存在 |
| `astra-player` | `astra-player` | `astra-player --script <automation.json> --transcript <transcript.json>` |
| `astra-crash-reporter` | `AstraCrashReporter` | 由 bundled Player 启动；`--self-test` 仅用于 bundle gate |
| `astra target` | `astra-cli` | `astra target list`, `astra target validate` |
| `astra platform` | `astra-cli` | `astra platform probe` |
| `astra-editor` | `astra-editor-app` | Qt/QML creator editor |
| `astra-emu-manager` | `astra-emu-manager` | legacy VN manager that creates RuntimeWorld, selects family plugin and owns overlay/filter/text pipelines |
| `astra-emu-family-*` | family crate | in-process legacy family plugin |

## Feature Rules

- Default features must build desktop core without Editor, AI provider or EMU family.
- Game target must bind a gameplay runtime provider explicitly; `NativeVnRuntimeProvider`、`AstraEmuRuntimeProvider` 和后续 `AstraRpgRuntimeProvider` 不能靠插件加载顺序抢占。
- `rpg.trpg` is an AstraRPG profile crate; do not create a top-level `AstraTRPG` module, standalone TRPG runtime provider or top-level `trpg.*` package section namespace.
- `luau` enables AstraVN and AstraEMU policy host integration. Legacy family adapters may parse historical script names inside their private core.
- `wgpu` is default Renderer2D provider. Platform decode features are profile-specific.
- 当前 `headless` 只提供分散的 Runtime/Media/Test 局部能力。Migration 11 完成后，所有平台无关 Runtime 测试必须创建 `HeadlessTestContext`，完整后端仍不得进入 shipping dependency graph。
- Target manifest is required for package validation. Missing SDK reports block native platform completion, but schema and CLI checks still run on ordinary CI.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p astra-cli -- test run scenarios/native_smoke.yaml --headless --report target/reports/stage1.yaml
cargo run -p astra-cli -- report explain target/reports/stage1.yaml
```

Expected report: `astra.scenario_report.v1` with matching state/event/presentation hash.

Migration 11 完成后，旧 `astra test run --headless` 将改为明确迁移错误；planned `astra-headless`、`astra.headless_run_report.v1` 和 checkout-bound workspace tests 才是统一入口。该变更尚未实现，当前命令仍按既有行为运行。
