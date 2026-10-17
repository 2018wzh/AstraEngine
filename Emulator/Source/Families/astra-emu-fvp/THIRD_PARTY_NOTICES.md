# 第三方来源与许可

## RFVP 完整覆盖源码

`Emulator/ThirdParty/rfvp` 为 [`xmoezzz/rfvp`](https://github.com/xmoezzz/rfvp) 的完整 fork，以 Git submodule 固定 `36786d644d988d1c84e129c915cc7bbdaf578ddd`。上游基线为 `af557db750c7833274f22a6d82b0dc33cdea2f85`，crate 版本 0.6.0；当前单适配分支 [`codex/emu-native-single-adaptation-20261018`](https://github.com/2018wzh/rfvp/tree/codex/emu-native-single-adaptation-20261018) 已推送。通用基线与变更分类见 [MODIFICATIONS.md](MODIFICATIONS.md)。

完整 MPL-2.0 许可位于 `../../../ThirdParty/rfvp/LICENSE`。发行时须保留覆盖源码、许可与变更记录，不以旧已发布基线替代当前二进制实际使用的源码。

Family 只提供注册和构建入口；C ABI、原生 owner、CPU lease 与 D3D12 交付均位于 fork 的薄嵌入层，复用原 App、VM、GPU、视频、设备音频和 V1/RFVS 保存。没有 Host advance 或 PCM sink。

## 系统字体

本次原生 App 路径沿用上游 `FontEnumerator::new`，编译期嵌入 `MSGOTHIC.TTF`、`MSMINCHO.TTF`、`MS-PGothic.ttf`、`MS-PMincho-2.ttf`。旧 Family 从系统绑定字体的路径已移除，不能继续宣称二进制不含这些字体。字体的分发授权尚未核验，发行前必须完成授权核验或实际改为系统字体来源；当前只作本地开发验收，不发布二进制。

## WMV 解码来源

当前 FVP 通过 RFVP 的原生 `crates/na_wmv_player` 及其原生 Movie 路径解码、合成和播放音轨。Family 原先直接依赖的主仓 `../na_wmv_player` 与独立 `video.rs` 已移除，不把它们作为当前 FVP 的 decoder 来源。

上游 decoder 清单没有独立 `license` 字段，必须随完整 fork 保留上游许可与来源记录；本记录不将 FFmpeg 算法名称视为复制 FFmpeg 源码或额外许可授权。主仓历史独立 decoder 的来源/许可不明问题不因此消失，也不据此假定 LGPL/GPL/MIT。发布前仍需核实实际覆盖范围。
