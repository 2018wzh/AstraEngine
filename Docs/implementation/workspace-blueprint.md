# Workspace Blueprint

AstraEngine v1 采用一个 Rust workspace 承载核心 crate 和 CLI。系列仓库可以拆分实现，但 public contract、schema 和测试命令以本蓝图为准。

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
| `astra-core` | 1 | id、diagnostic、schema、hash、source span | `serde`, `schemars`, `thiserror`, `miette` | Runtime、Editor、Luau、GPU/audio |
| `astra-runtime` | 1 | World、Actor/Component、StateMachine、EventBus、AwaitToken、Save/Replay facade | `astra-core`, `tokio` | Editor UI、Luau host、renderer backend |
| `astra-plugin` | 1 | `abi_stable` descriptor、registry、module slot | `astra-core`, `abi_stable`, `libloading` | Editor widget、native handles in public API |
| `astra-property` | 1 | derive、PropertySystem metadata、Inspector/save glue | `astra-core`, `syn`, `quote`, `proc-macro2` | hidden inheritance、global object system |
| `astra-test` | 1 | YAML scenario runner、report compare | `astra-core`, `astra-runtime`, `serde_yaml`, `clap` | platform UI |
| `astra-asset` | 2 | AssetId、VFS、sidecar schema、AssetRegistry | `astra-core`, `serde`, `schemars` | decoder native handle |
| `astra-cook` | 2 | Importer/CookProcessor、DDC key、package builder | `astra-asset`, `astra-package` | Editor UI |
| `astra-package` | 2 | binary package/save container、section reader/writer | `astra-core`, `postcard`, `serde` | story/runtime semantics |
| `astra-media` | 2 | Renderer2D/TextLayout/Decode/FilterGraph/AudioGraph traits | `astra-core`, `wgpu`, `cosmic-text` | VN state |
| `astra-release` | 2 | Release Gate validators、report writer | `astra-core`, `astra-package`, `astra-test` | Editor-only state |
| `astra-vn` | 3 | `.astra` parser/compiler、VN Core、Luau policy host | `astra-core`, `astra-runtime`, `astra-media`, `pest`, `mlua` | platform-native handles |
| `astra-editor-bridge` | 4 | Qt/Rust bridge、PIE/debug API | `astra-runtime`, `astra-vn`, `astra-release` | packaged runtime dependency |
| `astra-ai` | 4 | Runtime AI committed output、Editor AI audit | `astra-core`, `astra-runtime` | provider secret in replay |
| `astra-mcp` | 4 | MCP tool descriptor、permission、audit | `astra-core`, `astra-plugin` | Editor widget in Runtime tools |
| `astra-emu-manager` | 5 | Manager process, RPC, shared memory, report | `astra-core`, `astra-runtime` | family VM internals |
| `astra-emu-core-api` | 5 | family core ABI/RPC schema | `astra-core`, `astra-package` | Manager UI |
| `astra-emu-krkr` | 5 | first v1 family core | `astra-emu-core-api` | EngineCore private state |

## Binaries

| Binary | Crate | Command |
| --- | --- | --- |
| `astra` | `astra-cli` | `astra package validate`, `astra test run`, `astra report explain` |
| `astra-editor` | `astra-editor-app` | Qt/QML creator editor |
| `astra-emu-manager` | `astra-emu-manager` | legacy VN manager |
| `astra-emu-core-*` | family crate | out-of-process compat core |

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
astra test run scenarios/native_smoke.yaml --headless
```

Expected report: `astra.scenario_report.v1` with matching state/event/presentation hash.
