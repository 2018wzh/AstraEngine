# Artemis 最小原生适配

上游为 [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core)，固定基线 `0c06f37160961c9ff75d4937d5e6bb0500d0bef9`（0.4.0）。完整 fork 为 [2018wzh/art3m1s-core](https://github.com/2018wzh/art3m1s-core)，submodule 固定到本地单个适配提交 `6b59d145f5d64a5a543d0877459760b969e5f06d`，尚未推送。

## 核心差异

- PFS 索引与查询统一路径分隔符，保留核心原有大小写语义；补充混合分隔符、完整读取和范围读取回归。
- 路径转换测试使用原生 Path 比较，删除 Unix 分隔符假设，生产路径转换不变。
- 根清单声明独立 workspace。可选 RFVP 桥的相邻目录依赖改为精确上游 revision `ec204312e123b4839cec8e69e6237fb0374e5518`，保留原有 feature；AstraEMU 不启用此桥，FVP 仍使用自身固定基线与 Family。
- Vulkan 选择仅接受独显、集显和虚拟 GPU，拒绝 CPU 与未知设备类型；原生 GPU 渲染及平台实现保持不变。
- 增加显式 Vulkan 生命周期测试，覆盖三次创建、帧读回、关闭及重开。
- 原生 FFmpeg 使用与 SDK 相同的 8.x 依赖，复用现有原生视频解码和 GPU 上传路径。自定义 AVIO 按 FFmpeg 的所有权规则释放，初始化错误明确返回。
- 读档在执行 `onLoad` 立即标签之前绑定保存位置的原生输入等待，防止子流程先进入调用后被错误挂起；原生回调结束后恢复保存的消息文本。独立 workspace lockfile 与已有 FFmpeg 8.x 清单同步。
- 原生视频音频提取任务归播放实例所有；停止、替换与关闭先取消并等待线程，再发送音频停止。临时 WAV 由原生 session 持有，Family 打开句柄后删除目录项；尚未消费的文件在 session 关闭时清理。

桌面保留上游 Lua 5.1 与 mlua send；未吸收旧分支为 VN 依赖冲突而引入的 Luau 替换、强制唤醒、修改游戏变量和模拟视频结束逻辑。

## 验证与未完成事项

Windows Vulkan 编译、GPU 生命周期测试、PFS 9 项测试和 Lua 5.1 的 227 项单元测试通过。Clippy 执行成功，上游警告未屏蔽。shaderc 原生构建需要足够短的本地构建目录，测试产物不共享到其他工作树。

Family 已接入独立 Emulator workspace，使用当前 Family API v11、共享 `ProviderModule` 和 Manager 诊断桥。启动配置 `archive` 指定直接 PFS 文件名；只有一个候选时可以留空，多候选时必须显式选择。探测使用原生 PFS 索引和 `system.ini`。

Artemis 原生音频层只维护声音状态并向宿主发命令，没有可嵌入的 PCM mixer。本次适配使用现有 SDK Symphonia 解码入口和 Kira mixer，连接原生 BGM、SE、voice、音量、声像、淡入淡出及 A/B 循环。没有移植旧适配的线性重采样器和私有解码器。PCM 与命令队列有界，关闭和 Drop 都先取消写入，再等待 worker；损坏音频、缺失资源和队列失败结束会话。SDK 原有无条件公开的 `StageCanvas` 已显式依赖 `astra-media-core`，单独启用 audio 不再要求顺带打开 image/text。

测试覆盖 typed 配置、歧义归档、损坏归档的重复启动、损坏音频、阻塞 PCM 的关闭和 Drop、重复 worker 创建与回收。自有最小 PF6 在硬件 Vulkan 上通过三次 Family 启动、推进、挂起/恢复和关闭重开。这些测试没有使用商业剧情，不能视为真实游戏短流程、原生存读档或可听声音验收。

Family 现启用核心已有的 FFmpeg/GPU 视频 session，视频音频进入独立 Kira 通道，不触发普通声音完成通知，不与无 ID 的 BGM 互相停止。窗口标题与鼠标位置、显隐和自动隐藏请求已连接 Host。BGM crossfade 与其他未实现请求仍返回明确诊断；不会伪造视频完成、强制唤醒脚本或修改游戏变量。新增原生取消、join、队列中音频文件生命周期 3 项测试及 FFmpeg MPEG RGBA/YUV 解码 2 项测试通过；原生 all-target Clippy 执行成功，保留上游警告。Family 12 项普通回归和 all-target Clippy 通过，包括阻塞 PCM 关闭重开、视频音频句柄与独立停止命令。新版 Windows 样本在硬件 Vulkan / NullAudio 下完成标题读档直接恢复 CG 与文字、新槽保存、原生退出、重开读取新槽和正文推进。可听音频、完整路线和正式性能仍未验收，Android 未验收。

许可证与归属见 [第三方说明](THIRD_PARTY_NOTICES.md)。
