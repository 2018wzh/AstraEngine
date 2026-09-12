# Workspace 组织

当前工程采用单仓产品分区；目标边界见 [重构契约](../contracts/rebuild.md)，验证方式见 [开发手册](../manual/development.md)。

| Workspace | 成员与边界 |
| --- | --- |
| 根 Cargo.toml | Engine 共享库、VN、Player、开发工具和公开测试 |
| Emulator/Cargo.toml | Family API、FVP、Manager 与其服务；Minori/SDK 依真实迁移接入 |
| Editor/Cargo.toml | GPUI 实现接入时建立，当前不创建空 workspace |

各 workspace 独立 lockfile/target，共享底层库以 path dependency 引用。第三方 RFVP 保持独立源码和自己的 workspace，不纳入项目格式重写。

NativeVN 内部动态 ABI、Headless 宏、纯转发 crate 随真实消费者迁移删除。尚未实现的 RPG/AI/平台模块不进入活动 workspace。新增 crate 须承担真实独立职责或替换旧模块，不能用空 facade 证明能力完成。
