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
- Upstream version: `0.6.0`, revision `304e773387a9920c9db091ec1fd937c717aea949`
- Hosted adaptation origin: `f4f64a5bb726c1759350a666a35e0a454b810f61`
- License: Mozilla Public License 2.0.
- Derivative source: <https://github.com/2018wzh/rfvp>
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 独立 Host 重构在 Family 内保留 RFVP 游戏行为，调整最终帧、混合 PCM、输入和生命周期边界。具体修改随 Family 源码记录；旧 Host VFS、effect journal 和统一 snapshot 接口不再作为产品接口。

发布时提供与二进制对应的 MPL-2.0 covered source、修改说明和完整许可证，可随包分发源码或提供有效的 source offer。

当前 vendored source 位于 `Emulator/ThirdParty/rfvp/`，以 RFVP 0.6.0 为基线，hosted 适配源自 fork revision `f4f64a5bb726c1759350a666a35e0a454b810f61`；文件级变化和覆盖范围见该 Family 的 `MODIFICATIONS.md` 与 `THIRD_PARTY_NOTICES.md`。vendored RFVP core 仅作为 private `rlib` 构建，动态边界由 `astra-emu-fvp` 持有。FVP 的四个原始系统字体槽由宿主通过 `fontdb` 从已安装字体按精确 family name 绑定，仓库不再携带替代字体文件。

## WMV decoder

FVP 使用仓库内的 `Emulator/Source/Families/na_wmv_player` 处理 ASF、WMV 和 WMA。该 crate 的跟踪历史缺少明确的来源和许可证说明，源码注释也不足以确定其许可。当前将来源和许可证记为未知，不推断为 LGPL、GPL 或 MIT；已有信息见该 crate 的 `THIRD_PARTY_NOTICES.md`。

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
