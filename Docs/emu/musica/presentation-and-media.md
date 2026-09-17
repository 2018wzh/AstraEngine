# Musica 呈现与媒体

## 场景与文字

Musica Family 自持归档、VM、媒体和原生存档，通过共享 GPU Scene 绘制背景、前景、人物槽、演出、消息面板与文字。`bg` 存放场景资源，`st` 存放立绘，`sys` 存放系统图片；各层和原生参数见 [脚本执行](script-execution.md)。

文字复用 AstraText 的真实字体布局与字形资源，场景使用 GPU EncodedSrgb 合成。SDK TextureCache 管理有界纹理驻留；PNG 和 ANI／SQZ 静态帧复用现有解码器，动画立绘仍待接入。不存在旧 LegacyTextPresentation、Host VFS 或 CPU 产品呈现路径。

## 音频

BGM、SE 和 voice 由 Family 的 Kira worker 混音，解码复用 SDK/Symphonia。PCM 经有界可取消 Host 队列输出；关闭先取消并等待 worker 结束。backlog 回放不推进剧情，角色语音和回放偏好由 Manager 配置，详见 [脚本执行](script-execution.md)。

## 电影接入

来源实现复用 AstraMedia 的 FFmpeg 增量解码，不另写 AVI 容器或 codec。SDK 的可选 `video-ffmpeg` feature 提供 `VideoDecoderWorker`：在所属线程内创建、操作和释放解码器，异步返回初始化结果、逐包音视频或 seek 代次。一次只允许一个待完成请求，结果队列容量为一，解码预算沿用 `FfmpegStreamLimits`。

关闭会丢弃待处理结果、取消后续请求并等待 worker 释放原生资源；seek 返回新代次，后续包携带同一代次。失败明确返回，不能换 decoder 或退回首帧预览。默认 SDK feature 不要求 FFmpeg；显式选择该 feature 时需要匹配的原生依赖。

完整公共音视频样本解码、时间戳/序号、目标 PCM 格式、seek、待完成请求关闭和非法输入测试已通过。这里只完成解码 worker；Musica Family 的电影会话、GPU 逐帧呈现、PCM 调度、中途恢复和真实游戏验收仍待接入，不能把这些测试计作电影播放完成。

当前实施进度见 [实施状态](../../status/implementation-plan.md)。
