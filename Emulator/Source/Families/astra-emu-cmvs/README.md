# CMVS

当前 Family 插件使用公共 v13 descriptor/CPU lease 和 typed 配置，复用现有 CPZ5、PS2A VM 与硬件 GPU Scene。VM、格式、归档和原生存档语义由核心持有，不创建 Engine/VN session；下文原有平台检查记录保留执行时的范围。

Scene 已接入 SDK `StageCanvas`：逻辑舞台与 raster 分开，窗口 resize 只调整渲染目标，Family 帧报告逻辑与物理尺寸。Provider 跨 session 持有原生 `wgpu::Instance`，Scene 保持同一 Instance 存活，并释放每个 session 自身的 renderer/device/纹理资源。来源的 quad activation/live 查询与 scheduler rebuild gate 已迁入。Linux 定向检查包含 216 项 CPU、2 项 SDK StageCanvas 与 2 项 Tesla P4 Vulkan GPU 回归，以及 all-target Clippy；GPU 组覆盖 32 次 renderer 重建、resize/drop 和失败候选回收。

session 音频动作仍返回 `ASTRA_EMU_CMVS_MEDIA_UNBOUND`，open 的 `audio_format` 为 `RNone`。该迁移不代表可听 PCM、完整 session 媒体与恢复、真实游戏流程或第六核心已经完成。完整状态见[实施计划](../../../../Docs/status/implementation-plan.md)。

脚本和纹理名称在执行命令时从当前脚本池或原生全局字符串槽解析并复制。全局表统一为 128 个独立 Shift-JIS 值，表达式拼接和 case 148 区间赋值均立即复制；后续槽位修改、frame 切换或根脚本替换不会改变已经复制的值。旧嵌套引用表和八层引用解析已删除，不猜测脚本名。

当前接入 CPZ、PB2/PB3/JBP、MGV、PS2A 和系统存档格式层，来源为本地 CMVS 分支提交 `de4110e9e`。格式算法和已有测试保留；大文件按归档索引/头、图片版本、脚本指令/表达式/压缩载荷拆分，不再依赖旧 FamilyCore/Support。错误类型使用 SDK 的 `CoreError`。

VM 和 CMVS 3.90 指令契约已迁入，状态、单步调度和 opcode 表拆开。命令执行入口统一读取和弹出栈参数，再分派到 `vm/execute/` 下的内存、资源、对象、滤镜、演出、纹理、脚本、设置、系统和消息模块；各模块继续检查参数形状并返回原有契约错误，不尝试其他处理器。155 个原有处理分支保持一致，145 项格式与 VM 测试通过。opcode 表另与原始实现逐一比较全部 65536 个输入，契约保持一致。旧 stderr 调试输出改为 tracing 事件，只传位置、状态和计数，不传正文、任意内存值或整体 Debug 对象；日志过滤随宿主订阅器变化。

`execute_cmvs390_frame` 直接修改当前 VM，不再逐指令克隆状态或承诺错误回滚。开始执行前设置失败标志，只有成功返回才清除，因此错误或 panic 后的部分状态不能继续 dispatch，`validate_cmvs390_vm_state` 也拒绝该状态。调用方应结束失败会话，或恢复另一个经过校验的成功快照；不得只重置 `dispatch_stopped` 后重试。尚未开始执行时的等待、停止检查仍可在正常调度恢复后继续。状态结构新增失败标志，旧序列化状态不做迁移；CMVS 尚无已发布的 Family 存档格式。

`CmvsArchive` 已改为核心自有文件访问，复用 SDK 的归档数据类型、读取边界与可选明文缓存。挂载、文件访问和来源校验分模块。`mount_cmvs` 从游戏目录有界读取 `cmvs.profile.json`，typed `CmvsProfile` 使用 `astra.emu.cmvs.profile.v2`。`archives` 是有序 `CmvsArchiveFile { role, path }` 列表，用于挂载资源；脚本选择由原生 Script 搜索域决定，不把 profile 列表当运行时搜索表。`loose_files` 仍按唯一 role 映射相对文件路径。重复归档 role、重复来源、未知 schema、错误参数和未支持的 CPZ 版本明确失败，所有路径必须留在游戏目录。旧 v1 对象式归档配置直接拒绝，应按预期顺序重新编写列表，不自动推断优先级。当前挂载只接受 CPZ5，默认不创建磁盘缓存，不运行旧私有 patch 服务。

