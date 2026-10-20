# AstraEMU 第三方源码

外部核心使用完整 fork 和精确 submodule 提交。薄嵌入层提供新 ABI，主仓 Family 只保留必要的注册与构建桥；VM、GPU、平台和存档实现继续由原生核心拥有。

| 来源目录 | 固定 revision | 差异与许可 |
| --- | --- | --- |
| `art3m1s-core/` | `be82d5e0bbafcef55a530f69e5b9358094dbd671` | [Artemis 修改说明](../Source/Families/astra-emu-artemis/MODIFICATIONS.md)，完整 MPL-2.0 |
| `rfvp/` | `1e947a5ccbd2c7a698bd30d48347454ef1b908a9` | [FVP 修改说明](../Source/Families/astra-emu-fvp/MODIFICATIONS.md)，C ABI v3 移植并删除 D3D12 交付，完整 MPL-2.0 |
| `siglus_rs/` | `c0f7e7bbdf51a098288fd2c634d5ba8912786d6f` | [Siglus 修改说明](../Source/Families/astra-emu-siglus/MODIFICATIONS.md)，C ABI v3 移植，保留原生文字/键盘输入语义，完整 MPL-2.0 与嵌套 Kira |
| `sena-rs/` | `e4e708be94ddecf99741a1177bede5ed296d21f5` | 通用基线 `5fd1a8e3` 上的 C ABI v3 适配；引擎标识为 SoftPAL、模拟器为 Sena，支持范围不变，完整 MPL-2.0 |
| `ethornell-rs/` | `44801f7901199af208410131db757204981519ce` | fork 内 `MODIFICATIONS.md`，C ABI v3 适配；workspace 声明 MPL-2.0，Release 附 MPL-2.0 全文 |
| `kirikiri2/` | `2532a699b260488db32b32739f9648858c00a9a4` | 完整 Kirikiroid2-web，保留原组件许可、独立生命周期、WindowEx owner 初始化修复，FFmpeg 3.3.9 改为动态链接 |
| `cpal/` | `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` | CPAL 0.15.3 的 WASAPI 关闭与 ALSA 依赖兼容修复，Apache-2.0 |

RFVP、Siglus、Artemis、Sena、Ethornell 和 Kirikiroid2-web 的核心源码统一发布于各 fork 的 `astra`；主仓固定上表的精确 revision。外部核心的嵌入 manifest 统一固定 `2018wzh/AstraEngine@6799938dc33b25bcca4d9a11ddbea1366bf7d93c` 的 C ABI v3。通用修复基线保留在提交历史中，CPAL/Kira 使用按版本命名的稳定分支。

这些目录不是替代 Manager 的应用入口。构建只选择相应 native library，不将其他核心一起初始化或编译作为前置条件。许可与来源细节见 [第三方声明](../THIRD_PARTY_NOTICES.md)。
