# Musica Family 插件

Musica 实现当前 Family API，自持 PAZ、脚本 VM、媒体、GPU Scene 和原生存档。Manager 负责安装、配置、输入、最终帧及 PCM 输出，不创建 Engine/VN session。配置在启动时通过 descriptor 校验，诊断经共享日志桥进入 Manager。

## 构建与安装

基础插件不要求 FFmpeg。需要电影播放时显式选择原生依赖：

```sh
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-musica --release --features dynamic-plugin-export,ffmpeg-vcpkg
```

FFmpeg 使用当前工程的 vcpkg 配置；运行时必须能找到匹配的动态库。插件通过 Manager 本地安装入口装入，更新后重启 Manager。无 FFmpeg 构建遇到 movie 指令返回 `ASTRA_EMU_MUSICA_MOVIE_UNAVAILABLE`，不会把首帧或静音当作播放成功。

## 会话与恢复

`probe/open/advance/visit_frame/close` 使用当前 Family 契约。核心每次运行到消息、选择、等待、错误或终点；场景使用共享 GPU renderer，文字使用 AstraText。音频 worker 混合 BGM、SE、voice 和电影 PCM，经同一有界可取消 Host 队列输出。

原生保存包含 VM、场景参数、音频位置和显式电影游标，不包含解码器、GPU 资源或脚本栈。F5/F9 保存与恢复；读档重建资源并丢弃旧请求。关闭取消本会话的解码请求与 PCM 写入，并等待 worker 释放资源，动态库仍由 Manager 驻留。

电影沿用来源全屏舞台约束；窗口挂起暂停电影，Control 仅按 movie 自身标记跳过。电影播放期间暂停剧情和普通演出时钟，结束后解除 Media wait 并继续脚本。

格式、key、资源、opcode、配置及解码错误明确返回，不更换算法或吞掉失败。已有存档读取失败不覆盖原文件。

详细命令见 [脚本执行](script-execution.md)，共享绘制与媒体见 [呈现与媒体](presentation-and-media.md)，实际完成范围见 [实施状态](../../status/implementation-plan.md)。完整游戏结局与平台验收仍开放。