`CmvsArchive::load_script` 直接解析核心归档返回的共享字节，不再经流复制整份输入。输入上限仍为 64 MiB，读取与解码成功后才通过 `install_script_frame` 更新入口 PC、脚本身份、名称索引、字符串长度表与初始数据段。frame 限 0–3，非法 frame 或路径在修改 VM 前拒绝。`install_script_data` 在重载时替换稀疏字表，全零或空数据段会删除旧字，其他 frame 不受影响。加载日志只记录 frame 和解码字节数，不记录资源名或正文。帧安装及关联脚本路径的 8 项增量回归通过。

入口先读取原生 `cmvs.cfg` 的 `CMVS_SYSTEM_MAIN.SCRIPT_INIT_PATH` 建立 Script slot 0；boot 与后续调用统一走 `load_native_script`，只查找原生 Script 搜索域，纹理只查找 Texture 域。旧 profile 全归档入口扫描已删除，没有兼容 fallback。缺省空路径按原版指向游戏根；已配置但缺失或损坏的源不能由同名归档替换。case 16/17/18/19/20/23/24/26 分属八个独立表，配置指令立即复制路径，不再按槽数合并，也不生成伪音频动作。原生脚本与交替音频读取槽 0–3，纹理读取槽 0–9；配置函数允许但读取器不使用的槽不会进入搜索。未恢复用途的表保留独立的原生字段身份，读请求明确拒绝。

查找按该域的槽号顺序进行。末尾带分隔符的路径及空路径指向游戏目录内的文件，其余路径必须对应私有配置中已挂载的归档。缺文件可以继续下一槽，读取错误、越界路径及存在但未配置的归档明确失败；不会转而扫描其他域或全部归档。归档内同名文件保持原生索引顺序，资源 URI 带归档 role，允许不同归档包含相同相对名称。读取日志和错误不回显路径。新增有界原生文件读取供脚本、PB 图像及已有音频解码入口共用，不写入商业源。Family 保持 60 Hz VM 调度与真实纹理提交。

`CmvsScene` 使用公共 `WgpuOffscreenRenderer` 和 SDK `TextureCache` 合成父/子纹理层，核心自行解码 PB 图片。保留已实现的矩形、位置、层级排序和视口范围规则，GPU 完成合成后回读最终帧；不创建 Engine/VN session。硬件 GPU 回归覆盖层级顺序及同槽纹理尺寸更换。当前 141 项测试通过，其中 GPU 测试需要显式运行 `--include-ignored`。

归档解码结果与短条目缓存使用 Engine 的 `OwnedByteBuffer`。缓存命中只共享所有权，范围读取返回同一分配上的只读视图，流也持有原缓冲；不再在缓存命中后复制完整条目。范围可能延长整条解码数据的生命周期，调用方应及时释放。现有单条目缓存限额与源文件变化检查不变；回归验证共享分配、缓存释放后的读取、流生命周期及源变化拒绝。

CmvsArchive::load_audio 有界读取普通音频，或先校验 MGV 内嵌 Ogg，再调用 SDK 的浮点解码器。最大输入为 64 MiB，输出由调用方帧预算限制；不创建播放 worker，不推断 MGV 视频时间线，CMVS 实际播放仍待接通。

