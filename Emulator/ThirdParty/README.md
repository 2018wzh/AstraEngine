# AstraEMU 第三方源码

外部核心使用完整 fork 和精确 submodule 提交。薄嵌入层提供新 ABI，主仓 Family 只保留必要的注册与构建桥；VM、GPU、平台和存档实现继续由原生核心拥有。

| 来源目录 | 固定 revision | 差异与许可 |
| --- | --- | --- |
| `art3m1s-core/` | `ae967d8446fe99b97f6e1df93fb1b61d8ada2040` | [Artemis 修改说明](../Source/Families/astra-emu-artemis/MODIFICATIONS.md)，完整 MPL-2.0 |
| `rfvp/` | `36786d644d988d1c84e129c915cc7bbdaf578ddd` | [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)，完整 MPL-2.0；嵌入字体授权仍待核验 |
| `siglus_rs/` | `fd6b9f8e464cb06b4d0d17842342a15c8ee1be8a` | [Siglus 修改说明](../Source/Families/astra-emu-siglus/MODIFICATIONS.md)，含原生回标题修复，完整 MPL-2.0 与嵌套 Kira |
| `sena-rs/` | `67e269e4cd0387fd3292388ca7d4005bc902412d` | 通用基线 `2d28936c` 上的单个 ABI 适配，完整 MPL-2.0；识别原版与准确 AppID 的 Steam Koikake |
| `kirikiri2/` | `32130382185b60fe047ae9d98f8f3e4795c4db1e` | 完整 Kirikiroid2-web，保留原组件许可与独立生命周期修复 |
| `cpal/` | `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` | CPAL 0.15.3 的 WASAPI 关闭与 ALSA 依赖兼容修复，Apache-2.0 |

上述精确提交均已推送对应 fork；旧分支与完整来源历史保留。RFVP 适配分支是 `codex/emu-native-single-adaptation-20261018`，Siglus、Sena 是 `codex/emu-native-abi2-final-20261018`；各自直接基于通用修复基线。

这些目录不是替代 Manager 的应用入口。构建只选择相应 native library，不将其他核心一起初始化或编译作为前置条件。Ethornell 的许可覆盖待澄清，本地适配尚未注册或推送；具体分发限制见 [第三方声明](../THIRD_PARTY_NOTICES.md)。
