# AstraEMU Family ABI v9 Migration

> 历史迁移记录，已由 [独立 Host 重构](../migrations/astraemu-independent-host.md) 取代。下述接口不再用于当前 AstraEMU，也不保留兼容加载路径。

Family ABI v9、Product Runtime Provider ABI v4 与 Extension ABI v1 是一次破坏性迁移。Host 和 family 必须在同一版本完成切换；仓库不提供 v7/v8 compatibility shim，也不允许一个进程同时解析 Git 与 path 两份 `astra-emu-family-api`。

## Consumer 必须修改的内容

1. Product runtime descriptor 增加唯一 `presentation_lane`。AstraVN 使用 `Scene2D`，AstraEMU 使用 `Layer2D`。
2. Family descriptor 增加 `core_kind` 与 `presentation_mode`。native core 只能选择 `Native + MultiLayer`，ported core 只能选择 `Ported + SingleLayer`。
3. 删除 scene draw transaction、snapshot/save/restore、text lease、session resource presentation 与 step budget 调用。
4. 在 `step` 前绑定 Host surface、Hook 和 writable-file ports。Family 在 acquire 前调用同步 Hook，随后直接写 Host-owned surface，并在同一步提交 damage 与 retained Layer2D transaction。
5. Family 自行完成 decode、字体 fallback、shaping、换行、光栅化和原生存档格式。Host 只验证 ABI 所有权、surface geometry、transaction、路径隔离和系统错误。
6. 删除 AstraEMU runtime state/text/frame/audio/route/session/input 与 RFVP live hash。package、binary、source/archive entry、schema、build、profile 和 artifact-file 完整性 hash 不变。

Surface lease 使用独占、不可复制的 `OwnedWritableByteBuffer`。它在 FFI 上保留同一分配的可写指针与唯一 owner，acquire、family 写入、commit 全程不复制；只读 `OwnedByteBuffer` 不能用于 surface，也禁止通过 `const` 强转恢复写权限。`FfiLegacyFamilyHostAdapter` 是动态 family 绑定 VFS、surface、Hook 与 writable-file ports 的唯一公共适配入口。

FVP 固定使用 `Ported + SingleLayer`。RFVP hosted feature 直接依赖固定 AstraEngine ABI commit；AstraEngine workspace 通过精确 Git source `[patch]` 映射到当前 path crate，并以 `cargo tree` 阻断双 package identity。`astra-emu-fvp` 只保留 dylib/root-module、build identity、descriptor、provider 构造与 shutdown、panic containment、最终错误映射和 observability 边界。

Minori 固定使用 `Native + MultiLayer`，把 background、foreground/stand、effect、panel/text 映射为独立 retained layer。Siglus v8 不属于本迁移分支，必须单独迁移到 v9 后才能合并。

## 验收边界

ABI milestone 已通过 ABI crates、schema generator、loader rejection、文档与格式聚焦验证。FVP/RFVP、Minori、Manager、CLI、Headless 和 WGPU renderer 已迁移；动态 Extension loader 与 CLI/Headless 显式 binding 已接入。完成条件仍包括唯一 ABI package identity复验、FVP/Minori/Host product tests、Performance E2 和最终 workspace gate。Headless 只形成 E2；Windows Manager 的真实输入、画面、音频与 shutdown 仍需独立 E3。
