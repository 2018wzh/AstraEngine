# AstraEMU Third-Party Notices

本文件随 AstraEMU desktop/mobile package 分发。依赖的完整许可证文本由 release packaging 从锁定依赖的 license metadata 收集；以下条目记录产品主路径中需要单独保留的来源与归因。

## Slint 1.17.1

- Copyright © SixtyFPS GmbH.
- License mode: Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0.
- License: <https://slint.dev/terms-and-conditions>
- AstraEMU 的顶层 About 页面直接显示 Slint 官方 `AboutSlint` widget。不得从 shipping UI 删除该页面或把它放到无法从顶层导航到达的位置。
- Slint 同时提供其他许可证；AstraEngine workspace 不因本产品选择 Royalty-free 2.0 而改为 GPL。

## rfvp derivative used by astra-emu-fvp

- Upstream: <https://github.com/xmoezzz/rfvp>
- Fixed revision: `3b5ea6c96a925c12f95aef8554905e8fecbc77c3` (`0.5.0` tag)
- License: Mozilla Public License 2.0.
- Astra derivative source: `Emulator/Source/Families/astra-emu-fvp-rfvp-core/`
- Astra wrapper source: `Emulator/Source/Families/astra-emu-fvp/`
- 修改包括 host VFS、bounded deterministic stepping、effect/trace journal、snapshot isolation、fail-fast syscall coverage 与 ABI provider adapter。发布时同时分发 `MODIFICATIONS.md` 与对应 source archive/source offer；更细的文件级历史由 Git 保留。

MPL-2.0 要求的 covered source 以随 release 对应的 AstraEngine source archive 或公开 source offer 提供；release gate 必须把 source archive hash/source offer identity 与 binary/package identity 绑定。

## Noto Sans SC

- Upstream: <https://github.com/google/fonts>
- Fixed revision and file hash: `Engine/Fixtures/PublicDomainFonts/manifest.json`
- License: SIL Open Font License 1.1.
- AstraEMU FVP 使用该字体作为跨平台、可再分发的 CJK compatibility fallback；它不冒充或再分发 Microsoft 字体。完整许可证见 `Engine/Fixtures/PublicDomainFonts/OFL-NotoSansSC.txt`。
