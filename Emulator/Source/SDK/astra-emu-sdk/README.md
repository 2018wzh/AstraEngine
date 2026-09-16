# AstraEMU SDK

SDK 提供移植核心共同使用的模块，当前替代 Minori 内部的文字资源管理、纹理缓存、归档公共类型和音频解码。CMVS 与 Minori 共用错误、归档、缓存和音频模块；CMVS 的完整 Family session 仍待接通。Family API 独立存在，成熟外部核心无需依赖 SDK。

默认启用 `text`、`image`、`archive` 和 `cache` feature，`audio` 单独选择。仅需要格式错误类型的消费者使用 `default-features = false`；CMVS 选择 `archive/cache/image/audio`，不引入 SDK 文字模块。`CoreError` 保留稳定 code 与说明，不能直接把未经审查的 message 写入日志；核心仍负责产生不含私有数据的错误。

`decode_audio` 复用 Symphonia 和 Kira，将完整单/双声道音频解码为浮点帧。同步调用借用 `&[u8]`，调用方保留归档或容器的字节所有者，返回的 PCM 独立持有采样；不要求将共享缓冲或容器子范围复制为 `Vec`。调用方指定最大帧数并持有取消标志；探测前和包之间检查取消，采样率变化、非有限采样、解码失败或超预算均明确失败。Minori 的旧解码模块已删除，读取资产时保留 `astra-byte-source::OwnedByteBuffer`；CMVS 用同一函数处理普通音频和经核心校验的 MGV 内嵌 Ogg。旧的按值 `Vec` 参数直接改为借用切片，不保留并行解码入口。此模块不创建音频设备或 worker，也不代替流式长媒体播放。

`archive` 复用 `astra-byte-source` 提供本地归档节点、读取结果和索引校验，不提供 Host VFS、registry 或挂载工厂。`ArchiveManifest` 使用 `astra.emu.archive_manifest.v1`，是核心内部索引，不是产品 package。Minori 原来的 `Paz*` 公共读取类型已改为相同的 `Archive*` 类型，不保留别名。

`read_game_profile` 有界读取私有 JSON；相对配置路径以游戏目录为基准，绝对路径仍须位于游戏目录内。`resolve_game_file` 校验相对路径和 canonical 目录归属。错误码使用公共 `ASTRA_EMU_PROFILE_*`，解析失败不回显原文、不写文件。`cache` 复用原 Minori 的有界明文缓存、来源身份校验、淘汰和系统权限实现；是否创建磁盘缓存由核心明确选择。

`TextScene` 使用 `astra-text` 的字体排版和批量字形资源管理；调用方传入字体 provider、完整可见文字区域和颜色/位置，获得生命周期在前、绘制在后的 `SceneCommand`。区域消失时释放字形，共享字形在同一帧统一处理。SDK 不规定消息框坐标、字体或 VM 状态。返回命令须按顺序提交给同一渲染器；渲染失败关闭会话，不丢弃资源命令后继续。

`TextureCache` 复用 `astra-media-core::TextureFrame`、`image` 和 `lru`，同时限制条目数、尺寸和缓存持有的 RGBA 字节数。标准图片解码在转换 RGBA 前检查尺寸与分配预算；专用格式由核心解码后调用 `insert`。淘汰不会使外部持有的帧失效，预算不代表 GPU 或所有调用方的总内存。

SDK 不创建 EngineSession、VN session、PlatformHost 或 package，不拥有游戏存档与归档语义。Minori 直接使用 AstraEngine 的 `WgpuOffscreenRenderer`，SDK 无需再包一层渲染 provider。

```sh
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-sdk
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-minori gpu_text_updates -- --ignored
```

第二项需要硬件 GPU；不能用软件 adapter 的结果作为通过。