当前可构建 Family 动态库，但还不能完成代表游戏流程。`profile_file` 默认读取 `cmvs.profile.json`，`entry_script` 默认 `start.ps3`。探测检查根目录及原生 `data/pack` 目录的 CPZ5 标头；配置错误、重复会话、未接通动作均返回 Manager 可显示的诊断。关闭与失败释放进程级会话租约，允许重新启动。暂停和恢复清空时钟积累，单次推进受指令及 tick 预算限制。指针移动使用 Host 逆映射后的舞台坐标。

文字、媒体和存档动作尚未接通；命中这些动作后会话失败，后续推进和帧访问拒绝继续，不吞掉动作或伪造成功。只有纹理资源及已有 GPU 合成路径可用，未声明 NativeSave 或 PCM 能力。未移植来源中的特定作品修补、CPU 改帧、猜测字符串和忽略存档请求。原生菜单、正文演出与完整存档恢复仍需实际接通。

```sh
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-cmvs --all-targets -- -D warnings
```

原始游戏文件、密钥配置、解码结果和截图只进入 ignored 私有工作区。

本轮修正了输入轮询的原生命名和行为：case 416 读取确认键的释放锁存与按住状态，重复轮询不消费；case 417 只清除按下、释放锁存。Enter、Space、数字键盘 Enter 和主指针按钮分别跟踪，同一 tick 内的按下/释放不会丢失；失焦或挂起清空输入，不生成推进操作。新增状态只用于核心 VM，既有内部序列化状态不迁移。

case 160/164 原先误命名为 Message，实际把一至两个音频资源交给交替的两个音频通道；161 停止两通道，162 淡出当前通道。现改为 PlayCrossfadeAudio、StopAudio 和 FadeOutAudio，并保留淡出时长；未连接播放时返回 MEDIA_UNBOUND，不再报文字未接通。正文不能从这组命令生成，需继续接入真正的文字表面与 SDK TextScene。该命名修正及输入测试不代表 PCM、正文或代表游戏流程完成。

脚本安装绑定真实字符串池缓存；缓存不进入 VM 快照，恢复后必须从经校验的脚本重新绑定，缺缓存立即拒绝，不能误用其他 frame。调用和根替换动作携带已经复制的名称，因此不再需要 Family 延后按 frame 解析。

内部 VM 格式升为 v8，版本字段先于其余字段解析，旧快照直接拒绝，无转换或覆盖。全局字符串值属于核心私有游戏状态，可随私有快照恢复，不写日志、公开报告或包。原生槽容量为 1024 字节（含 NUL）；实现对超长、内嵌 NUL 和非法 Shift-JIS 拒绝写入并保留槽值，不模拟原版不安全的缓冲区溢出，也不截断。CMVS 仍未声明 NativeSave，不能把内部快照回归视为原版存档恢复通过。

case 751 也已纠正：读取 skip/auto 输入模式及 interpreter override，默认返回 false，不再把同步纹理解码当作恒为 true 的跳读信号。测试覆盖默认、skip、auto 与 override；Family 尚未提供 skip/auto 设置操作，不能据此宣称完整自动播放已接通。

原生 tag `0x40` 字符串从当前脚本的可写 data segment 读取，不能与 tag `0x00` 的只读字符串池混用。读取包含段尾不足一个 dword 的实际字节，要求 NUL 终止符位于段内；拷贝到全局槽后不再受 data segment 修改影响。未恢复的栈字符串域 `0xC0` 仍明确拒绝。


资源域及值所有权回归覆盖跨脚本切换、槽位替换、快照恢复、目录与归档优先级、损坏路径和未配置来源。169 项普通回归通过；本批未运行 GPU 测试。真实私有启动已执行资源配置，随后在 case 22 的保存目录请求处明确停止；尚未完成 CMVS 代表游戏流程。

case 21/22 的路径值现在在命令执行时立即复制，保存为相对路径和符号化前缀，切脚本或修改全局字符串槽不会改变已配置路径。case 22 接入原生 mode 0 的游戏根目录；按原版仅创建以分隔符结束的前缀，末段没有分隔符时不创建该段，不创建或覆盖存档文件。每层现有目录必须解析到游戏根内，越界、文件占位及未知前缀明确失败。Documents/AppData 保存根选择尚未接入，不隐式查找或迁移。内部快照 v8 拒绝旧结构。

