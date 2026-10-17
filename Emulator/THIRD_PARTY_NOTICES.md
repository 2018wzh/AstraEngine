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
- Hosted adaptation fork: <https://github.com/2018wzh/rfvp>
- Hosted adaptation branch: `codex/astra-minimal-native-20261017`
- 当前 submodule 固定 revision：[`dcd39c23ca59b38a9a96b6f301aeb05121aa3e18`](https://github.com/2018wzh/rfvp/commit/dcd39c23ca59b38a9a96b6f301aeb05121aa3e18)，直接基于上述上游 HEAD 的一个适配提交，已推送至上述 fork 分支。
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/2018wzh/rfvp>
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 独立 Host 重构在 Family 内保留 RFVP 游戏行为，调整最终帧、混合 PCM、输入和生命周期边界。具体修改随 Family 源码记录；旧 Host VFS、effect journal 和统一 snapshot 接口不再作为产品接口。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

完整 fork 以 Git submodule 固定在 `Emulator/ThirdParty/rfvp/`；旧已发布基线不能代替当前二进制实际使用的 covered source。匹配当前 gitlink 的适配源码已推送 GitHub，二进制分发仍须提供对应源码、许可与修改记录。文件级变化与覆盖范围见该 Family 的 `MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`。Family 以 rlib 链接核心，产品动态 ABI 边界由 `astra-emu-fvp` 持有；核心原有构建 targets 保留。原生四个系统字体槽由宿主通过 `fontdb` 按实际 family name 与 TTC index 绑定，不分发 Microsoft 字体，optional slot 沿用原生 fallback。

## KrKr native development bundle

- Upstream: <https://github.com/fenghengzhi/kirikiroid2-web>
- Fixed upstream revision: `7105b50822097d11b7b1f5894bf541509fd51350`。
- Local adaptation revision: `8bf8896dfaed4d1d956e7b8944ed643df848a26c`；尚未推送。
- 原生核心保留完整 fork；本地适配整理为该上游提交的单个子提交，来源分支与历史保持不变。当前尚未完成原生游戏流程，不将开发运行包记为已验收或已发布核心。
- 运行包保留核心实际 `LICENSE`、Astra 适配的 BSD-3-Clause `LICENSE.astra`、`MODIFICATIONS.md`，以及固定安装依赖的 copyright 文件。根许可包含多个组件的实际条款，不用上游 README 的 MIT 概述替代。
- 同目录 VC runtime 与 OpenMP runtime 来自构建工具链。FFmpeg 许可取决于实际配置；本批只作本地开发和 Sandbox 测试，未作整个依赖图的分发许可结论。

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

## DirectX Shader Compiler

- Upstream: <https://github.com/microsoft/DirectXShaderCompiler>
- Fixed version: `1.8.2502`.
- 应用的 HLSL 编译路径使用固定版本 DXC；分发编译器时一并保留官方发行包中的许可证和第三方说明。

## CPAL 0.18.2

- Source: <https://github.com/RustAudio/cpal/releases/tag/v0.18.2>
- License: Apache-2.0; minimum Rust version: 1.85.
- Manager 使用 crates.io 的正式版本与锁文件 checksum。Windows WASAPI 播放进度读取该版本的设备 callback/playback 时间戳；其他后端仍可基础播放，但未提供相同精度的进度声明。发布包须包含完整 Apache-2.0 许可证。
