# AstraVN Module Layout Migration

本计划只迁移已经存在的 `astra-vn` 代码路径和依赖声明，把 AstraVN 从 Runtime 分区移到 `Engine/Source/Modules/AstraVN/`。它不拆分代码逻辑；拆分步骤见 [AstraVN Crate Split Migration](astra-vn-crate-split-migration.md)。

## 迁移前实现入口

- `Engine/Source/Runtime/astra-vn`：迁移前单 crate，包含 `.astra` parser/compiler、VN Core、Luau policy、presentation、system UI、save/package、Editor metadata、plugin extension 和 Rust dylib facade。
- `Engine/Source/Developer/astra-test`：通过 `astra-vn` 运行 VN scenario。
- `Engine/Source/Developer/astra-release`：通过 `astra-vn` 校验 `vn.*`、`tsuinosora.*` 和 `player.full_playable` gate。
- `Engine/Source/Programs/astra-cli`：通过 `astra-vn` 执行 NativeVN cook/package/bundle/test route flow。
- 根 `Cargo.toml`：迁移前 workspace member 指向 `Engine/Source/Runtime/astra-vn`。

## 目标设计

AstraVN 成为 UE 风格 Module 分区下的产品模块：

```text
Engine/Source/Modules/AstraVN/
  astra-vn/
```

`astra-vn` package name 保持不变，仍输出 `rlib` 和 Rust ABI `dylib`。搬迁后，Runtime 分区只保留 EngineCore、Asset/VFS、Media、Package、Plugin、Player Core 等共享 runtime crate；AstraVN 的垂直产品实现不再放在 `Engine/Source/Runtime/`。

## 分步迁移

1. 移动 crate 目录。
   将 `Engine/Source/Runtime/astra-vn` 移到 `Engine/Source/Modules/AstraVN/astra-vn`，保留现有 `src/`、`tests/`、`Cargo.toml` 和 `standard_policy.luau`。
2. 更新 workspace member。
   根 `Cargo.toml` 中的 member 改为 `Engine/Source/Modules/AstraVN/astra-vn`。
3. 更新 `astra-vn` 内部 path dependency。
   新路径下，`astra-core`、`astra-media-core`、`astra-package`、`astra-runtime` 指向 `../../../Runtime/...`；`astra-target` 指向 `../../../Platform/astra-target`；dev dependency `astra-plugin` 指向 `../../../Runtime/astra-plugin`。
4. 更新下游 crate path dependency。
   `astra-test`、`astra-release` 和 `astra-cli` 的 `astra-vn` path 改为 `../../Modules/AstraVN/astra-vn`。
5. 更新文档和状态页路径。
   `Docs/implementation/workspace-blueprint.md`、`Docs/status/stages/stage-3-astra-vn.md`、`Docs/status/stages/stage-test-matrix.md` 和 `Docs/status/coverage-matrix.md` 改用新 module path。
6. 清理旧路径引用。
   `Engine/Source/Runtime/astra-vn` 只允许在迁移文档中作为迁移前路径出现，不能继续作为目标实现路径。

## 验收命令

```bash
python Tools/check_docs.py
rg -n "Engine/Source/Runtime/astra-vn" Docs Cargo.toml Engine -g "*.md" -g "*.toml"
cargo metadata --no-deps
cargo test -p astra-vn --test vn_dylib_facade
cargo test -p astra-vn-plugin --test vn_plugin_extensions
cargo test -p astra-test --test vn_scenario
cargo test -p astra-release --test release_report release_gate_
cargo test -p astra-cli --test target_platform nativevn_minimal_profile_cooks_packages_and_runs_headless
```

代码搬迁完成后，`rg` 命令只允许本文件、[AstraVN Crate Split Migration](astra-vn-crate-split-migration.md) 和 [Game Runtime Provider Migration](game-runtime-provider-migration.md) 命中迁移前路径；其他命中必须改链。

## 不得修改项

- 不改变 `astra-vn` package name、crate public re-export、`.astra` source 语义、VN package section schema 或 release check id。
- 不把 AstraVN Core 抽成 AstraEMU/AstraRPG 的基类。
- 不把 Editor UI、Luau VM handle、renderer/audio native handle、Actor 指针、本地 root 或商业 payload 放入 public API、save、package 或 report。
- 不把本次路径搬迁写成 Stage 3 完成证据；它只关闭 module layout 对齐项。
