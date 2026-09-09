# AstraEMU 独立 Host 重构

本轮采用同仓独立 emulator host，先完成 Windows、FVP 和现有 Slint Manager。AstraVN、Editor 和 EngineCore 的运行模型不变。此方案取代 AstraEMU 对 RuntimeWorld、StateMachine、product package/save、旧 Family ABI、通用 Hook 与 Trusted Luau patch 的依赖；不保留兼容入口。其他 family 的核心源码保留，但不进入本轮活动依赖图。

## 运行边界

设计参考固定为 [art3m1s](https://github.com/Alphaly2K/art3m1s/tree/3053170d7a5fe174a4c8dab29d7f5152af086219)、[art3m1s-core](https://github.com/Alphaly2K/art3m1s-core/tree/635f19511d31481663c7d6ae5aa5cb96dc9d82e6) 和 [RFVP](https://github.com/xmoezzz/rfvp/tree/469f848ac55755e4628e5b3758da58b563068aa3)。采用清晰的引擎/Host 职责边界与 elapsed 推进方式；保留 RFVP 原生游戏语义。ABI 仍需自行处理借用、回调、取消和动态库生命周期，不能照搬进程全局回调或把参考程序的接口直接当作多 Family 插件规范。

Family 自行解释脚本、读取原生文件、解码、混音、绘制和管理游戏原生存档。Host 只传入游戏位置，不提供 VFS、存档根目录或资源服务。一个进程同时只运行一个游戏 session。窗口焦点、前后台、尺寸和物理输入传给 Family，由 Family 决定游戏行为。

独立 `astra-emu-family-api` 使用 Rust `abi_stable` DTO 与生命周期；第三方只需依赖 ABI，不强制使用 SDK。接口包含 descriptor、probe、open、advance(elapsed)、input/window event、close、CPU 最终帧、独立 PCM 输出和可选异步文本替换。静态与动态插件使用同一接口。旧 ABI binary 显式拒绝。

Family 借出只读最终帧，Host 在调用期间同步复制，随后释放借用；不跨 ABI 传 GPU handle。音频在打开流时固定 sample rate、channel count 和 i16/f32 format。Family 音频 worker 向有界、可取消的 Host 队列阻塞写入；设备转换由 Host 负责，设备实时回调不调用 Family。关闭必须取消写入、停止 worker，再释放 session 和动态库。不可恢复错误关闭游戏并回到资料库，显示可定位的诊断。

## Manager 与插件

保留现有 Slint 视觉与资料库操作，替换内部 host。资料库与设置采用新 schema，明确重建旧数据，不实现旧 schema 迁移。原生游戏存档不属于 Manager 数据。首发随应用提供 FVP，同时允许用户从本地显式安装插件，校验 ABI 与必需 capability。多个 probe 命中时由用户选择，不能静默取第一项。

## 翻译

实现通用 OpenAI-compatible service、配置页与连接测试。FVP 本轮声明不支持文本替换，相关操作禁用，不修改 RFVP 翻译路径，也不以测试 Family 代替真实接入；端到端翻译等待 Minori。

接口为异步正文替换，只挂起当前文字流程，音频继续。默认 timeout 15 秒且可配置；失败或超时显示错误并使用原文，不跳过、不自动重试；退出取消请求。上下文限最近 8 段、总计 6000 字符，说话人与 ruby 可选；新游戏或读档清空。译文只作有界 session cache，读档、新游戏和配置改变时清空。继续沿用游戏字体，由 Family 检查字形和排版，不增加字体覆盖设置。正文、上下文、secret 不进日志或持久化缓存。

## 滤镜

Host 直接处理 Family 最终帧，交付缩放、锐化、Anime4K Restore_S 与 Upscale_S。效果链可加载外部 Magpie 格式 HLSL 并调整参数。兼容范围固定在 Magpie revision `3396e1e000bbab050d098032dac04ba0250683ad` 的 format 4、上述效果所需指令；未知指令或能力显式报错。兼容解析和内建函数独立实现，不复制 Magpie GPL 实现。

内置 Anime4K shader 从 MIT upstream revision `7684e9586f8dcc738af08a1cdceb024cc184f426` 独立移植，保留许可。使用固定 DXC 1.8.2502 和 hassle-rs 0.12 生成 SPIR-V，经 Naga 校验并转为 WGSL，再交给 wgpu 29；不走 unsafe passthrough。RGBA16F read/write storage texture 需要显式 device feature，最终输出 RGBA8 并与 Slint 共用 device。失败的新配置不生效，显示错误并保留之前有效配置。已完成的独立探测只证明编译与 GPU 多 pass 可行，不能替代产品集成和视觉测试。

## 实施顺序

1. 固定上述共享边界并更新实施规则；建立独占重构分支。
2. 替换独立 ABI，重写 RFVP adapter，打通 frame/audio/input/native save 最小端到端路径。
3. 简化 Manager core、插件加载、资料库与翻译服务；迁移 Slint host 并移除旧依赖路径。
4. 接入 HLSL 效果链和内置 Anime4K；完成参数、错误与切换行为。
5. 更新 workspace、文档与 schema，删除过时程序和中间文件；执行增量回归和提交前检查。
6. 在授权 Windows Sandbox 中用《樱花萌放》完成任一角色结局，可使用 Ctrl 快进；检查系统页、音视频、原生存读档、退出和冷启动。将独立游戏副本以可写目录挂载给 guest，原始游戏目录保持不变。不测试 FVP 翻译。

## 交付边界

代码任务由 Luna 在独立 worktree 完成，主任务负责设计、审阅和集成。各 worktree 独享 target，遇到构建锁等待。按普通软件开发方式运行测试，不建立新的 evidence/report 留存体系；临时游戏内容、截图、日志和编译探测产物完成后清理。代码通过相关检查后提交到重构分支，不自动合并或发布。未通过的检查与未完成的真实游戏流程如实记录为未完成。
