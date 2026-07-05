# ADR 0008: 插件采用 Rust-facing `abi_stable` 风格 ABI

## Context

插件作者需要 Rust 体验，Engine 又需要可审计的二进制兼容边界。纯 Rust dylib 跨版本风险高，手写底层 ABI 接口开发成本高。

## Decision

插件采用 Rust-facing `abi_stable` 风格 ABI。插件通过 descriptor 声明 engine version、rustc fingerprint、feature fingerprint、capability、permission 和 packaged eligibility。插件支持加载/卸载，不支持运行中重载。

## Consequences

插件开发体验接近 Rust trait，Release Gate 仍能拒绝不匹配 binary。所有 provider 必须通过 registry 和 slot 注册。
