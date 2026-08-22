# AstraEMU Family ABI v9 Migration

Family ABI v9、Product Runtime Provider ABI v4 与 Extension ABI v1 是一次破坏性迁移。Host 和 family 必须在同一版本完成切换；仓库不提供 v7/v8 compatibility shim，也不允许一个进程同时解析 Git 与 path 两份 `astra-emu-family-api`。

## Consumer 必须修改的内容

1. Product runtime descriptor 增加唯一 `presentation_lane`。AstraVN 使用 `Scene2D`，AstraEMU 使用 `Layer2D`。
2. Family descriptor 增加 `core_kind` 与 `presentation_mode`。native core 只能选择 `Native + MultiLayer`，ported core 只能选择 `Ported + SingleLayer`。
3. 删除 scene draw transaction、snapshot/save/restore、text lease、session resource presentation 与 step budget 调用。
4. 在 `step` 前绑定 Host surface、Hook 和 writable-file ports。Family 在 acquire 前调用同步 Hook，随后直接写 Host-owned surface，并在同一步提交 damage 与 retained Layer2D transaction。
5. Family 自行完成 decode、字体 fallback、shaping、换行、光栅化和原生存档格式。Host 只验证 ABI 所有权、surface geometry、transaction、路径隔离和系统错误。
6. 删除 AstraEMU runtime state/text/frame/audio/route/session/input 与 RFVP live hash。package、binary、source/archive entry、schema、build、profile 和 artifact-file 完整性 hash 不变。

FVP 固定使用 `Ported + SingleLayer`。RFVP hosted feature 直接依赖固定 AstraEngine ABI commit；AstraEngine workspace 通过精确 Git source `[patch]` 映射到当前 path crate，并以 `cargo tree` 阻断双 package identity。`astra-emu-fvp` 只保留 dylib/root-module、build identity、descriptor、provider 构造与 shutdown、panic containment、最终错误映射和 observability 边界。

Minori 固定使用 `Native + MultiLayer`，把 background、foreground/stand、effect、panel/text 映射为独立 retained layer。Siglus v8 不属于本迁移分支，必须单独迁移到 v9 后才能合并。

## 验收边界

ABI milestone 只要求 ABI crates、schema generator、loader rejection、文档与格式检查通过。consumer 尚未迁移时，完整 workspace 失败是已知迁移状态，不能伪装成兼容实现。完成条件还包括 RFVP 独立构建、唯一 ABI package identity、FVP/Minori/Host product tests、Performance E2 和最终 workspace gate。Headless 只形成 E2；Windows Manager 的真实输入、画面、音频与 shutdown 仍需独立 E3。
