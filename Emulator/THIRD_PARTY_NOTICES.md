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
- Hosted derivative revision: `f4f64a5bb726c1759350a666a35e0a454b810f61`
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/2018wzh/rfvp>
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 独立 Host 重构在 Family 内保留 RFVP 游戏行为，调整最终帧、混合 PCM、输入和生命周期边界。具体修改随 Family 源码记录；旧 Host VFS、effect journal 和统一 snapshot 接口不再作为产品接口。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

## Noto Sans SC

- Upstream: <https://github.com/google/fonts>
- 所用字体文件随固定版本的 RFVP 源码保留，发布包包含其来源说明与完整许可证。
- License: SIL Open Font License 1.1.
- AstraEMU FVP 使用该字体作为跨平台、可再分发的 CJK compatibility fallback；它不冒充或再分发 Microsoft 字体。完整许可证见 `Engine/Fixtures/PublicDomainFonts/OFL-NotoSansSC.txt`。

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