本批 173 项普通测试通过，另 1 项 GPU 测试未执行；Clippy、fmt 和文档检查通过。私有 Family 启动仍耗时在挂载与源校验，尚未获得首 tick 结果，不能据此说明 opcode 22 之后的真实流程已通过。

原生文字对象已加入核心私有状态：case 256 创建独立的 12 槽文字对象，并复用对应字体 owner；257 销毁文字对象但保留字体 owner；259/260 设置范围及清空后光标；261/262/263/264/268 分别处理字号与间距、字体、双颜色、禁则与光标。参数遵守原生有符号保留值和颜色高字节规则，字符串立即复制，快照恢复校验对象与字体归属。

267 静态文字已接 Family 内的 SDK TextScene：原生 parser 处理控制符、换行、禁则及仅绘制 base 的静态 ruby，字符记录保留绘制时的字体、颜色和位置，后续命令不改写已生成文字。940 选择固定或比例前进量；默认遵守原生非零初值，比例模式从同一 Cosmic provider/cache 读取真实 glyph advance。所有字符由单个 TextScene 批量提交，GPU 复用 GlyphMask；原生 1/2 描边使用最大覆盖核，275 的纵向、横向和对角渐变使用 mesh 顶点颜色。258 单独设置整个文字对象的位置偏移。内部 VM 格式为 v8，旧快照拒绝；这仍不是原版 NativeSave 实现。

字体名称从本机 font database 精确匹配，字体覆盖或 shaping 失败会停止会话，不代换 Noto 或系统 generic 字体。空字体名使用原生启动配置：读取 `initial.cfg` 的 `CMVS_INIT_CFG.CONFIG_PUT/REG`，模式 1 读取 Windows HKCU `Software/<REG>/param` 的 `MI_Font`，模式 3 先读取同一根的 `ConfigPut` 决定模式；未配置值时读取 `cmvs.cfg` 的 `MESSAGE_INFO.FONT`，空值保留原生默认“ＭＳ ゴシック”。模式 2 选择保存根内的配置，当前仅支持保存根 mode 0；Documents/AppData 根仍明确未绑定。Registry 读取限定在 CMVS Windows 模块，其他平台遇到 registry 模式明确失败。整个读取过程不创建 registry key，也不写回原配置。

INI 使用精确锁定的 `configparser 3.3.0`（MIT 许可选项），在核心先严格解码 CP932 或带 BOM 的 UTF-16LE。读取限制为 1 MiB，配置文件必须位于授权游戏根内；损坏、越界或过长字体名明确失败。只读授权脚本已确认实际创建命令使用空字体名；真实字体显示仍须经过原脚本的 Family/GPU 流程。266 实时文字 parser 及其他未恢复命令继续明确拒绝。

本批 CPU 回归覆盖固定/比例度量、静态控制符与 ruby、禁则、原生描边和渐变、文字变换、资源上传/复用/释放及错误原子性。真实 Family 首 tick、文字画面和声音仍未验收；CPU 字形与 mesh 回归不代表 GPU 产品流程通过。

CMVS 普通挂载现在只读取 CPZ header/index，校验索引身份、角色唯一性、相对路径、资源范围及源大小/修改时间。它不再为了未使用的 manifest 在首 tick 前遍历全部归档和 loose payload。只读私有启动的分段计时确认索引读取约 0.01 秒，而单个归档的全量 hash 耗时 7–34 秒，120 秒时限内尚未进入首 tick；这次失败不是字体显示验证。

`CmvsArchive::manifest()` 改为返回 `Result<ArchiveManifest, CoreError>`，请求时才计算真实内容身份；持久 plaintext cache 仅在实际读取需要缓存的条目时计算其来源身份。默认播放没有持久 cache，脚本 URI 校验直接使用 mount prefix。首次 hash 建立内容基线，后续 `validate_sources()` 重新读取并比较；源大小或修改时间变化仍在每次资源读取前后拒绝。不能把延迟内容身份理解为挂载时已验证全部 payload，未知资源损坏仍在使用时明确失败。SDK manifest 类型及其他 Family 保持不变。

