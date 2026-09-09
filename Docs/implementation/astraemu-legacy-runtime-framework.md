# AstraEMU 独立 Host 实现结构

当前结构由 [ADR 0019](../adr/0019-astraemu-independent-host.md) 和 [Family 契约](../contracts/astraemu-ipc.md) 定义。AstraEMU 不再实现 RuntimeWorld/StateMachine gameplay adapter。

## 模块职责

| 模块 | 职责 |
| --- | --- |
| astra-emu-family-api | 独立 `abi_stable` descriptor、probe、session、输入/窗口事件、借用帧、PCM、异步文本替换 |
| astra-emu-fvp | RFVP 核心接入、原生文件、媒体、最终帧和原生存档 |
| astra-emu-manager-core | 资料库、新 schema、配置、插件安装与选择、session 管理 |
| astra-emu-manager-ui-slint | 资料库布局、只读 ViewModel、用户操作回调 |
| astra-emu-manager | 窗口/event loop、输入、音频设备、wgpu 最终帧与 HLSL 效果链 |
| astra-emu-translation-openai-compatible | 异步连接/翻译、timeout/cancel、有限上下文与 session cache |
| astra-emu-metadata | 现有 VNDB/Bangumi 元数据和游玩状态服务 |

## 执行与退出

Manager 显式选择唯一 Family，传游戏位置打开 session，按 elapsed 与物理输入推进。Family 借出最终帧，Host 同步复制并上传；Family 音频 worker 阻塞写入独立的有界可取消 PCM 队列，Host 负责设备转换。关闭先取消请求与阻塞写入，等待 worker，再销毁 session 和动态库。

FVP 不提供翻译 capability。其后接入的 Family 可以使用异步文本替换，只挂起当前文字流程，失败保留原文并显示诊断。Host 不提供游戏 VFS、原生存档服务、通用 Hook 或 Trusted Luau patch。

## 依赖与测试

旧 Family/Extension ABI、runtime provider、evidence 程序与 Host VFS 不进入活动依赖图。其他 family core 保留为后续源码，不因首轮 FVP 而删除研究资料。没有第二消费者时不新建 SDK 或工具 crate。

具体实施顺序、滤镜版本、错误与测试范围见 [实施方案](../migrations/astraemu-independent-host.md)，进度见 [Stage 5](../status/stages/stage-5-astra-emu.md)。
