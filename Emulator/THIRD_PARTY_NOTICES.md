# AstraEMU Third-Party Notices

本文件随 AstraEMU Windows 应用分发。发布包还需包含活动依赖的完整许可证文本；以下条目记录需要单独保留的来源与归因。

## Slint 1.17.1

- Copyright © SixtyFPS GmbH.
- License mode: Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0.
- License: <https://slint.dev/terms-and-conditions>
- AstraEMU 的顶层 About 页面直接显示 Slint 官方 `AboutSlint` widget。不得从 shipping UI 删除该页面或把它放到无法从顶层导航到达的位置。
- Slint 同时提供其他许可证；AstraEngine workspace 不因本产品选择 Royalty-free 2.0 而改为 GPL。

## rfvp derivative used by astra-emu-fvp

- Upstream: <https://github.com/xmoezzz/rfvp>
- Upstream version: `0.6.0`, revision `af557db750c7833274f22a6d82b0dc33cdea2f85`
- Native adaptation fork: <https://github.com/2018wzh/rfvp>
- Native adaptation branch: `astra`
- 当前 submodule 固定 revision：[`478448ed9d60778f78d5de911fa5717c48cb201c`](https://github.com/2018wzh/rfvp/commit/478448ed9d60778f78d5de911fa5717c48cb201c)，直接父提交为通用原生修复基线 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`。临时分支已按用户授权清理，旧引用另存 Git bundle。
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/2018wzh/rfvp>
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 统一 C ABI 直接使用原生 App、VM、renderer、设备音频与保存。CPU lease 和 D3D12 共享纹理交付位于核心薄嵌入层；主仓 Family 不再实现 Hosted Tick、Host PCM 或另一套游戏执行。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

完整 fork 以 Git submodule 固定在 `Emulator/ThirdParty/rfvp/`。文件级变化与覆盖范围见该 Family 的 `MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`。当前原生字体构造仍嵌入上游四个 MS 命名字体文件；其分发授权未核验，不能沿用旧文档“不分发 Microsoft 字体”的结论，二进制发行保持阻塞。

## KrKr native development bundle

- Upstream: <https://github.com/fenghengzhi/kirikiroid2-web>
- Fixed upstream revision: `7105b50822097d11b7b1f5894bf541509fd51350`。
- 固定 fork revision：[`5b2eccd0a51b6b47bb845f48acfd12bd2167c477`](https://github.com/2018wzh/kirikiroid2-web/commit/5b2eccd0a51b6b47bb845f48acfd12bd2167c477)，发布分支 `astra`。
- 完整 fork 保留独立的 Windows 构建、可重入生命周期、游戏路径缓存与 WindowEx overlay owner 初始化修复。C++ 嵌入适配直接导出统一 ABI，删除 child/IPC；清理后的 Release 由用户人工验收。
- 本地运行包保留完整核心的实际 `LICENSE`、本仓 KrKr 适配说明及构建依赖的 copyright 文件；没有独立许可文本的文件不补造许可证。根许可包含多个组件的实际条款，不用上游 README 的 MIT 概述替代。
- 同目录 VC runtime 与 OpenMP runtime 来自构建工具链。FFmpeg 许可取决于实际配置；当前只作本地主机硬件开发验收，未作整个依赖图的分发许可结论。

## Noto 字体

Musica 编译期嵌入 Noto Sans JP 与 Noto Sans SC，使用 SIL Open Font License 1.1。字体通过仓库 Git LFS 提供，运行包在 `licenses/noto/OFL.txt` 保留相应许可；它们不替代 FVP 请求的原生系统字体。

## WMV decoder

FVP 使用完整 RFVP fork 中的原生 `crates/na_wmv_player` 及原 Movie 路径处理 ASF/WMV/WMA，不再直接依赖主仓 `Emulator/Source/Families/na_wmv_player`。原生 decoder 清单没有独立 license 字段，须保留完整 fork 许可与来源，并在分发前核实实际覆盖范围，不能从 FFmpeg 算法名称推断额外授权。主仓旧独立 decoder/工具的来源和许可不明问题仍保留在其 `THIRD_PARTY_NOTICES.md`，不能混称为当前 FVP 的直接依赖。打包复制当前 Family 的说明与完整 RFVP 许可，不再附加旧 decoder 的错误归因。

## Anime4K

- Upstream: <https://github.com/bloc97/Anime4K>
- Fixed revision: `7684e9586f8dcc738af08a1cdceb024cc184f426`
- Copyright © 2019–2021 bloc97.
- License: MIT.
- 内置 Restore CNN S 与 Upscale CNN x2 S 从该版本的 GLSL shader 独立移植，随 shader 保留完整 MIT 许可证。Magpie format 4 的兼容解析与内建函数独立实现，不包含 Magpie GPL 实现代码。
- 已按固定版本的 [Restore](https://github.com/bloc97/Anime4K/blob/7684e9586f8dcc738af08a1cdceb024cc184f426/glsl/Restore/Anime4K_Restore_CNN_S.glsl) / [Upscale](https://github.com/bloc97/Anime4K/blob/7684e9586f8dcc738af08a1cdceb024cc184f426/glsl/Upscale/Anime4K_Upscale_CNN_x2_S.glsl) 修正移植中的 8×8 调度覆盖、正负激活、Restore 最后一层输入和放大残差采样坐标；权重不变，完整游戏帧保持不透明 alpha。硬件像素回归与独立 CPU 参考比较链式和单独两种效果，覆盖非整组尺寸与图像边缘。

## DirectX Shader Compiler

- Upstream: <https://github.com/microsoft/DirectXShaderCompiler>
- Fixed version: `1.8.2502`.
- 应用的 HLSL 编译路径使用固定版本 DXC；分发编译器时一并保留官方发行包中的许可证和第三方说明。

## 原生音频依赖

- CPAL 0.18.2：<https://github.com/RustAudio/cpal/releases/tag/v0.18.2>，Apache-2.0，Musica 的原生设备 owner 使用该版本；Manager 不再拥有核心 PCM 输出。
- CPAL 0.15.3：`2018wzh/cpal@ecfda841a9140d60bbec1630d47ca91f1a77b8f3`，保留 WASAPI 关闭修复并将 ALSA 依赖对齐到 0.11。完整源码与 Apache-2.0 许可位于 `ThirdParty/cpal`。
- Kira 保留各核心原版本，独立 fork 修复 decoder/device worker 的显式关闭。Musica/Artemis 使用 0.12.2 `7d8961b20bce2ab8fd43b4bb34150d7e2898785c`；RFVP 使用 0.10.8 `d2f10db15d9c451c7b2fd33043ae7732e75bee5d`；Siglus 嵌套 0.9.6 `a299b42690094a71885f6ed97dc504c4038da9a7`；Sena 使用 0.12.1 `523790dbb7ddea5ca432f375a9f3180923f2ea1e`。来源均为 <https://github.com/2018wzh/kira>，保留 MIT/Apache-2.0 文本和修改说明。

## 其他原生核心

- Siglus：`2018wzh/siglus_rs@a94565cccd4f6d5e0a341e5732c5d98e9de2c71f`，通用基线 `13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2`；MPL-2.0，完整源码及嵌套 Kira 位于 `ThirdParty/siglus_rs`。来源与输入边界改动见 Family 的 `MODIFICATIONS.md`。
- Artemis：`2018wzh/art3m1s-core@ae967d8446fe99b97f6e1df93fb1b61d8ada2040`，MPL-2.0，保留原生 Lua 5.1/Vulkan/FFmpeg 与完整来源，见 `ThirdParty/art3m1s-core/LICENSE`。
- Sena：SoftPAL 引擎的模拟器，上游 <https://github.com/xmoezzz/sena-rs>，通用修复基线 `5fd1a8e37600fb917e053f633671560df9ca42a7`，单适配 `2018wzh/sena-rs@8465e2818842b95aeb37424104e3ab95feff777d`。基线包含新会话读档后重新启用保存的修复；完整 MPL-2.0 许可位于 `ThirdParty/sena-rs/LICENSE-MPL-2.0`。支持范围仍为原生及 Steam Koikake，未扩大到其他 SoftPAL。
- Ethornell：上游 <https://github.com/xmoezzz/ethornell-rs> 的 workspace 声明 MPL-2.0，但 crate 未继承且未提供许可文本。适配源码按用户明确授权发布于 [2018wzh/ethornell-rs 的 astra](https://github.com/2018wzh/ethornell-rs/tree/astra)，当前 `e8adc0ca57d163cd9718c157d3f9c106aaf4fde0`；Release 对应的运行代码为 `b6991511b57d8031be14f366732c5f1974937c42`。发布不代表许可覆盖已经澄清，尚未注册为可分发 submodule，不公开发布二进制。
