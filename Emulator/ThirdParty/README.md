# AstraEMU 第三方源码

外部核心使用完整 fork 和精确 submodule 提交。每个 fork 的 `astra` 分支是上游基线加一个 AstraEMU 适配提交，适配包、构建入口和修改说明都在这个提交里；主仓不保留外部核心的适配代码，只固定 gitlink 并从 fork 构建、打包。VM、GPU、平台和存档实现继续由原生核心拥有。

plugin ID 等于上游仓库名，适配包命名为 `astra-emu-<上游仓库名>`。

| 来源目录 | 固定 revision | 上游基线 | plugin ID / 适配包 | 差异与许可 |
| --- | --- | --- | --- | --- |
| `rfvp/` | `95d3ee23548cb32e035aaaaeab2d309fc5d3020e` | `3359d8fbea3c53d6d262a9f71b6d4d79888c2368` | `rfvp` / `crates/astra-emu-rfvp` | fork 内 `crates/astra-emu-rfvp/MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`，C ABI v3 移植并删除 D3D12 交付，完整 MPL-2.0 |
| `siglus_rs/` | `04426d0384dd4e048b41fdd5b0508f0b92a77958` | `13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2` | `siglus_rs` / `astra/astra-emu-siglus_rs` | fork 内 `astra/MODIFICATIONS.md` 与 `astra/THIRD_PARTY_NOTICES.md`，保留原生文字/键盘输入语义，完整 MPL-2.0 与嵌套 Kira、eluna_rs 许可 |
| `art3m1s-core/` | `5c83084d55573855713feefa949be24f0ae6baee` | `ad90a62cb192b119b7cf8589aa7f87a94bd747c8` | `art3m1s-core` / `crates/astra-emu-art3m1s-core` | fork 内 `crates/astra-emu-art3m1s-core/MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`，完整 MPL-2.0 及 `crates/pf8/LICENSE` |
| `sena-rs/` | `6693a0d84d6adb0672817c8486111c165279ea46` | `5fd1a8e37600fb917e053f633671560df9ca42a7` | `sena-rs` / `crates/astra-emu-sena-rs` | 引擎标识为 SoftPAL、模拟器为 Sena，支持范围不变，完整 MPL-2.0 |
| `ethornell-rs/` | `e1fd89dbc4d6a2f3b15aa882deea2ade73020e42` | `6550460beb2012781cdaffdf18da59780de422ef` | `ethornell-rs` / `crates/astra-emu-ethornell-rs` | fork 内 `MODIFICATIONS.md`；workspace 声明 MPL-2.0，fork 附 `LICENSE-MPL-2.0` 全文 |
| `kirikiri2/` | `e681590a1075cb4be4e9d41d99cffc2cdba01bc3` | `7105b50822097d11b7b1f5894bf541509fd51350` | `kirikiroid2-web` / `astra/`（C++ 在 `astra/native`） | fork 内 `astra/README.md`；完整 Kirikiroid2-web，保留原组件许可、独立生命周期、WindowEx owner 初始化修复，FFmpeg 3.3.9 改为动态链接 |
| `cpal/` | `ecfda841a9140d60bbec1630d47ca91f1a77b8f3` | — | — | CPAL 0.15.3 的 WASAPI 关闭与 ALSA 依赖兼容修复，Apache-2.0 |

Rust 适配统一以 Git 依赖固定 `2018wzh/AstraEngine@6d9d691dcc8f3d768e40b5be1d6f37366cca62d8` 的 `astra-emu-abi`（C ABI v3），KrKr 的 `astra/native/include/astra_emu.h` 取自同一 revision。通用修复基线保留在提交历史中，CPAL/Kira 使用按版本命名的稳定分支。

这些目录不是替代 Manager 的应用入口，也不是 `Emulator/Cargo.toml` 的 workspace 成员。每个核心在自己的 manifest 上独立构建，命令见[开发手册](../../Docs/manual/development.md#workspace-与常用命令)；Android Manager 以路径依赖静态链接 `rfvp/crates/astra-emu-rfvp`。许可与来源细节见[第三方声明](../THIRD_PARTY_NOTICES.md)。
