# AstraEMU 第三方源码

外部核心使用完整 fork 和精确 submodule 提交。主仓中的 Family 适配层负责新 ABI，核心保留原生 VM、GPU、平台和存档实现。

`art3m1s-core/` 固定到本地适配提交 `13e55ffa98b8ea2fec4b294d210bd50567106a08`，尚未推送；Family 接入仍在实施。来源与差异见 [Artemis 修改说明](../Source/Families/astra-emu-artemis/MODIFICATIONS.md)，许可证见 [art3m1s-core/LICENSE](art3m1s-core/LICENSE)。

`rfvp/` 是 FVP adapter 使用的 MPL-2.0 源码，不属于主 workspace 成员，也不单独作为应用入口构建。目录放在 Family crate 外，避免 Cargo 将嵌套路径依赖自动加入 workspace。

来源、固定 revision 和修改范围见 [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)及[第三方说明](../Source/Families/astra-emu-fvp/THIRD_PARTY_NOTICES.md)。完整许可证保存在 [rfvp/LICENSE](rfvp/LICENSE)。