懒内容身份改动后的同一私有游戏 Family 启动已实际运行：12 个 CPZ 的索引在启动后 0.14–0.30 秒完成，1.85 秒创建 DX12 独显设备，1.89 秒打开 session；首次 `advance` 在 1.90 秒因 command 691 返回 `ASTRA_EMU_CMVS_ACTION_UNBOUND`，2.02 秒关闭 session 并释放进程。挂载阻断已跨过，但首 tick 尚未完成，也没有执行到可确认的 267、捕获帧或文字显示；本次未提供音频 sink，不能作为声音测试。

原生 691 已更正为只读存档槽扫描，而非窗口布局。参数负值取 10，大于 256 取 256；实际启动脚本传入 10。核心按已配置保存路径读取 `saveNNN.dat` 最多 512 字节，缺文件或非 CSV2–CSV9 签名视为空槽；合法签名但标题/日期截断、路径越界或读取失败明确拒绝。标题中的原生日期字符按原版 32 位公式比较，不按文件修改时间排序；两秒内时间相同保留较小槽号。

扫描先构建完整 catalog，再一次替换核心状态；失败保留原 catalog，整个过程不创建或改写存档。typed catalog 保存标题、占用和最新槽，提供保存/读取启用与选择状态；它进入核心私有 VM v8 快照，不进入日志。Windows 原生菜单 UI、原版存读档 codec 和实际菜单交互仍未接入，槽目录可读不等于存档可加载。本批有界真实 GPU 重试尚未执行。

692 已更正为 `SetNativeSaveEnabled`，对应原生 `4959A0` 的保存许可和菜单开关，不再产生错误的 GPU presentation action。命令同步写入 1596 与 15064；750 读取 1596，原生自动保存 gate 要求两者均非零。初始化保留原版 1596=1、15064=0，因而最初 750 可为真而保存 gate 仍关闭。许可直接保存在现有私有 VM 字段中，不在 slot catalog 复制派生布尔状态；快照恢复后使用同一 gate。

此实现只恢复许可与查询语义，Windows 菜单交互及实际自动保存仍未接入，不创建空文件或将未实现保存动作当作成功。先前错误 CPZ 入口的实际脚本运行已跨过 691/692：索引约 0.19 秒，DX12 session 约 1.54 秒打开，首次 advance 在 296 返回 `STORAGE_UNBOUND`，约 1.68 秒关闭；仍未完成首 tick、正文、PCM 或输入。


296 `LoadSystemState` 接入只读 `system.dat` 原生容器：完整校验长度、版本、身份、校验和、cipher/LZSS 及字符串边界后，才应用 flags、CG、BGM 和 64 个标题槽。BGM 写入原生 float-word 全局区，标题写入现有 string slots，不保留第二份存档旁路副本。两组原生列表保留 CP932 字节、原顺序和原版 ASCII/Shift-JIS 去重；非零 count 先清空再按各自 enable gate 加入，零 count 保留当前列表。缺文件保留当前 globals，不创建文件；损坏文件和已有错误状态明确失败，不改写文件或部分更新 VM。私有 VM v8 保存真实 owner 和列表，拒绝旧快照。

本批只实现加载，不包含 297 写回、自动保存、Windows 菜单或原生 slot body 恢复。合成容器回归覆盖字符串、BGM owner、私有快照恢复、缺文件及失败原子性；真实游戏重试另行记录，不能据此声明首 tick 或文字已经通过。


