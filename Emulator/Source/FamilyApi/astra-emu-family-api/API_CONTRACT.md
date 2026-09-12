# Independent Family API v2

`astra-emu-family-api` 是 Family 插件唯一必需的契约依赖。不依赖 RuntimeWorld、package/save、VFS、renderer/audio backend 或 UI。静态核心和动态核心共用 typed DTO；动态入口使用 `abi_stable` 的 `FamilyModule`，提供 descriptor/probe/open/advance/frame/close。

## 启动配置

`FamilyDescriptor.configuration` 是有序 `ConfigField` 列表；字段包含唯一 ASCII ID、UTF-8 label、可为空的显示 group、`ConfigKind` 和必需 default。group 为平面显示分组，不改变 key，也不支持递归。`OpenRequest.configuration` 为 `ConfigEntry` 列表，值类型为 Bool、Integer(i64)、Number(f64)、String 或 Enum。String 与 Enum 不互换，不做字符串到数字的隐式转换。

最多 128 个字段/值，ID 最多 128 bytes，只接受字母、数字和 `._-`；label/group 最多 256 bytes，不允许控制字符；String 上限最多 4096 UTF-8 bytes，不允许 NUL；Enum 最多 128 个唯一选项，选项遵循 ID 规则。Integer/Number 范围含端点，Number 的范围和数值都必须有限。默认值必须符合声明的类型和范围。

`validate_config_schema` 校验完整 schema；`resolve_config` 拒绝未知/重复 key、类型不匹配、无效默认值和越界，再按 schema 顺序补齐缺省值。错误 `ASTRA_EMU_FAMILY_CONFIG` 包含字段/值 index 和原因，不包含用户值。Host 在调用核心前验证，核心仍须调用 `validate_for_descriptor` 和 `resolve_config`。配置只在启动时生效，不能在线修改运行中的核心。配置不授予文件/网络等额外 Host 权限。

Manager SQLite schema 4 按 plugin ID + game ID 保存 typed 值；空 game ID 为核心默认值，游戏覆盖优先，再使用 schema 默认值。每层先验证，未知旧键不会在合并时丢失。Manager 数据版本变化明确重建，不迁移旧 ABI/配置；核心原生存档不受影响。操作见 [配置手册](CONFIGURATION.md)。

## 服务、输入和资源

Descriptor 必须声明 CpuFrame，拒绝重复 capability/format。`PcmAudio` 声明仍要求 Host audio sink，open 返回格式必须与 capability 一致。Family 在任何 write 前用返回的固定格式调用 configure。PCM 为有界可取消队列，设备回调不能调用 Family；格式、channel alignment、chunk 上限和有限 F32 值均验证。

`TextReplacement` 仅表示可选能力：不传 service 为本次关闭翻译，不阻止基础播放。服务使用 typed reset/submit/poll/cancel；poll 为 Pending、Ready、Cancelled 或 Failed。正文缓存不超过最近八段、6000 字符，日志不含正文。FVP 不声明此能力。

Host 只提供 game_path、初始 WindowState 和输入。Family 自持 VM、文件、解码、混音、渲染和原生存档。probe 返回 `ROption<ProbeReport>`，None 是普通未匹配；game_id 为 opaque UTF-8。事件保留键盘、指针、滚轮、文本及窗口顺序。PointerMove 为 letterbox 映射后的游戏帧像素，resize 为物理客户区像素；elapsed_ns 接受包括零在内的 u64，由核心决定 tick。

`FrameView<'a>` 借用 CPU RGBA8 sRGB opaque 像素；`FrameConsumerRef<'_>` 只可在同步 frame 调用内使用。Host 返回前复制 stride × height 字节，不跨 ABI 传 GPU/native handle。

## 生命周期与迁移

ABI fingerprint 为 `astra.emu.independent_family_abi.v2`，schema 为 `astra.emu.independent_family_api.v2`。v1 插件必须重新构建安装，不提供兼容 reader/adapter。

动态库首次加载后一直驻留到进程退出，失败加载产生的 ABI 元数据也不会指向卸载的代码；更新插件必须重启 Manager。provider/module 对象按正常生命周期释放。会话 close 和 open 失败仍须取消 Host 请求、唤醒阻塞 PCM 写入并等待所有 worker 结束，错误不得跳过其他 worker 清理。驻留不允许保留已关闭会话的线程或 callback。

验证使用普通 Rust unit/integration test。配置合法/默认/边界/损坏值、可选翻译和必需音频、Manager 持久化覆盖及失败写入保护均需通过；真实游戏输入、音视频、原生存读档和退出仍须实际运行，局部测试不代表产品验收。
