# CMVS

当前 Family 插件导出 v7 descriptor 和 typed 配置，复用现有 CPZ5、PS2A VM 与硬件 GPU Scene。153 项普通回归及 1 项显式硬件 GPU 回归通过；格式与 VM 的局部结果不代表真实游戏流程已完成。

脚本切换只接受已解析的名称引用。进程字符串槽缺失、为空、包含多个片段、循环引用或超过八层时，返回 `ASTRA_EMU_CMVS_VM_SCRIPT_NAME` 并使本次执行失败；不再生成哨兵偏移交给适配层猜测目标脚本。正常单片段和有界嵌套引用继续使用原有调用路径。原生菜单向字符串槽写入脚本名的逻辑仍待接入，不能用特定作品的脚本名硬编码代替。

当前接入 CPZ、PB2/PB3/JBP、MGV、PS2A 和系统存档格式层，来源为本地 CMVS 分支提交 `de4110e9e`。格式算法和已有测试保留；大文件按归档索引/头、图片版本、脚本指令/表达式/压缩载荷拆分，不再依赖旧 FamilyCore/Support。错误类型使用 SDK 的 `CoreError`。

VM 和 CMVS 3.90 指令契约已迁入，状态、单步调度和 opcode 表拆开。命令执行入口统一读取和弹出栈参数，再分派到 `vm/execute/` 下的内存、资源、对象、滤镜、演出、纹理、脚本、设置、系统和消息模块；各模块继续检查参数形状并返回原有契约错误，不尝试其他处理器。155 个原有处理分支保持一致，145 项格式与 VM 测试通过。opcode 表另与原始实现逐一比较全部 65536 个输入，契约保持一致。旧 stderr 调试输出改为 tracing 事件，只传位置、状态和计数，不传正文、任意内存值或整体 Debug 对象；日志过滤随宿主订阅器变化。

`execute_cmvs390_frame` 直接修改当前 VM，不再逐指令克隆状态或承诺错误回滚。开始执行前设置失败标志，只有成功返回才清除，因此错误或 panic 后的部分状态不能继续 dispatch，`validate_cmvs390_vm_state` 也拒绝该状态。调用方应结束失败会话，或恢复另一个经过校验的成功快照；不得只重置 `dispatch_stopped` 后重试。尚未开始执行时的等待、停止检查仍可在正常调度恢复后继续。状态结构新增失败标志，旧序列化状态不做迁移；CMVS 尚无已发布的 Family 存档格式。

`CmvsArchive` 已改为核心自有文件访问，复用 SDK 的归档数据类型、读取边界与可选明文缓存。挂载、文件访问和来源校验分模块。`mount_cmvs` 从游戏目录有界读取 `cmvs.profile.json`，typed `CmvsProfile` 使用 `astra.emu.cmvs.profile.v2`。`archives` 是有序 `CmvsArchiveFile { role, path }` 列表，同名脚本查找按列表顺序决定优先级；不再由 role 名称的字母排序决定。`loose_files` 仍按唯一 role 映射相对文件路径。重复归档 role、重复来源、未知 schema、错误参数和未支持的 CPZ 版本明确失败，所有路径必须留在游戏目录。旧 v1 对象式归档配置直接拒绝，应按预期顺序重新编写列表，不自动推断优先级。当前挂载只接受 CPZ5，默认不创建磁盘缓存，不运行旧私有 patch 服务。

`CmvsArchive::load_script` 直接解析核心归档返回的共享字节，不再经流复制整份输入。输入上限仍为 64 MiB，读取与解码成功后才通过 `install_script_frame` 更新入口 PC、脚本身份、名称索引、字符串长度表与初始数据段。frame 限 0–3，非法 frame 或路径在修改 VM 前拒绝。`install_script_data` 在重载时替换稀疏字表，全零或空数据段会删除旧字，其他 frame 不受影响。加载日志只记录 frame 和解码字节数，不记录资源名或正文。帧安装及关联脚本路径的 8 项增量回归通过。