先前错误 CPZ 入口的有界 GPU 调试结果：12 个 CPZ 索引约 0.16 秒，DX12 discrete 设备约 0.87 秒，session open 约 0.89 秒；首次 advance 越过 296，停在 352 的窗口标题 action 未接入，约 0.97 秒关闭。实际读取原版 system.dat 的独立私有副本，容器 2504 字节、version 1，解码 flags 32512 字节、CG 2048/BGM 1024 words、64 个空标题槽、两组列表 count 均为零。原文件未改写，自有进程已退出。仍无完成首 tick、267 文字帧、连续输入、PCM 或声音结果。


352 现在在 CMVS 核心解析完整 tagged-string 域并立即拥有标题副本，保留 component+288 和空标题清空语义；Family 通过 v8 `SetTitle` 提交到真实 Host，遵守公共 UTF-8 4096 字节/控制字符边界。Host 窗口线程实际显示仍待连同后续 startup 动作验证。

143 的旧 caption 分类已更正为 `SetWindowIcon`：原版按游戏根拼接图标路径，`LoadImageA(IMAGE_ICON, LR_LOADFROMFILE)` 成功后设置 `WM_SETICON/ICON_SMALL`，失败不阻止脚本。实际目标图标已用 Windows 原生 loader 验证可加载，但本批 Family 图标呈现尚未接入，返回独立 `WINDOW_ICON_UNBOUND`，不伪称已显示。153 的 startup 条件是全局 word2190 大于零；当前真实 system.dat 对应值为零。自定义 cursor 的后续非零分支仍未接 Host。


143 的只读图标加载已接公共 v9 `SetIcon`：CMVS 仅启用现有 image 的 ICO feature，按照 [LoadImage 文档](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-loadimagea) 的零尺寸规则选择首图，以有界单 entry 视图交给 image 解码 BMP/PNG，再提交 straight-alpha RGBA8。目录、offset/length、图像尺寸和分配预算均受限，不使用 decoder 默认的最大图选择。缺文件或明确无效图标返回原生 best-effort 结果；越界路径、资源预算或不支持的 decoder 能力明确失败，有效图标不能当缺失忽略。窗口属性队列按类型保留最新值，标题和图标分别交给 Host，避免同一次 advance 相互覆盖。原版图标 256×256 的 Windows loader 只读检查成功；Family/Host 的实际窗口标题和图标显示仍待验证。


窗口命令批之后、仍使用错误 CPZ 入口的单次 GPU probe：索引约 0.25 秒、DX12 discrete 创建约 1.02 秒、session open 约 1.03 秒。352/143 已执行并将有效标题/图标排队；首次 advance 在 129 的原生脚本搜索返回 `RESOURCE_NOT_FOUND`，约 1.17 秒正常关闭。未完成首 tick，窗口请求尚未返回 Host，因此未验证 Manager 实际显示；267、帧、输入、PCM 和声音也未通过。私有副本此前缺少的原版有效图标已补齐，不能把该副本缺项解释为原生图标加载失败。


入口纠正：此前 Family 跳过原生 SCRIPT_INIT_PATH，误选已挂载 CPZ 中同名 start.ps3；游戏原版实际先加载配置目录内的 loose bootstrap，再由脚本设置后续搜索槽。原版与私有副本的 bootstrap 均存在且内容相同，未修改商业脚本。此前 GPU、原生 system.dat 与各命令的局部检查仍有效，但不能称为正确原生冷启动已经跨过这些动作。当前修复统一 config slot0→boot→call 的搜索路径，正确入口的单次有界 probe 已重新执行：12 个 CPZ 索引约 0.20 秒，DX12 discrete 设备约 0.97 秒，session open 约 0.98 秒；正确 bootstrap 已执行并加载后续脚本，首次 advance 在后续脚本 command 354 返回 ACTION_UNBOUND，约 1.14 秒正常关闭，自有进程已退出。未完成首 tick，未交付 Host 窗口请求，267 文字、图像帧、连续输入、PCM 与实际声音仍未验证。回归覆盖同名 loose/CPZ、后续槽覆盖、缺目录、坏选定资源以及失败不替换活动 VM。

