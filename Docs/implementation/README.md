# 实现说明

产品边界以[总体架构](../product/architecture.md)和[重构契约](../contracts/rebuild.md)为准，实际进度只在[实施状态](../status/implementation-plan.md)记录。本目录中的旧 Stage、provider、Qt、sandbox 和证据审批说明不再构成实施门禁；尚未迁移的页面只能解释对应旧实现，不能覆盖现行契约。

## 按职责查阅

| 入口 | 内容 |
| --- | --- |
| [Workspace 组织](workspace-blueprint.md) | 根、Editor、Emulator workspace 与依赖边界 |
| [开发与测试](../manual/development.md) | 定向测试、完整检查、平台与私有数据限制 |
| [Engine](../modules/engine-core.md) | 可嵌入运行层与公共能力 |
| [AstraVN](../modules/astra-vn.md) | typed VN 会话、剧情、呈现与存读档 |
| [Editor](../../Editor/README.md) | GPUI、ACP/MCP 编辑及真实预览入口 |
| [EMU 客户端](../manual/astraemu-client.md) | 独立 Manager、核心与运行包 |
| [EMU SDK](../../Emulator/Source/SDK/astra-emu-sdk/README.md) | 可选共享底层模块，不依赖 VN session |

## 日常修改

- Rust 类型是接口与 schema 的真源；先读真实调用方，再修改共享契约。删除旧路径时同步消费者，不保留长期兼容双轨。
- crate 按实际消费者、平台隔离与生命周期划分。合并纯转发层；proc-macro、动态库产物和独立工具保留各自编译边界，不为数量目标合并。
- 大模块按职责拆分，保持原公开 API 路径；内部可见性只覆盖真实调用方。方法、trait impl、属性、平台 cfg、嵌入资源路径和测试 crate-root 路径必须一起迁移。
- 模块变短不等于删掉实现或提升性能。保留真实文字、媒体、错误与资源回收；不新增每帧复制、通用转发或第二套执行路径。
- 普通修改运行受影响 crate 的检查和测试；阶段结束才运行完整活动 workspace 检查。GPU、音频和设备行为按需启动真实宿主，缺环境明确记为未验证。
- 文档只更新受影响接口与实际状态。链接与内容卫生由 `Tools/check_docs.py` 检查，不再强制 Stage 矩阵、四份状态表联动或具名证据审批。
