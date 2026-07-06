# Workspace Blueprint

AstraEngine v1 采用 UE 风格顶层代码分区和 Rust workspace。顶层目录按 `Engine/`、`Editor/`、`AstraEMU/`、`Examples/`、`Docs/`、`Tools/` 组织；Rust 内部仍按小 crate 开发。系列仓库可以拆分实现，但 public contract、schema 和测试命令以本蓝图为准。

## Layout

| 路径 | 状态 | 职责 |
| --- | --- | --- |
| `Engine/Source/Runtime/` | Stage 1 implemented, Stage 3 planned | EngineCore runtime crate：`astra-core`、`astra-runtime`、`astra-engine` Rust dylib facade、`astra-plugin-abi`、`astra-plugin`；Stage 3 planned `astra-vn` Rust dylib facade |
| `Engine/Source/Platform/` | Stage 1/2 implemented | Target 与 Platform crate：`astra-target`、`astra-platform`、六个平台 host crate |
| `Engine/Fixtures/PublicDomainMedia/` | Stage 2 implemented | CC0 public media fixture：`flower.mp4`、`flower.webm`、`t-rex-roar.mp3` 和 manifest，用于真实 decode/browser media evidence |
| `Engine/Source/Developer/` | Stage 1 implemented | 开发期工具 crate：`astra-property`、`astra-property-derive`、`astra-test` |
| `Engine/Source/Programs/` | Stage 1 implemented | CLI 和独立程序：`astra-cli` 提供 `astra test run`、`astra report explain` |
| `Engine/Plugins/Fixtures/` | Stage 1 implemented | 测试插件 fixture，覆盖真实 load/unload |
| `Engine/Plugins/Providers/` | Stage 2/4 not implemented | 通用 provider 插件由 Stage 1/2 registry 和 gate 管理；OpenAI、Ollama、ComfyUI 仍是 Stage 4 AI provider |
| `Editor/Source/` | Stage 4 not implemented | Qt/QML editor bridge 和应用入口 |
| `AstraEMU/Source/` | Stage 5 not implemented | Manager、RuntimeWorld bridge、auto probe、Trusted Luau、文本管线、FilterGraph preset、family plugin 和 LegacyRuntimeProvider facade |
| `Examples/` | Stage 2+ not implemented | 产品样例和发布样例 |

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
| `astra-plugin-abi` | 1 | `abi_stable` RootModule、FFI structs、LoadPhase、extension/dependency DTO | `abi_stable`, `serde`, `schemars` | host loader、RuntimeWorld |
| `astra-plugin` | 1 | descriptor gate、registry、module slot、真实 loader、action adapter、extension registry backend | `astra-core`, `astra-plugin-abi`, `astra-runtime`, `libloading`, `semver` | Editor widget、native handles in public API |
| `astra-target` | 1 | Editor/Game/Program Target schema、manifest validation、Target report | `astra-core`, `serde`, `schemars` | platform native handle、Editor UI |
| `astra-property` | 1 | PropertySystem metadata、Inspector/save glue、derive re-export | `astra-core`, `astra-property-derive` | hidden inheritance、global object system |
| `astra-property-derive` | 1 | derive macro | `syn`, `quote`, `proc-macro2` | runtime state、global object system |
| `astra-test` | 1 | YAML scenario runner、report compare | `astra-core`, `astra-runtime`, `serde_yaml` | platform UI |
| `astra-cli` | 1 | `astra test run`、`astra report explain` | `astra-test`, `clap` | package build、release validation |
| `astra-asset` | 2 | AssetId、VFS、sidecar schema、AssetRegistry | `astra-core`, `serde`, `schemars` | decoder native handle |
| `astra-cook` | 2 | Importer/CookProcessor、DDC key、cook audit | `astra-asset`, `astra-package`, `image` | Editor UI |
| `astra-package` | 2 | binary package/save container、section reader/writer、Zstd codec、crypto descriptor、plugin registry sections | `astra-core`, `postcard`, `serde`, `zstd` | story/runtime semantics |
| `astra-media` | 2 | Renderer2D/TextLayout/Decode/FilterGraph/AudioGraph traits and headless providers | `astra-core`, `image`, `symphonia`, `cosmic-text`, optional `wgpu`/`ffmpeg-next` via `ffmpeg-vcpkg`/`kira` | VN state |
| `astra-release` | 2 | Release Gate validators、plugin/package/provider checks、report writer | `astra-core`, `astra-package`, `astra-media` | Editor-only state |
| `astra-platform` | 2 | PlatformHost trait、PlatformCapabilityReport、SDK 状态和 token DTO | `astra-core`, `serde`, `schemars` | Runtime state、Actor 指针、native handle ownership |
| `astra-platform-windows` | 2 | Windows capability probe 和 host adapter | `astra-platform` | non-Windows private API leaking into shared crate |
| `astra-platform-linux` | 2 | Linux capability probe 和 host adapter | `astra-platform` | distro-specific state leaking into shared crate |
| `astra-platform-macos` | 2 | macOS capability probe 和 host adapter | `astra-platform` | AppKit object crossing public API |
| `astra-platform-ios` | 2 | iOS capability probe 和 host adapter | `astra-platform` | JIT requirement、UIKit object crossing public API |
| `astra-platform-android` | 2 | Android capability probe 和 host adapter | `astra-platform` | JVM object crossing public API |
| `astra-platform-web` | 2 | Web browser probe、required smoke 和 host adapter | `astra-platform` | browser object crossing public API |
| `astra-vn` | 3 | `.astra` parser/compiler、VN Core、Luau policy host、presentation/system UI、VN plugin extension bindings、Rust ABI dylib facade | `astra-core`, `astra-runtime`, `astra-package`, `astra-media`, `astra-plugin-abi`, `pest`, `mlua` | platform-native handles, second runtime, C ABI promise |
| `astra-editor-bridge` | 4 | Qt/Rust bridge、PIE/debug API | `astra-runtime`, `astra-vn`, `astra-release` | packaged runtime dependency |
| `astra-ai` | 4 | Runtime Director、provider profile、memory ledger、Editor AI audit | `astra-core`, `astra-runtime`, `astra-package` | provider secret in replay |
| `astra-mcp` | 4 | MCP tool descriptor、Context Pack、permission、audit、command allowlist | `astra-core`, `astra-plugin` | Editor widget in Runtime tools |
| `astra-emu-manager` | 5 | Manager app, RuntimeWorld bridge, plugin enablement, auto probe, Trusted Luau host, text pipeline, filter preset binding, report | `astra-core`, `astra-runtime`, `astra-plugin`, `astra-media` | family VM internals |
| `astra-emu-family-api` | 5 | LegacyFamilyPlugin descriptor, LegacyRuntimeProvider session/effect/snapshot DTO | `astra-core`, `astra-runtime`, `astra-package` | Manager UI |
| `astra-emu-artemis` | 5 | Artemis v1 family plugin | `astra-emu-family-api`, `astra-plugin` | EngineCore private state |

## Binaries

| Binary | Crate | Command |
| --- | --- | --- |
| `astra` | `astra-cli` | `astra cook`, `astra package build`, `astra package validate`, `astra test run`, `astra report explain` |
| `astra target` | `astra-cli` | `astra target list`, `astra target validate` |
| `astra platform` | `astra-cli` | `astra platform probe` |
| `astra-editor` | `astra-editor-app` | Qt/QML creator editor |
| `astra-emu-manager` | `astra-emu-manager` | legacy VN manager that creates RuntimeWorld, selects family plugin and owns overlay/filter/text pipelines |
| `astra-emu-family-*` | family crate | in-process legacy family plugin |

## Feature Rules

- Default features must build desktop core without Editor, AI provider or EMU family.
- `luau` enables AstraVN and AstraEMU policy host integration. Legacy family adapters may parse historical script names inside their private core.
- `wgpu` is default Renderer2D provider. Platform decode features are profile-specific.
- `headless` must be available for Runtime, Media and Test without window creation.
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
