# AstraEMU 第三方源码

`rfvp/` 是 FVP adapter 使用的 MPL-2.0 源码，不属于主 workspace 成员，也不单独作为应用入口构建。目录放在 Family crate 外，避免 Cargo 将嵌套路径依赖自动加入 workspace。

来源、固定 revision 和修改范围见 [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)及[第三方说明](../Source/Families/astra-emu-fvp/THIRD_PARTY_NOTICES.md)。完整许可证保存在 [rfvp/LICENSE](rfvp/LICENSE)。

`siglus_rs/` 是 Siglus family adapter 使用的 MPL-2.0 源码，以 git submodule 指向
`2018wzh/siglus_rs` 的 `astra-hosted` 分支（上游 `xmoezzz/siglus_rs` 加单个适配
commit）。它不属于主 workspace 成员。来源、固定 revision 和修改范围见
[Siglus 修改说明](../Source/Families/astra-emu-siglus/MODIFICATIONS.md)及
[第三方说明](../Source/Families/astra-emu-siglus/THIRD_PARTY_NOTICES.md)。

`rfvp/` 与 `kirikiri2/` 之外，新增第三方核心一律以 submodule 接入，不在本仓
镜像全量源码。克隆后需执行 `git submodule update --init` 获取 submodule 内容。