`load_called_script` 经 `resolve_script_uri` 解析调用操作数后走同一加载入口。裸文件名沿用 ASCII 大小写不敏感、按归档挂载顺序首次命中的规则，不受 URI 字母排序影响；显式目录支持游戏使用的两种分隔符，越界路径拒绝。未找到脚本时返回稳定诊断，不附带原始名称。Family session 已调用该入口，按 60 Hz 调度真实 VM，并处理脚本切换和纹理提交。

`CmvsScene` 使用公共 `WgpuOffscreenRenderer` 和 SDK `TextureCache` 合成父/子纹理层，核心自行解码 PB 图片。保留已实现的矩形、位置、层级排序和视口范围规则，GPU 完成合成后回读最终帧；不创建 Engine/VN session。硬件 GPU 回归覆盖层级顺序及同槽纹理尺寸更换。当前 141 项测试通过，其中 GPU 测试需要显式运行 `--include-ignored`。

归档解码结果与短条目缓存使用 Engine 的 `OwnedByteBuffer`。缓存命中只共享所有权，范围读取返回同一分配上的只读视图，流也持有原缓冲；不再在缓存命中后复制完整条目。范围可能延长整条解码数据的生命周期，调用方应及时释放。现有单条目缓存限额与源文件变化检查不变；回归验证共享分配、缓存释放后的读取、流生命周期及源变化拒绝。

CmvsArchive::load_audio 有界读取普通音频，或先校验 MGV 内嵌 Ogg，再调用 SDK 的浮点解码器。最大输入为 64 MiB，输出由调用方帧预算限制；不创建播放 worker，不推断 MGV 视频时间线，CMVS 实际播放仍待接通。

当前可构建 Family 动态库，但还不能完成代表游戏流程。`profile_file` 默认读取 `cmvs.profile.json`，`entry_script` 默认 `start.ps3`。探测检查根目录及原生 `data/pack` 目录的 CPZ5 标头；配置错误、重复会话、未接通动作均返回 Manager 可显示的诊断。关闭与失败释放进程级会话租约，允许重新启动。暂停和恢复清空时钟积累，单次推进受指令及 tick 预算限制。指针移动使用 Host 逆映射后的舞台坐标。

文字、媒体和存档动作尚未接通；命中这些动作后会话失败，后续推进和帧访问拒绝继续，不吞掉动作或伪造成功。只有纹理资源及已有 GPU 合成路径可用，未声明 NativeSave 或 PCM 能力。未移植来源中的特定作品修补、CPU 改帧、猜测字符串和忽略存档请求。原生菜单写入字符串槽与存档恢复等语义仍需独立补齐。

```sh
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs --all-targets -- -D warnings
```

原始游戏文件、密钥配置、解码结果和截图只进入 ignored 私有工作区。

本轮修正了输入轮询的原生命名和行为：case 416 读取确认键的释放锁存与按住状态，重复轮询不消费；case 417 只清除按下、释放锁存。Enter、Space、数字键盘 Enter 和主指针按钮分别跟踪，同一 tick 内的按下/释放不会丢失；失焦或挂起清空输入，不生成推进操作。新增状态只用于核心 VM，既有内部序列化状态不迁移。

case 160/164 原先误命名为 Message，实际把一至两个音频资源交给交替的两个音频通道；161 停止两通道，162 淡出当前通道。现改为 PlayCrossfadeAudio、StopAudio 和 FadeOutAudio，并保留淡出时长；未连接播放时返回 MEDIA_UNBOUND，不再报文字未接通。正文不能从这组命令生成，需继续接入真正的文字表面与 SDK TextScene。该命名修正及输入测试不代表 PCM、正文或代表游戏流程完成。

脚本调用和根脚本替换的动作携带名称来源 frame。VM 先切换 current_frame 后，Family 仍从调用者的脚本池解析名称，不会把相同偏移误用为目标脚本的字符串。全局字符串槽跨脚本复制的所有权仍待补齐，不能据此宣称完整菜单切换已通过。
