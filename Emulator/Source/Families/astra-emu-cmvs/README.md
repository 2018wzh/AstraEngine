# CMVS

当前接入 CPZ、PB2/PB3/JBP、MGV、PS2A 和系统存档格式层，来源为本地 CMVS 分支提交 `de4110e9e`。格式算法和已有测试保留；大文件按归档索引/头、图片版本、脚本指令/表达式/压缩载荷拆分，不再依赖旧 FamilyCore/Support。错误类型使用 SDK 的 `CoreError`。

VM 和 CMVS 3.90 指令契约已迁入，状态、单步调度和 opcode 表拆开。命令执行入口统一读取和弹出栈参数，再分派到 `vm/execute/` 下的内存、资源、对象、滤镜、演出、纹理、脚本、设置、系统和消息模块；各模块继续检查参数形状并返回原有契约错误，不尝试其他处理器。155 个原有处理分支保持一致，145 项格式与 VM 测试通过。opcode 表另与原始实现逐一比较全部 65536 个输入，契约保持一致。旧 stderr 调试输出改为 tracing 事件，只传位置、状态和计数，不传正文、任意内存值或整体 Debug 对象；日志过滤随宿主订阅器变化。

`execute_cmvs390_frame` 直接修改当前 VM，不再逐指令克隆状态或承诺错误回滚。开始执行前设置失败标志，只有成功返回才清除，因此错误或 panic 后的部分状态不能继续 dispatch，`validate_cmvs390_vm_state` 也拒绝该状态。调用方应结束失败会话，或恢复另一个经过校验的成功快照；不得只重置 `dispatch_stopped` 后重试。尚未开始执行时的等待、停止检查仍可在正常调度恢复后继续。状态结构新增失败标志，旧序列化状态不做迁移；CMVS 尚无已发布的 Family 存档格式。

`CmvsArchive` 已改为核心自有文件访问，复用 SDK 的归档数据类型、读取边界与可选明文缓存。挂载、文件访问和来源校验分模块。`mount_cmvs` 从游戏目录有界读取 `cmvs.profile.json`，typed `CmvsProfile` 使用 `astra.emu.cmvs.profile.v1`，直接声明 scheme、archive role 和 loose file role；不运行旧私有 patch 服务。路径必须留在游戏目录，重复来源、未知 schema、错误参数和未支持的 CPZ 版本明确失败。当前挂载只接受 CPZ5，默认不创建磁盘缓存。

`CmvsArchive::load_script` 直接从核心归档加载 PS2A，读取与解码成功后才通过 `install_script_frame` 更新入口 PC、脚本身份、名称索引、字符串长度表与初始数据段。frame 限 0–3，非法 frame 或路径在修改 VM 前拒绝。`install_script_data` 在重载时替换稀疏字表，全零或空数据段会删除旧字，其他 frame 不受影响。加载日志只记录 frame 和解码字节数，不记录资源名或正文。帧安装及关联脚本路径的 8 项增量回归通过。

`load_called_script` 经 `resolve_script_uri` 解析调用操作数后走同一加载入口。裸文件名沿用 ASCII 大小写不敏感、按归档挂载顺序首次命中的规则，不受 URI 字母排序影响；显式目录支持游戏使用的两种分隔符，越界路径拒绝。未找到脚本时返回稳定诊断，不附带原始名称。VM 调度与呈现的完整 session 尚未接通。

`CmvsScene` 使用公共 `WgpuOffscreenRenderer` 和 SDK `TextureCache` 合成父/子纹理层，核心自行解码 PB 图片。保留已实现的矩形、位置、层级排序和视口范围规则，GPU 完成合成后回读最终帧；不创建 Engine/VN session。硬件 GPU 回归覆盖层级顺序及同槽纹理尺寸更换。当前 141 项测试通过，其中 GPU 测试需要显式运行 `--include-ignored`。

CmvsArchive::load_audio 有界读取普通音频，或先校验 MGV 内嵌 Ogg，再调用 SDK 的浮点解码器。最大输入为 64 MiB，输出由调用方帧预算限制；不创建播放 worker，不推断 MGV 视频时间线，CMVS 实际播放仍待接通。

这还不是可安装的 Family 插件。Scene 与完整 VM session、媒体、存读档和新 Family API 的连接正在迁移；大型 effect enum 和命令执行分派仍待按职责细分。不导入旧 Runtime provider 或 Host VFS 契约。来源工作树中 provider 的未提交修改保留，待检查日志与错误处理后整合。当前测试不能代替真实 CPZ5 游戏挂载和代表流程。

```sh
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs --all-targets -- -D warnings
```

原始游戏文件、密钥配置、解码结果和截图只进入 ignored 私有工作区。
