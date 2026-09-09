# AstraEMU 第三方源码

`rfvp/` 是 FVP adapter 使用的 MPL-2.0 源码，不属于主 workspace 成员，也不单独作为应用入口构建。目录放在 Family crate 外，避免 Cargo 将嵌套路径依赖自动加入 workspace。

来源、固定 revision 和修改范围见 [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)及[第三方说明](../Source/Families/astra-emu-fvp/THIRD_PARTY_NOTICES.md)。完整许可证保存在 [rfvp/LICENSE](rfvp/LICENSE)。
