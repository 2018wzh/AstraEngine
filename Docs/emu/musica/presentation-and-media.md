# Musica 呈现与媒体

## 场景与文字

Musica Family 自持归档、VM、媒体和原生存档，通过共享 GPU Scene 绘制背景、前景、人物槽、演出、消息面板与文字。逻辑舞台固定 `1280x720`，启动配置显式给出任意正整数 raster 宽高；公共 canvas 计算统一 scale 的居中 aspect-fit viewport，raster 内部和窗口外部均保留黑边，Host 仅对 content viewport 做输入逆映射。资产物理像素不改变 VM、存档或动画几何。`bg` 存放场景资源，`st` 存放立绘，`sys` 存放系统图片；各层和原生参数见 [脚本执行](script-execution.md)。

文字复用 AstraText 的真实字体布局与字形资源，场景使用 GPU EncodedSrgb 合成。SDK TextureCache 管理有界纹理驻留；PNG 和 ANI／SQZ 静态帧复用现有解码器，动画立绘仍待接入。不存在旧 LegacyTextPresentation、Host VFS 或 CPU 产品呈现路径。

## 音频

BGM、SE 和 voice 由 Family 的 Kira worker 混音，解码复用 SDK/Symphonia。 Manager 提供来源实现的 BGM、voice、SE 音量（0–100）及独立静音开关，默认 100 且不静音；se/se2/se3 共用 SE 用户音量。Kira 子音轨应用用户增益，声音本身保留剧情音量和渐变，因此保存与读档不会重复乘用户音量。启动和恢复时按当前配置构建音轨初始增益，静音也继续解码、计时并保留语音等待。PCM 经有界可取消 Host 队列输出；关闭先取消并等待 worker 结束。backlog 回放不推进剧情，角色语音和回放偏好由 Manager 配置，详见 [脚本执行](script-execution.md)。

## 电影接入

来源实现复用 AstraMedia 的 FFmpeg 增量解码，不另写 AVI 容器或 codec。SDK 的可选 `video-ffmpeg` feature 提供 `VideoDecoderWorker`：在所属线程内创建、操作和释放解码器，异步返回初始化结果、有界音视频包批次或 seek 代次。一次只允许一个待完成请求，结果队列容量为一，解码预算沿用 `FfmpegStreamLimits`。每批另有包数和字节上限，worker 至多保留一个暂时装不下的包，seek 丢弃它；单包超过批次字节预算直接失败。Musica 每帧取至多四包，提交前检查 PCM 帧数和包数余量，避免将音视频总吞吐限制在呈现帧率。

关闭会丢弃待处理结果、取消后续请求并等待 worker 释放原生资源；seek 返回新代次，后续包携带同一代次。失败明确返回，不能换 decoder 或退回首帧预览。默认 SDK feature 不要求 FFmpeg；显式选择该 feature 时需要匹配的原生依赖。

`PcmQueue` 复用共享媒体包类型，向核心已有浮点混音缓冲加入 PCM，不新建音频设备。它保留逐包时间戳、限制完整驻留内存、拒绝旧代次和倒退包；重置时释放旧 PCM，缺包时不推进电影音频时钟。暂停和 Host 接受输出后的时钟发布由调用方控制。

Family 通过可选 `ffmpeg-vcpkg` feature 接入完整电影播放：从原生 PAZ 有界读取，调用 SDK worker 增量解码，逐帧交给现有 GPU Scene 全屏绘制。电影 PCM 混入现有 Kira worker 的同一 Host 输出；不新建设备。视频与 PCM 队列分别限额，暂停停止消费，关闭丢弃队列并等待解码器退出。

有音轨时按 Host 已接受的电影 PCM 位置推进；无音轨及音频结束后的视频尾部使用宿主时间。读档关闭旧电影，按已保存的微秒位置重开并 seek；新的 PCM 实例隔离旧结果。Control 按来源的 movie `skippable` 标记停止影片，与消息 Skip/Control 设置独立。未启用 feature 时明确返回 `ASTRA_EMU_MUSICA_MOVIE_UNAVAILABLE`，解码失败不回退。

公共完整音视频样本的 Family GPU 回归覆盖连续画面、非零 PCM、暂停、中途 F5/F9、播放结束、损坏输入、阻塞 PCM 关闭与重复开关。真实游戏电影、长影片吞吐、设备音画同步及各平台运行仍待验证；这些测试不关闭 Musica 结局验收。

当前实施进度见 [实施状态](../../status/implementation-plan.md)。

电影 VM 入口已接入来源命令、Media wait、播放位置及恢复校验，见 [脚本执行](script-execution.md)。VM 仅保存显式游标；解码器、PCM 队列和 GPU 资源不进入存档。Family 现通过显式 FFmpeg feature 重建电影播放，默认无 FFmpeg 构建仍明确拒绝电影事件。
