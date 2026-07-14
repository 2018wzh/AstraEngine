# ADR 0014: Stable Rust toolchain 与构建身份

## Context

workspace 当前声明 `rust-version = "1.85"`，但主线依赖已经超过这个工具链可以被可靠证明的范围。继续把 1.85 写成 MSRV，会让 manifest、实际依赖和 CI 证据互相矛盾。UI migration 还会引入 Yakui、egui、Luau analyzer 和 Web component tooling，不能再靠开发机上碰巧可用的工具版本决定发布结果。

## Decision

AstraEngine 改用仓库根 `rust-toolchain.toml` 的 `stable` channel 作为唯一 Rust 工具链入口。实现 Migration 12 时删除 workspace 和成员 crate 的 `rust-version` 声明，不再承诺未经独立 CI 验证的固定 MSRV。

每次构建都必须把实际 `rustc`、Cargo、target、profile、feature、workspace manifest、`Cargo.lock` 和 commit/dirty state 写入 `astra.build_identity.v1`。UI 第三方依赖先在该 toolchain 上完成 license、API、target 和依赖隔离检查，再在同一实现提交中精确锁定。解析失败或工具链身份漂移直接阻断，不允许 vendored fallback 或共享 `target` 兜底。

## Consequences

- `stable` 表示由 lockfile 和 build identity 固定本次构建，不表示允许依赖自动漂移。
- CI、Cook、fixture、Windows/Web bundle 和 release report 使用同一工具链指纹。
- Yakui、egui、`luau-analyze`、jco 等工具不在设计文档中预写易过期版本号；实现提交必须记录最终版本和 hash。
- 若产品以后需要正式 MSRV，必须增加独立 CI 矩阵和新 ADR，不能从 `rust-version` 字段反推完成度。

## Verification

本 ADR 当前只锁定迁移方向。实现完成后，`Tools/run_cargo_isolated.py` 必须证明 toolchain/lockfile/feature/target 身份一致，并阻断 identity mismatch。

