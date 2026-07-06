# Workspace Blueprint

AstraEngine v1 采用 UE 风格顶层代码分区和 Rust workspace。顶层目录按 `Engine/`、`Editor/`、`AstraEMU/`、`Examples/`、`Docs/`、`Tools/` 组织；Rust 内部仍按小 crate 开发。系列仓库可以拆分实现，但 public contract、schema 和测试命令以本蓝图为准。

## Layout

| 路径 | 状态 | 职责 |
| --- | --- | --- |
| `Engine/Source/Runtime/` | Stage 1 implemented | EngineCore runtime crate：`astra-core`、`astra-runtime`、`astra-plugin` |
| `Engine/Source/Developer/` | Stage 1 implemented | 开发期工具 crate：`astra-property`、`astra-property-derive`、`astra-test` |
| `Engine/Source/Programs/` | Stage 1 implemented | CLI 和独立程序：`astra-cli` 提供 `astra test run`、`astra report explain` |
| `Engine/Plugins/Fixtures/` | Stage 1 implemented | 测试插件 fixture，覆盖真实 load/unload |
| `Editor/Source/` | Stage 4 not implemented | Qt/QML editor bridge 和应用入口 |
| `AstraEMU/Source/` | Stage 5 not implemented | Manager、RuntimeWorld bridge、family plugin 和 legacy provider |
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
| `astra-plugin` | 1 | `abi_stable` RootModule、descriptor、registry、module slot、真实 loader | `astra-core`, `abi_stable`, `libloading`, `semver` | Editor widget、native handles in public API |
| `astra-property` | 1 | PropertySystem metadata、Inspector/save glue、derive re-export | `astra-core`, `astra-property-derive` | hidden inheritance、global object system |
| `astra-property-derive` | 1 | derive macro | `syn`, `quote`, `proc-macro2` | runtime state、global object system |
| `astra-test` | 1 | YAML scenario runner、report compare | `astra-core`, `astra-runtime`, `serde_yaml` | platform UI |
| `astra-cli` | 1 | `astra test run`、`astra report explain` | `astra-test`, `clap` | package build、release validation |
| `astra-asset` | 2 | AssetId、VFS、sidecar schema、AssetRegistry | `astra-core`, `serde`, `schemars` | decoder native handle |
| `astra-cook` | 2 | Importer/CookProcessor、DDC key、package builder | `astra-asset`, `astra-package` | Editor UI |
| `astra-package` | 2 | binary package/save container、section reader/writer | `astra-core`, `postcard`, `serde` | story/runtime semantics |
| `astra-media` | 2 | Renderer2D/TextLayout/Decode/FilterGraph/AudioGraph traits | `astra-core`, `wgpu`, `cosmic-text` | VN state |
| `astra-release` | 2 | Release Gate validators、report writer | `astra-core`, `astra-package`, `astra-test` | Editor-only state |
| `astra-vn` | 3 | `.astra` parser/compiler、VN Core、Luau policy host | `astra-core`, `astra-runtime`, `astra-media`, `pest`, `mlua` | platform-native handles |
| `astra-editor-bridge` | 4 | Qt/Rust bridge、PIE/debug API | `astra-runtime`, `astra-vn`, `astra-release` | packaged runtime dependency |
| `astra-ai` | 4 | Runtime AI committed output、Editor AI audit | `astra-core`, `astra-runtime` | provider secret in replay |
| `astra-mcp` | 4 | MCP tool descriptor、permission、audit | `astra-core`, `astra-plugin` | Editor widget in Runtime tools |
| `astra-emu-manager` | 5 | Manager app, RuntimeWorld bridge, plugin enablement, report | `astra-core`, `astra-runtime`, `astra-plugin` | family VM internals |
| `astra-emu-family-api` | 5 | LegacyFamilyPlugin descriptor, VFS/script/action/media/snapshot provider DTO | `astra-core`, `astra-runtime`, `astra-package` | Manager UI |
| `astra-emu-artemis` | 5 | Artemis v1 family plugin | `astra-emu-family-api`, `astra-plugin` | EngineCore private state |

## Binaries

| Binary | Crate | Command |
| --- | --- | --- |
| `astra` | `astra-cli` | Stage 1: `astra test run`, `astra report explain`; Stage 2 adds `astra package validate` |
| `astra-editor` | `astra-editor-app` | Qt/QML creator editor |
| `astra-emu-manager` | `astra-emu-manager` | legacy VN manager that creates and drives RuntimeWorld |
| `astra-emu-family-*` | family crate | in-process legacy family plugin |

## Feature Rules

- Default features must build desktop core without Editor, AI provider or EMU family.
- `luau` enables AstraVN and AstraEMU policy host integration. Legacy family adapters may parse historical script names inside their private core.
- `wgpu` is default Renderer2D provider. Platform decode features are profile-specific.
- `headless` must be available for Runtime, Media and Test without window creation.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p astra-cli -- test run scenarios/native_smoke.yaml --headless --report target/reports/stage1.yaml
cargo run -p astra-cli -- report explain target/reports/stage1.yaml
```

Expected report: `astra.scenario_report.v1` with matching state/event/presentation hash.
