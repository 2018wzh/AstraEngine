# 公共契约

产品方向和跨模块边界以[总体架构](../product/architecture.md)、[重构契约](rebuild.md)为准。下列页面解释现有 Rust API、数据格式和真实消费者；不以未来 provider、阶段矩阵或报告清单反向要求实现。

| Contract | 文件 |
| --- | --- |
| Logging / Crash Observability | [logging-observability.md](logging-observability.md) |
| Runtime | [runtime.md](runtime.md) |
| Runtime 异步任务作用域 | [task-scope.md](task-scope.md) |
| Plugin ABI | [plugin-abi.md](plugin-abi.md) |
| Data / Save / Package | [data-formats.md](data-formats.md) |
| Asset VFS | [asset-vfs.md](asset-vfs.md) |
| Media | [media.md](media.md) |
| Runtime UI | [ui.md](ui.md) |
| Performance | [performance.md](performance.md) |
| Player 片段预览 | [player-preview.md](player-preview.md) |
| Script / VN | [script-vn.md](script-vn.md) |
| 演出帧推进迁移 | [presentation-tick-migration.md](presentation-tick-migration.md) |
| AstraEMU 独立 Family ABI、最终帧与 PCM | [astraemu-ipc.md](astraemu-ipc.md) |
| Release Gate | [release-gate.md](release-gate.md) |
| TsuiNoSora Sidecar Schema | [tsuinosora-sidecar-schema.md](tsuinosora-sidecar-schema.md) |
