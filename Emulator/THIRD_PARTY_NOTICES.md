# AstraEMU Third-Party Notices

本文件随 AstraEMU Windows 应用分发。发布包还需包含活动依赖的完整许可证文本；以下条目记录需要单独保留的来源与归因。

## Slint 1.18.1

- Copyright © SixtyFPS GmbH.
- License mode: Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0.
- License: <https://slint.dev/terms-and-conditions>
- AstraEMU 的顶层 About 页面直接显示 Slint 官方 `AboutSlint` widget。不得从 shipping UI 删除该页面或把它放到无法从顶层导航到达的位置。
- Slint 同时提供其他许可证；AstraEngine workspace 不因本产品选择 Royalty-free 2.0 而改为 GPL。

## mlua 0.10.5

- Upstream: <https://github.com/mlua-rs/mlua/tree/v0.10.5>，MIT；完整许可和源码保存在 `ThirdParty/mlua`。
- 本地回补仅修改 `src/state/raw.rs` 的数字分类：要求浮点数与整数精确相等，并保留 IEEE 负零；不以 epsilon 将极小非零值转成 `0`。修复同时用于严格文件范围和 JSON/TOML 数值转换，Luau 运行时版本、API 与执行队列不变。
- Luau 不区分整数和同值的整值浮点字面量；数据编码保留数值，不保证保留原 TOML 数字字面量的类型拼写。
- 本地包在 `licenses/mlua/LICENSE` 包含此 MIT 许可；固定依赖由 Emulator lockfile 和源码路径复现。

## Luau 663 / luau0-src 0.12.3

- Upstream: <https://github.com/luau-lang/luau>，MIT；Emulator lockfile 固定 `luau0-src 0.12.3+luau663`。
- `luau0-src` 的官方 `LICENSE` 同时保留 Aleksandr Orlenko、Roblox Corporation 和 Lua.org/PUC-Rio 的 MIT 条款，运行包逐字复制为 `licenses/luau/LICENSE`。
- 打包器从 `CARGO_HOME` 下该精确版本的源码读取许可，也可通过 `--luau-license` 指定同一来源；源码许可缺失时打包失败，不生成替代文本。

## aPLib 通用解码

