# ADR 0015: UI backend provider 分工

## Context

AstraVN 需要可换肤、手柄友好、适合发布游戏的 UI；AstraEMU Manager 需要表单、日志、诊断和工具面板。两类产品共享输入、语义、纹理和绘制边界，但不适合强行使用同一个第三方 widget runtime。Editor 已由 ADR 0002 固定为 Qt/QML，也不应被运行时 UI 反向替换。

## Decision

- AstraVN 发布 UI 使用 Yakui backend。
- AstraEMU Manager 与诊断/翻译 overlay 使用 Slint 1.17.1；该实现留在 Stage 5，不属于 Migration 12 的 AstraVN 代码范围。
- AstraEditor 继续使用 Qt/QML；PIE 只消费共享 runtime UI 的 frame、semantic snapshot 和 input disposition。
- Astra 自有 `astra-ui-core` 持有 backend-neutral input、action、semantic、resource 和 render contract。Yakui、Slint、wgpu、winit 类型不得进入 package、save/replay、plugin ABI 或产品 public ViewModel。
- provider 必须由 target/profile/package 的唯一显式 binding 选择。缺 binding、冲突或能力不匹配时阻断，不按注册顺序选取，也不回退到另一个 backend。

AstraVN 只使用 Yakui core 和经过审核的 upstream widgets。平台输入、wgpu adapter、正式文本、语义树和资源生命周期由 Astra 持有；不引入 `yakui-wgpu`、`yakui-winit` 或 `yakui-app`。正式 VN 文本只走 Astra TextLayout。

## Consequences

- `SystemUiModel` 固定矩形命中模型在 Migration 12 完成时删除。
- VN 的 UI 输出并入 renderer-ready Scene2D/Mesh2D 主路径，不恢复 bitmap/headless 产品 presenter。
- Headless 复用 Migration 11 的完整 test host 和相同 UI contract，只提供 E2 证据。
- AstraEMU 的 Slint host 持有窗口、winit 0.30 event loop、surface 和精确锁定的 wgpu 29.0.4 `Device`/`Queue`。Astra renderer 绘制游戏 underlay，Slint 绘制 Manager/overlay；二者共享同一设备，禁止 CPU 整帧回读和跨设备纹理复制。Slint 类型不能暴露给 family plugin、Manager Core 或 RuntimeWorld。
- AstraEMU 采用 Slint Royalty-free 2.0 模式，在 About 和第三方 notices 保留规定归因；workspace 许可证不因此改为 GPL。

## Verification

Migration 12 必须证明 Windows/Web 的显式 Yakui binding、物理输入消费、语义快照、Scene2D 输出、context restore 和无 backend 类型泄漏。AstraEMU Slint/WGPU host、响应布局、accessibility、overlay 输入隔离和平台证据由 Stage 5 单独关闭。
