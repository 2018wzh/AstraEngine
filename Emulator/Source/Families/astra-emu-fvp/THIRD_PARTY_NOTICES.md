# 第三方来源与许可

## RFVP 完整覆盖源码

`Emulator/ThirdParty/rfvp` 为 [`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp) 的完整 fork，以 Git submodule 固定精确提交。当前上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`，crate 版本 0.6.0；适配分支 [`codex/astra-minimal-native-20261017`](https://github.com/2018wzh/rfvp/tree/codex/astra-minimal-native-20261017) 已推送至 GitHub fork。精确适配提交与变更分类见 [MODIFICATIONS.md](MODIFICATIONS.md)。

完整 MPL-2.0 许可位于 `../../../ThirdParty/rfvp/LICENSE`。发行时须保留覆盖源码、许可与变更记录，不以旧已发布基线替代当前二进制实际使用的源码。

Family 是独立动态 ABI 边界，使用核心 `hosted-gpu` feature，复用原 VM、GPU、视频/音轨和原生 V1 保存。Astra 专用命令收集和 CPU lease 留在 Family，不向核心添加 Astra Runtime 依赖。

## 系统字体

原生四个系统 slot 的字体来自宿主安装的 MS Gothic、MS Mincho、MS PGothic、MS PMincho。Family 传入实际字体字节与 TTC face index，不分发 Microsoft 字体或替代字体。MS Gothic 为启动要求；其他 slot 沿用核心原生 optional-slot fallback 行为。

## WMV 解码来源

当前 FVP 通过 RFVP 的原生 `crates/na_wmv_player` 及其原生 Movie 路径解码、合成和播放音轨。Family 原先直接依赖的主仓 `../na_wmv_player` 与独立 `video.rs` 已移除，不把它们作为当前 FVP 的 decoder 来源。

上游 decoder 清单没有独立 `license` 字段，必须随完整 fork 保留上游许可与来源记录；本记录不将 FFmpeg 算法名称视为复制 FFmpeg 源码或额外许可授权。主仓历史独立 decoder 的来源/许可不明问题不因此消失，也不据此假定 LGPL/GPL/MIT。发布前仍需核实实际覆盖范围。