离线补丁库的有界 Rust aPLib 解码按 [apultra 的 expand.c](https://github.com/emmanuel-marty/apultra/blob/master/src/expand.c) token 协议改写，保留 Emmanuel Marty（2019）的 zlib 许可及修改归属，完整文本在 `astra-emu-patch/src/codecs.rs` 中。该通用算法不包含商业游戏资源、密钥或文件偏移；运行包须保留许可文本。

```text
Copyright (C) 2019 Emmanuel Marty

This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

## 静态 ASS 字幕依赖

`astra-subtitle` 使用 <https://github.com/libass/libass> 的 libass 0.17.4#1（ISC），原生依赖安装在独立的 `x64-windows-static-md` vcpkg 树中。RFVP 当前公开原生源码的 manifest 与独立 lockfile 均固定共享实现 `2018wzh/AstraEngine@c27ac6e107328170deecac428bba7472d3ce2d6c`；其 ABI 与 Kira 的独立 pin 保持不变。打包时显式指定该实际安装的 `share/`，逐字保留以下 `copyright` 文件到 `licenses/subtitle/<port>/copyright`：

- libass（ISC）、FreeType（本产品选择 FreeType License）、HarfBuzz（Old MIT 及其附带条款）；
- FriBidi（LGPL-2.1）、Brotli（MIT）、bzip2、libpng、zlib。

本产品使用 FreeType Project（<https://freetype.org/>）的软件，保留其完整许可与归因。闭包包含 FreeType 启用的 Brotli/bzip2/PNG/zlib，不能只附 libass 自身的许可。没有复制 DLL 的静态依赖仍须保留条款。

当前组合只用于本机私有运行包。FriBidi 的静态链接涉及 LGPL 的对应源码及重链接材料等义务；复制许可文本不构成对外分发合规结论。公开二进制分发前须核验实际构建和完整义务，不能把本地包直接上传为公开 Release。


## rfvp derivative used by astra-emu-fvp

- Upstream: <https://github.com/xmoezzz/rfvp>
- Upstream version: `0.6.0`, revision `af557db750c7833274f22a6d82b0dc33cdea2f85`
- Native adaptation fork: <https://github.com/2018wzh/rfvp>
- Native adaptation branch: `astra`
- 当前 submodule 固定 revision：[`ea8b5964fd5b6cde8186eff6a5da1607a3eb6a41`](https://github.com/2018wzh/rfvp/commit/ea8b5964fd5b6cde8186eff6a5da1607a3eb6a41)。历史单适配为 `478448ed9d60778f78d5de911fa5717c48cb201c`，其直接父提交为通用原生修复基线 `3359d8fbea3c53d6d262a9f71b6d4d79888c2368`；旧引用另存 Git bundle，不代表当前构建源码。
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/2018wzh/rfvp>
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 统一 C ABI 直接使用原生 App、VM、renderer、设备音频与保存。CPU lease 和 D3D12 共享纹理交付位于核心薄嵌入层；主仓 Family 不再实现 Hosted Tick、Host PCM 或另一套游戏执行。当前原生 typed 配置支持按原 HCB 偏移索引的 UTF-8 翻译表及 movie/BGM ASS 映射，字幕使用显式字体和原媒体时钟绘入场景，沿暂停、结束、seek/读档与尺寸变化更新。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

完整 fork 以 Git submodule 固定在 `Emulator/ThirdParty/rfvp/`。文件级变化与覆盖范围见该 Family 的 `MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`。当前原生字体构造仍嵌入上游四个 MS 命名字体文件；其分发授权未核验，不能沿用旧文档“不分发 Microsoft 字体”的结论，二进制发行保持阻塞。

## KrKr native development bundle

- Upstream: <https://github.com/fenghengzhi/kirikiroid2-web>
- Fixed upstream revision: `7105b50822097d11b7b1f5894bf541509fd51350`。
- 固定 fork revision：[`5b2eccd0a51b6b47bb845f48acfd12bd2167c477`](https://github.com/2018wzh/kirikiroid2-web/commit/5b2eccd0a51b6b47bb845f48acfd12bd2167c477)，发布分支 `astra`。
- 完整 fork 保留独立的 Windows 构建、可重入生命周期、游戏路径缓存与 WindowEx overlay owner 初始化修复。C++ 嵌入适配直接导出统一 ABI，删除 child/IPC，沿用原生 XP3/PSB/KBAD 与字体路径；当前 revision 未因本轮资源验证改变。Release 的实际游戏流程仍由用户人工验收。
- 本地运行包保留完整核心的实际 `LICENSE`、本仓 KrKr 适配说明及构建依赖的 copyright 文件；没有独立许可文本的文件不补造许可证。根许可包含多个组件的实际条款，不用上游 README 的 MIT 概述替代。
- 同目录 VC runtime 与 OpenMP runtime 来自构建工具链。FFmpeg 许可取决于实际配置；当前只作本地主机硬件开发验收，未作整个依赖图的分发许可结论。

## Noto 字体

Musica 编译期嵌入 Noto Sans JP 与 Noto Sans SC，使用 SIL Open Font License 1.1。字体通过仓库 Git LFS 提供，运行包在 `licenses/noto/OFL.txt` 保留相应许可；它们不替代 FVP 请求的原生系统字体。

## Musica key.toml 解析

`toml 1.1.3+spec-1.1.0` 来源为 <https://github.com/toml-rs/toml>，许可为 MIT OR Apache-2.0；`winnow 1.0.4` 来源为 <https://github.com/winnow-rs/winnow>，许可为 MIT。具体解析、序列化与关联 crate 版本固定于 Emulator lockfile；本地运行包在 `licenses/rust/` 保留本轮新增解析依赖的完整许可文本。

## 异步 GIF 封面

封面缩略图使用 `image` 的 GIF 解码支持：[`gif 0.14.2`](https://crates.io/crates/gif/0.14.2) 与 [`weezl 0.1.12`](https://crates.io/crates/weezl/0.1.12) 为 MIT OR Apache-2.0，关联的 [`color_quant 1.1.0`](https://crates.io/crates/color_quant/1.1.0) 为 MIT。本地运行包在 `licenses/rust/` 保留这些版本的完整许可文本。

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

- Siglus：当前公开 `astra` 与主仓 gitlink 为 [`2018wzh/siglus_rs@23d082a76dfcb071224056858c49abd53f63cbc3`](https://github.com/2018wzh/siglus_rs/commit/23d082a76dfcb071224056858c49abd53f63cbc3)，历史适配为 `a94565cccd4f6d5e0a341e5732c5d98e9de2c71f`、通用历史基线为 `13b55b47484faf8dfe03deb6ffd56ca0ec4ba3f2`；MPL-2.0，完整源码及嵌套 Kira 位于 `ThirdParty/siglus_rs`。当前原生配置支持 original/chs 成对资源、G01/movie 选择及独立保存根，不执行 wrapper 机器码补丁或补造缺失字体。来源与输入边界改动见 Family 的 `MODIFICATIONS.md`。
- Artemis：当前公开 `astra` 与主仓 gitlink 为 [`2018wzh/art3m1s-core@be82d5e0bbafcef55a530f69e5b9358094dbd671`](https://github.com/2018wzh/art3m1s-core/commit/be82d5e0bbafcef55a530f69e5b9358094dbd671)，历史适配为 `ae967d8446fe99b97f6e1df93fb1b61d8ada2040`；MPL-2.0，保留原生 Lua 5.1/Vulkan/FFmpeg 与完整来源，见 `ThirdParty/art3m1s-core/LICENSE`。当前 `extra_archive` 直接消费显式额外 PFS，保留基包及原覆盖顺序。
- Sena：SoftPAL 引擎的模拟器，上游 <https://github.com/xmoezzz/sena-rs>，通用历史基线 `5fd1a8e37600fb917e053f633671560df9ca42a7`、历史单适配 `8465e2818842b95aeb37424104e3ab95feff777d`；当前公开 `astra` 与主仓 gitlink 为 [`2018wzh/sena-rs@277b83a10a7a81e4dce2c1c736b412628028edf1`](https://github.com/2018wzh/sena-rs/commit/277b83a10a7a81e4dce2c1c736b412628028edf1)。基线包含新会话读档后重新启用保存的修复；当前嵌入配置提供 `text_language=jp/cn/tc`，原生选择多语言记录字段并保留原日文单字段行为，字体由原生游戏字体路径或独立原生配置的显式字体文件提供。完整 MPL-2.0 许可位于 `ThirdParty/sena-rs/LICENSE-MPL-2.0`。支持范围仍为原生及 Steam Koikake，未扩大到其他 SoftPAL。
- Ethornell：上游 <https://github.com/xmoezzz/ethornell-rs> 的 workspace 声明 MPL-2.0，但 crate 未继承且未提供许可文本。适配源码按用户明确授权发布于 [2018wzh/ethornell-rs 的 astra](https://github.com/2018wzh/ethornell-rs/tree/astra)，当前公开 revision 为 [`1ebd4337af25437560f1ccff16bab2c6427396f3`](https://github.com/2018wzh/ethornell-rs/commit/1ebd4337af25437560f1ccff16bab2c6427396f3)，当前本地核心从该源码构建；`e8adc0ca57d163cd9718c157d3f9c106aaf4fde0` 与旧 Release 运行代码 `b6991511b57d8031be14f366732c5f1974937c42` 仅作历史记录。当前原生能力包括仅作用于显示文本的 GBK 解码选择和每 runtime 显式字体，不代表未知补丁 handler、字体注册或资源优先级已经闭合。发布不代表许可覆盖已经澄清，源码仍为外部 owned source，尚未注册为可分发 submodule，不公开发布二进制。
