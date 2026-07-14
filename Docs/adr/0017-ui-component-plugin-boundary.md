# ADR 0017: UI component plugin ABI 与信任边界

## Context

项目需要作品专属 Route Chart、Phone Trigger、特殊 HUD 等组件，但不能让插件跨 ABI 传递 Yakui node、callback、GPU handle 或 Rust 对象所有权。Windows native plugin 与 Web component 的装载模型不同，签名和资源上限必须在进入主线前固定。

## Decision

新增独立 `astra-ui-plugin-abi`。它复用通用 plugin 的 fingerprint、签名和 lifecycle helper，但自行定义 UI component descriptor、typed slot、bounded DTO 和 session API。host 负责把 DTO 实现为 Yakui node；插件从不持有 Yakui、wgpu、winit 或平台对象。

动态 component 只能挂载到 `.astra` 静态声明的 typed slot。slot 声明允许的 provider、component type、最大实例数和稳定 instance key。生命周期为 provider create/destroy、session open/close、component mount/update/event/snapshot/restore/unmount。插件状态只能是 schema-bound UI session state，不得成为 save 或 gameplay authority。

Windows artifact 使用 Ed25519 签名的 dylib，项目/release profile 明确 signer allowlist。签名覆盖 canonical manifest、schema、capability 和 artifact hash。native dylib 是进程内受信代码；capability 清单只约束 host API，不构成对插件直接 OS 调用的沙箱。

Web 由 Rust DTO schema 生成并校验 WIT adapter，Cook 使用精确锁定的 jco 把已签名 component 转为 ES module 与 core wasm，并绑定输入输出 hash。浏览器不直接装载 Component Model binary。

## Consequences

- host capability 仅开放 IME、clipboard read/write 和 open-external-URL，并要求 target/profile grant、用户手势和权限证据；不提供任意 host 文件或网络 API。
- hard limit：tree depth 32、每 view 4096 nodes、每 view 1024 component instances、单 DTO 4 MiB、每 provider/session state 1 MiB、每次调用 256 effects、Web memory 64 MiB。release profile 还必须声明更严格的 time/fuel budget。
- panic、非法 DTO、越界、超时或 restore 失败会终止 UI session，不生成替代组件，也不切换 provider。
- Migration 12 用签名 Windows/Web fixture 验证 ABI，但正式产品页面不得依赖 fixture 才能运行。

## Verification

实现必须覆盖签名/allowlist、fingerprint、typed slot、生命周期、state restore、DTO bounds、effect bounds、Web transpile/hash、capability permission、panic/timeout 和 redaction。任何失败均为 blocking diagnostic。
