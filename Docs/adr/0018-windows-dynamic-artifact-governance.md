# ADR 0018: Windows dynamic artifact governance

## Context

Rust `dylib` 会把其 Rust linkage closure 的公开符号带入 PE export table。AstraVN facade 曾因导出数越过 Windows 的 65,535 上限而阻断测试和 Player packaging。更换 MSVC 或 lld 不能改变该格式上限；也不能用隐藏导出、链接器 fallback 或忽略失败来伪造通过。

## Decision

- `Tools/dynamic_artifact_policy.toml` 是 workspace 动态制品的唯一允许清单。未列出的 `dylib` 或 `cdylib` 一律阻断。
- `astra-engine` 与 `astra-vn` 保持 `rlib + dylib` 的 Rust compatibility facade。它们不是插件稳定 ABI；跨插件边界继续使用 `astra-plugin-abi` 的 bounded DTO 和 RootModule。
- compatibility facade 只保留 runtime、script、presentation 与 system 的受控 public surface；package、policy、runtime-provider API 由各自 feature crate 直接提供。功能 crate 不得依赖 facade。策略清单固定 facade 的直接 workspace dependency closure；新增依赖必须同时审查导出预算和更新该清单。
- Android/Web Player 的 `cdylib` 是平台 host 制品；provider fixture 的 `cdylib` 仅用于受控测试，不能被表述为发布 provider。
- Windows Rust dylib 的 named export 硬预算为 60,000。`Tools/check_dynamic_artifacts.py --verify-windows-exports` 在专属 audit target 构建并解析 PE export directory；缺产物、PE 格式异常、超预算或策略漂移均失败。

## Consequences

`cargo test --workspace` 前必须先通过动态制品静态检查；Windows CI 还必须通过 export audit。预算接近或超过上限时，只能通过收缩 facade 或其依赖闭包解决，不能加入静默链接器兜底。审计输出只包含 crate、预算和计数，不记录本机路径、导出符号名或二进制内容。

```bash
python Tools/check_dynamic_artifacts.py
python Tools/check_dynamic_artifacts.py --verify-windows-exports
```
