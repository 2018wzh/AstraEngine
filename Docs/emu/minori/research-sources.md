# Minori 研究资料索引

这份索引只记录可公开核验的来源。商业资源、解密材料、原版截图和逐帧观察放在 ignored 私有目录；公开仓库不保存副本或本地路径。

## 格式契约

| 优先级 | 来源 | 固定版本或访问日期 | 许可证 | 适用范围 | 当前结论 |
| ---: | --- | --- | --- | --- | --- |
| 1 | [GARbro](https://github.com/morkt/GARbro) `ArcFormats/Musica/ArcPAZ.cs` | `b09ee4570ccb1daf6ac56710ee8934dc0b8baeb0` | MIT | PAZ v0-v2、Blowfish、RC4、zlib、movie 与分卷 | 作为格式 contract；差异仍由授权样本阻断验证 |
| 1 | [GARbro](https://github.com/morkt/GARbro) `ArcFormats/Musica/ArcANI.cs` | 同上 | MIT | ANI frame table、offset、BPP 与 raw pixel layout | 已实现有界纯 Rust container adapter；通用像素 buffer 交给 `image` |
| 1 | [GARbro](https://github.com/morkt/GARbro) `ArcFormats/Musica/ArcSQZ.cs` | 同上 | MIT | SQZ1 index、双倍 frame count、zlib BGRA32 frame | 已实现有界纯 Rust container adapter；严格校验解压大小 |
| 2 | 当前授权样本 | 2026-07-21 | local-private | 八个逻辑 PAZ、18 个物理文件、14502 个 entry | 八包 decoded full verify 已通过；cache identity 复核另有 blocker |
| 3 | 原程序可观察行为 | 尚未形成正式 E3 证据 | local-private | 脚本 VM、系统 UI、输入、存档和演出时序 | GARbro 未覆盖，不从格式 reader 反推语义 |

## 复用组件

| 组件 | 固定版本 | 许可证 | 用途与边界 |
| --- | --- | --- | --- |
| [image](https://docs.rs/image/latest/image/) | workspace lockfile `0.25.10` | MIT OR Apache-2.0 | PNG/JPEG/BMP 和 RGBA buffer；ANI/SQZ 只负责专有 container 解包 |
| [cosmic-text](https://docs.rs/crate/cosmic-text/latest) | workspace lockfile `0.18.2` | MIT OR Apache-2.0 | 日文 shaping、fallback、度量和换行；Minori 不实现私有字体排版器 |
| [Symphonia](https://docs.rs/symphonia/latest/symphonia/) | workspace lockfile `0.6.0` | MPL-2.0 | 音频 demux/decode；混音继续使用 Astra AudioGraph 与 `ProductionAudioMixer` |
| `flate2` | workspace lockfile `1.1.9` | MIT OR Apache-2.0 | PAZ/SQZ zlib；所有输出都受 descriptor 与 host budget 限制 |
| `blowfish` / `rc4` | workspace lockfile `0.10.0` / `0.2.0` | MIT OR Apache-2.0 | 旧 PAZ 兼容；不作为新数据的安全加密方案 |

## 视觉参考

| 来源 | 访问日期 | 归属与可信度 | 允许用途 |
| --- | --- | --- | --- |
| [Kotaku gallery](https://kotaku.com/games/natsuzora-no-perseus/gallery) | 2026-07-21 | 页面标注图片来源为 Minori；辅助级 | 画面比例、消息框位置、人物占屏和色调检查 |

网络截图不能成为 exact checkpoint，也不能证明像素或演出 parity。只有同 build、同输入序列的 Headless artifact 能形成 E2；原版同点截图和 Windows 实机运行另属 E3。
