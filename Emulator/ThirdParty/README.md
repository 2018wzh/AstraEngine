# AstraEMU 第三方源码

外部核心使用完整 fork 和精确 submodule 提交。薄嵌入层提供新 ABI，主仓 Family 只保留必要的注册与构建桥；VM、GPU、平台和存档实现继续由原生核心拥有。

| 来源目录 | 固定 revision | 差异与许可 |
| --- | --- | --- |
| `art3m1s-core/` | `ae967d8446fe99b97f6e1df93fb1b61d8ada2040` | [Artemis 修改说明](../Source/Families/astra-emu-artemis/MODIFICATIONS.md)，完整 MPL-2.0 |
| `rfvp/` | `478448ed9d60778f78d5de911fa5717c48cb201c` | [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)，移除自动验收驱动，完整 MPL-2.0；字体授权仍待核验 |
| `siglus_rs/` | `a94565cccd4f6d5e0a341e5732c5d98e9de2c71f` | [Siglus 修改说明](../Source/Families/astra-emu-siglus/MODIFICATIONS.md)，保留原生文字/键盘输入语义，完整 MPL-2.0 与嵌套 Kira |
| `sena-rs/` | `8465e2818842b95aeb37424104e3ab95feff777d` | 通用基线 `5fd1a8e3` 上的单个 ABI 适配；引擎标识为 SoftPAL、模拟器为 Sena，支持范围不变，完整 MPL-2.0 |
| `kirikiri2/` | `5b2eccd0a51b6b47bb845f48acfd12bd2167c477` | 完整 Kirikiroid2-web，保留原组件许可、独立生命周期及 WindowEx owner 初始化修复 |
| `cpal/` | `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` | CPAL 0.15.3 的 WASAPI 关闭与 ALSA 依赖兼容修复，Apache-2.0 |

RFVP、Siglus、Artemis、Sena 和 Kirikiroid2-web 的核心源码统一发布于各 fork 的 `astra`；主仓仍固定上表的精确 revision。通用修复基线保留在提交历史中，CPAL/Kira 使用按版本命名的稳定分支。本轮临时分支已清理，旧引用另存 Git bundle；`astra` 强推切换经用户明确批准。

这些目录不是替代 Manager 的应用入口。构建只选择相应 native library，不将其他核心一起初始化或编译作为前置条件。Ethornell 适配另发布于 [2018wzh/ethornell-rs 的 astra](https://github.com/2018wzh/ethornell-rs/tree/astra)；许可覆盖仍待澄清，尚未注册为可分发 submodule，具体限制见 [第三方声明](../THIRD_PARTY_NOTICES.md)。