354 现直接更新核心存档标题缓冲 10324，删除未被消费的 Host 动作。原生 CP932 字节长度小于 128 时复制到已有 interpreter_string_buffers；达到上限则保留旧标题，并对 10532 错误字段 OR 8，不清除其他位。原版保存函数从同一缓冲生成日期标题与 tag 256；当前仅完成缓冲和 VM 保存恢复语义，原生保存写回仍未实现。回归覆盖 127/128 字节、多字节字符、空标题、旧值和错误位保留及序列化恢复。

354 修复后，209 项 unit 通过，1 项 GPU fixture 忽略，Clippy all-target、fmt 与文档检查通过。正确入口再次执行单次有界 GPU probe：索引约 0.20 秒、DX12 discrete 约 1.03 秒、session open 约 1.05 秒；首次 advance 已跨过 354，随后在 353 返回 `ASTRA_EMU_CMVS_WINDOW_CAPTION`，原因是窗口标题前缀尚未绑定，约 1.21 秒正常关闭且无残留进程。仍无完成首 tick、Host 窗口请求交付、267 正文、图像帧、输入或音频结果；本批到此冻结。

353 现从352写入的288缓冲读取标题前缀，将实际参数复制到12816后缀缓冲，经现有 Family SetTitle 提交组合标题；354的存档标题缓冲保持独立。`initial.cfg` 的 `CMVS_INIT_CFG.FONT_SHIFT_JIS` 缺省为1：非零使用原生 CP932 全角空格，零使用两个 ASCII 空格，选择存入已有 process_global_words 并随 VM 恢复。原生拼接使用128字节栈数组且没有越界检查；本实现对组合 CP932 字节数达到128明确拒绝，并保留旧后缀，不模拟内存破坏。旧固定空格片段已删除，私有 VM snapshot 升至9并拒绝旧版本，Family API不变。213项 unit 通过、1项 GPU fixture忽略，覆盖分隔符、空后缀、多字节边界与恢复。

353批正确入口有界 GPU probe 仍在首 advance 返回 `VM_STRING_TAG`。追加 Command 诊断后的同配置复测：索引约0.21秒、DX12 discrete约1.51秒、open约1.53秒、close约1.74秒；错误未经过 Command 上下文，具体非 Command 指令、offset以及是否已经到达353仍未定位。现已统一为全部 VM 指令和 action 错误追加 kind/offset（Command另含id），补充五类指令诊断回归；该最终诊断未再进行 GPU 复测。本批冻结，仍无完成首 tick、Host标题交付、图像/文字帧、输入或音频通过，原存档未写入。

正确入口后续定位：统一诊断将 VM_STRING_TAG 定位到 value expression offset 200324。对照原版46F180，0x201表达式中的0x170赋值必须按源记录的tag/value填充字符串缓冲，再按目标记录tag/value写入；原实现错误地用目标tag解释源payload。修复仅调整执行解析，不改变私有snapshot或字符串域；回归以相同payload分别读取池、local data和global来源，并检查失败保留目标及保存恢复。实际首tick/文字仍待修复后的GPU流程确认。

本批 215 项 unit 通过，1 项 GPU fixture 忽略，all-target Clippy、fmt 与文档检查通过。正确入口实际 GPU 复测已跨过 offset 200324 的赋值：索引约 0.48 秒、DX12 discrete 约 2.40 秒、open 约 2.45 秒；首 advance 的下一阻断是 command 548 at offset 200462，返回 ACTION_UNBOUND，约 3.32 秒正常关闭且无残留进程。尚无完整首 tick、帧、267 正文、Host 请求交付、输入或音频结果。

最终合成 RGB 的 Family 帧声明 Ignored alpha，保留原字节/stride，发布时不重复逐像素 Opaque 认证；VM/Scene/媒体不变。Host 必要 GPU 归一化有成本，release 硬件对比和未验收范围见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md#10-整改进度2026-10-13)。这不补齐上述未接通的真实游戏能力。
