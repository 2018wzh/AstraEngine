# AstraEMU 客户端

完整运行包包含唯一客户端可执行文件 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。无参数启动 Manager GUI；同一程序提供扩展管理及 `patch list` / `patch apply` CLI，CLI 不启动 GUI、核心或游戏。不再构建或随包提供独立补丁可执行文件。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

资料库出现后，点击“刷新核心”手动加载核心，再点击“扫描游戏”刷新已有资料库，或点击“添加游戏”选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个模拟器能识别同一目录时，在“模拟器配置”中选择并保存。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

核心自行打开真实音频设备；设备不可用或格式不支持时显示原生错误，不自动切换 NullAudio。Manager 不再驱动核心 Tick 或接收核心 PCM。

FVP 当前原生 App 路径嵌入上游四个 MS 命名字体文件，按上游原样随核心发布。来源与许可见 [第三方声明](../../Emulator/THIRD_PARTY_NOTICES.md)。


点击“刷新核心”后，客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。每个核心是一个子目录 `cores/<stem>/`，内含恰好一个 `astra_emu_*` 核心库、依赖、`Resources/`、`licenses/` 与 `core.json`；同名子目录以用户数据目录为准，子目录内核心库数量不对时显示 `ASTRA_EMU_CORE_LAYOUT`。旧版放在 `cores/` 顶层的核心库会在下次加载前自动迁入各自子目录。核心损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。本次运行只加载一次核心，安装、更新或修复加载失败后需要重启客户端，不做热替换。

“核心”页合并显示已加载、加载失败、待处理和可在线安装的核心。“从本地安装”接受 `.zip` 核心包或单个核心库；允许联网后可按元数据核心目录在线安装或更新（显示“可更新”）。核心包先校验路径、`core.json` 与每个文件的 sha256，再放入 `cores/.pending/<stem>/`，卸载写入 `cores/.pending/<stem>.remove`；下次加载核心前应用，替换失败恢复原目录。随包核心不能从此入口移除。

游戏视图的会话菜单按核心实际声明显示。Musica 提供快速存读档、存读档页、自动播放、快进、回看及设置入口，操作沿原生输入处理；不支持的操作不会伪装为成功。


新 Manager 只加载 C ABI v3 核心，旧 Family vtable 与 ABI v2 核心不兼容。核心自行运行 VM、原生菜单、渲染、音视频与保存；Manager 接收包含文字和游戏 UI 的完整帧，帧只经 CPU lease 交付并进入呈现滤镜链。声明 `CAP_RENDER_EXTENT` 的核心（RFVP、Musica、Sena）按 `render_extent` 请求的光栅尺寸渲染。

当前 RFVP 已验证原生剧情、选择、菜单/缩略图、存读档和关闭重开；此前的 D3D12 交付已在 C ABI v3 中删除。Siglus 已通过新 ABI 的标题、影片、正文、存读档和同 DLL 重开代表流程。KrKr 已删除 child/IPC，公开 TJS/PNG/OpenAL 与重开验证通过。七核心可玩范围及已知阻塞统一见[实施计划](../status/implementation-plan.md)；本轮不要求每核心通关，旧 Hosted/Sandbox 结果不代替本次验收。

Musica 提供同步文字 Hook；共享串行 Luau VM 执行已启用扩展。原生取消不排在 Luau 队列后。可安全重试的错误显示重试和结束，其他失败只允许结束。只安装可信本地扩展；CLI 为 `extension-install <id> <source.luau>`、`extension-enable <id> <true|false>`、`extension-configure <id> <configuration.json>`、`extension-list`。本批不提供在线扩展市场。

验收使用 Release Manager 和 Release 核心，由用户在实际窗口中手动操作。Manager 不再提供脚本式 headless、自动输入、截图或关闭重开入口；扩展管理 CLI 保留。BGI 已由用户确认人工通过，其他核心按用户实际结果记录，不以构建成功代替人工验收。

历史开发证据保留在 Git 和 ignored 输出中。已知问题见[实施计划](../status/implementation-plan.md)；早期 Debug 或锁屏消息级结果不外推为本次 Release 性能、前台手感或听音结论。

## 启动与后台加载

Manager 先显示本地资料库、已缓存的标题和游玩信息，封面暂用占位图。启动、首帧和窗口聚焦都不会自动加载核心或刷新游戏库。“刷新核心”只加载核心并显示就绪或失败状态，不识别游戏，也不改动既有游戏绑定；等待手动操作时不显示为忙碌。

核心就绪后，“扫描游戏”在后台完整扫描已配置目录和已登记游戏，“添加游戏”扫描所选目录；界面不阻塞。识别提示保存在用户数据目录的 `probe-cache.json`，根据程序、核心清单、descriptor 和游戏文件元数据决定是否复用；手动完整扫描只复用已登记目录的有效提示，新目录、变化项和复杂或无法可靠校验的目录重新 probe。每次实际启动仍只对所选模拟器执行一次新的原生校验；不匹配时报错，不自动换用其他模拟器。

封面只为可见范围和选中作品异步解码，缩略图缓存有上限；滚动、筛选或重排后，旧任务不能覆盖新卡片。标题、元数据关联和累计游玩信息通过一次批量快照读取，不逐游戏重复查询。

扩展线程和共享 Luau VM 在首次实际需要文字 Hook 的游戏启动时创建，保留到 Manager 退出。游戏纹理与滤镜编译推迟到首个实际游戏帧；资料库阶段只校验效果描述。首次编译失败显示原始错误，不降级；运行中修改滤镜失败时保留旧有效效果。

现有日志新增 `astra.emu.startup.phase`，记录阶段名和距入口的 `elapsed_ms`，包括首帧及用户手动触发后的核心就绪；手动扫描日志记录游戏数及缓存命中数。用户等待手动操作的时间不能算作自动加载耗时。不记录私有配置或脚本文本；首窗、封面和滤镜表现仍由用户在 Release 中手动观察，不运行自动采样或窗口驱动。

## 游戏引擎与模拟器

详情分别显示引擎与模拟器及版本。例如引擎为 SoftPAL，模拟器为 Sena；同一引擎可以安装多个实现，并为不同游戏分别选择。更换模拟器后先保存，再填写该实现的参数。选择随资料库保存，重新扫描与重启不会清除；所选实现缺失时不会自动换用其他模拟器。参数仍按模拟器与游戏/默认范围独立保存。

“模拟器配置”分为“引擎公共设置”和“模拟器专属设置”。公共分辨率是核心送入滤镜之前的原生渲染尺寸：RFVP 提供等比例倍率，Musica 与 Sena 提供实际渲染宽高；它不修改逻辑舞台或窗口大小。未接入该能力的模拟器明确显示不可调整，仍可使用显示滤镜，不以 Host 重采样冒充原生渲染。两组参数共用 `game`（当前游戏）或 `core`（该模拟器默认）配置范围，以及同一组保存/重置按钮；下次游戏启动应用。

外观设置提供主题、密度、封面大小、列表/网格、减少动画和封面标记。升级时保留旧版已有的主题、密度和列表/网格选择，新选项采用默认值。详情页的 Bangumi 编辑草稿按选中游戏刷新，不将上一游戏未保存的备注带到另一游戏。


## Musica 私有配置

Musica 运行时只读取 `key.toml`（`astra.emu.musica.keys.v2`），包含已确认的 PAZ 参数、八个归档 role 的密钥及可选纹理替换映射。默认从游戏目录读取；也可在该游戏的“模拟器配置”中点击“选择 key.toml 并应用到当前游戏”，校验后保存绝对路径。Manager 不转换格式、不生成密钥，也不修改所选文件。

JSON profile、GARbro `Formats.dat` 和旧 `keys.v1` 只在[开发工具](../../Emulator/Source/Programs/astra-emu-musica-cli/README.md)中转换为新 `key.toml`，不作为运行时输入。旧 key 缺少的算法参数须明确提供。配置、密钥和既有存档不自动改写；不要公开提交或分享私有文件。

Musica 默认 `launch_mode=title`，启动先显示标题菜单，选择开始后再执行 `entry_script`。`direct` 是显式直入脚本模式，原版 eden 存档导入仍须选择它。已有保存的 `direct` 不自动覆盖；需要改为菜单启动时，在游戏的模拟器专属设置中选择 `title` 并保存。更新核心后须手动重启 Manager，已运行的会话不会自动切换。

## 本地化与原生选项

日常使用先在该游戏的“模拟器配置”中选择核心已提供的原生选项，保存到 `game` 范围后重新启动游戏。语言、字体、原生归档和脚本能直接读取时无需转换。按样本的当前选择如下；完整 18 样本证据及阻塞见[实施计划](../status/implementation-plan.md#18-样本矩阵)。

| 核心 / 样本 | 操作 |
| --- | --- |
| Artemis 终之空 2025 | `archive` 选择原基包；`extra_archive` 填已核实的独立 PFS 补丁相对路径（相对基包父目录，留空禁用）。多个 PFS 时明确选择基包，不重命名原文件，也不把备份自动当补丁 |
| Sena Koikake | `text_language` 选 `cn`；`jp`/`tc` 分别为原生日文/繁体列。单字段记录沿原生规则处理，不凭编码生成译文 |
| Siglus | `resource_language` 选 `chs`，原版选 `original`；`save_root` 可显式选择独立存档目录。使用配套 Gameexe/Scene 与原生 G01/影片选择；Rewrite 的未知机器码效果及缺失字体仍使完整迁移阻塞 |
| Musica | `script_encoding` 选实际来源的 `shift_jis` 或 `gbk`；PAZ 索引与密钥不随此改编码。继续使用已确认的 `key.toml`；必要转换输出的字体、纹理、字幕及翻译表配置由收据导入，不靠运行时补丁 DLL |
| Ethornell/BGI | 需要 GBK 时显式选择显示编码，并使用该 runtime 的实际字体配置；这不改变内部资源名/操作数。HD 的归档归属、`fcxzzd.arc`/字体接线与樱之诗的私有消费者仍未闭合 |
| KrKr | 直接使用既有原生 XP3/TJS/PSB 等资源路径；LLJ 的运行分支/字体与千恋万花的 key、完整中文列及插件接口仍待解明，不能靠旁置 Win32 DLL 或 Steam 语言设置判为完成 |

已安装的 Artemis 10th、BGI 春之雪、BGI 15th、ef latter、Anemoi 尚缺已确认译文。先确认与该版本匹配的译文来源；修改编码或复制别的版本补丁不能补出译文。既有 BGI 用户人工通过记录保留，但不充当新汉化迁移证明。

### 高级：手动离线文件转换

旧补丁的私有容器或 DLL 数据确实无法由核心直接消费时，才使用 Manager 的“汉化补丁”页或 `astra-emu-manager patch`。GUI 直接调用共享 `astra-emu-patch` 库，与同一可执行文件内的 CLI 复用转换实现，不派生补丁子程序。转换输出为普通原生资源及 typed 配置，游戏启动不执行转换脚本，不加载旧补丁 DLL；也不为接入本地化新增运行时 Lua 格式或转换器依赖。Artemis 自身 Lua 与 Musica 文字 Hook 的 Luau 功能仍各用原有生命周期。没有每游戏一个 recipe 或 13 个脚本的要求。

“汉化补丁”是导航栏中游戏库与设置之间的独立页面（窄窗口在游戏库标题栏右侧）；作品详情的“更多操作”和游戏右键菜单中的“汉化补丁”会直接带着这部作品打开第 2 步。操作顺序：

1. 结束运行中的游戏，点击“刷新核心”并等待加载/扫描结束。在“选择游戏”中搜索并点选来源游戏，列表显示封面、引擎 · 模拟器和 VNDB 关联状态。
2. “选择补丁”只列出补丁目录中适用于这部作品的条目：作品关联的 VNDB vID 在条目的 `vn_ids` 中，且作品选定的模拟器（尚未选定时为识别出的候选模拟器）在条目的 `emulators` 中；推荐配置指定的补丁标为“推荐”。作品没有关联 VNDB 时不列出目录补丁，先在 游戏设置→作品资料 关联 VNDB 作品。任何作品都可以点击“选择本地脚本…”使用已审核的 Luau 入口。
3. “目录”中的补丁输入目录默认是游戏目录，补丁文件在别处时改选；输出目录必须是尚不存在或为空的新目录。
4. 在“执行”中核对后点击“开始转换”，需要中止时点击“取消”。转换期间前面的步骤锁定，不能启动游戏或扫描。
5. 成功后 Manager 导入新独立目录，校验收据、所选核心及 typed 字段，再以 SQLite 事务保存该输出游戏的核心绑定和配置；“完成”页的“打开新作品”在游戏库中选中它。来源游戏绑定不改。若显示“文件转换完成，导入失败”，保留已完成输出并处理具体错误，再用“添加游戏”选择该输出目录；导入只读取收据，不重跑脚本。

统一入口迁移前的 Manager/CLI 逻辑及真实 SQLite 写入/重开已有检查证据；这些历史结果不替代 Beta2 的实际进程检查，Manager GUI 操作仍待实际观察。目录登记 eden、ef first、樱花萌放、夏空四个必要转换入口；前奏诗已定位回看页标题的原生查找链缺口，仍未登记，LLJ 分支及其他阻塞见实施计划。夏空随附字体只供 ASS，正文沿用 Musica 内置中文字体；不要因字符映射表完整就把字幕子集强制选为正文字体。复制、解析或构建成功均不代表完整本地化人工通过。

`astra-emu-manager patch list` 只读取程序相邻 `metadata/patches/catalog.json`（随包元数据快照）的目录记录，逐行输出 JSON（NDJSON：`id`、`title`、`script`、`vn_ids`、`emulators`），不执行或探测脚本。没有随包快照时，可用 `astra-emu-manager patch list --scripts <可信目录>` 显式选择。Manager 中的补丁目录来自当前使用的元数据（同步缓存或随包快照）的 `patches/`。应用命令为：

```sh
astra-emu-manager patch apply --script <可信入口.luau> --source <原游戏目录> \
  --patch <补丁目录> --output <全新独立目录>
```

省略 `--patch` 时使用原游戏目录。CLI 可用 Ctrl+C 取消，原生取消通道与 VM 中断使用同一取消标志；取消返回码为 130，等待工作线程结束后退出，其他失败返回 1。CLI 不自动启动 GUI、核心或游戏，也不自动导入或启动转换产物。只使用可信本地脚本，转换由用户显式发起。转换器只读打开输入/密钥/旧存档，不复制旧存档；输出为不依赖原安装回退读取的完整独立副本，目标存在即失败。成功时显示输出与 `astra-patch-result.json` 收据位置；用 Manager“添加游戏”选该输出目录即可校验并导入 typed profile。不要手改收据去绕过核心/字段错误。

Windows 使用同一个 GUI 子系统程序 `astra-emu-manager.exe`。CLI 连接现有父控制台并保留重定向的标准输入输出，不另开控制台窗口；交互式 `cmd.exe` 若立即返回提示符，可用 `start /wait "" astra-emu-manager.exe patch list` 等待命令结束，`patch apply` 同理。

转换成功后仍须核对实际字体、脚本分支、资源优先级和媒体效果，再由用户手动验收游戏。私有输出、商业文本/图片、字体、密钥与旧存档不进入公开源码或运行包。本轮 90 个已知原文件哈希的匹配范围、缺失的任务前存档基线及既有隐私事故统一记录在[完整性、隐私与发布](../status/implementation-plan.md#完整性隐私与发布)，不据此宣称全部原安装/存档未变。

## 0.1.0-beta.2 私测

Beta2 将 GUI 与补丁 CLI 统一到 `astra-emu-manager`。Manager 已使用 Release 优化重新构建，`--version` 为 `0.1.0-beta.2`；统一命令的真实进程、公开转换和 Ctrl-C 检查分开记录，整包证据见同目录 `release-manifest.json` 与 `SHA256SUMS`，不沿用 Beta1 的通过数。七核心复用冻结 Beta1 中已核验的相同哈希 Release DLL；版本号不代表七个阻塞样本已经完成。

解压到独立新目录，保留整个 `cores/`、Resources、依赖与许可文件。使用隔离游戏副本和新存档槽，不覆盖原版存档；必要转换仍输出全新独立目录。人工检查字体/双语切换、标题与剧情、影片字幕、暂停恢复、基本菜单与存读档、退出重开，并记录程序版本、核心 revision、操作步骤和错误码。不要提交剧情正文、密钥、商业资源或私人路径。

本批只作本机私测，许可限制及逐样本状态见[实施计划](../status/implementation-plan.md#本地化原生优先的当前状态)。不自动启动游戏、输入、截图或关闭窗口，BGI 既有人工验收不重复。

### 0.1.0-beta.1 私测（历史冻结包）

Beta1 使用 Release 优化构建；Manager“关于”及当时 Manager、独立转换 CLI 两个程序的 `--version` 实际显示 `0.1.0-beta.1`。该包固定在提交 `455c94284519ce4121deedc8767cc66a9badfe4f`，不原地替换。两个可执行文件是冻结历史产物，不是当前构建或打包方式；新包使用上述 Beta2 单一入口。

## 构建运行包

历史 checkpoint 已构建七核心、Manager 和当时的独立转换 CLI，并组装本机验证目录 `AstraEMU-native-first-checkpoint-20261019`；其 PE 依赖/导出及包内 CLI 公开转换已检查。这些是统一入口迁移前的历史证据，不代表 Beta2 已构建或通过核验。该目录是原生优先的验证组合，尚有七个候选的静态消费者/素材阻塞，实际 Manager 界面和游戏仍待用户手动验收。公开 Release 由下文的发布工作流构建：FriBidi 与两组 FFmpeg 以 DLL 随各自核心目录分发，并附 LGPL-2.1 文本与对应源码包。

下面以 rfvp 为例。先初始化主仓固定的完整 submodule；不要换回旧 revision 绕过失败。外部核心的适配代码在各 fork 的 `astra` 分支，各自在 fork manifest 上独立构建，不进入 Manager 的 Cargo workspace；参与核心保留不同原生 Lua/音频依赖，不合并进同一次 Cargo 调用。公开 Release 由 `.github/workflows/emu-release.yml` 按同样步骤构建全部七个核心，见下文“发布工作流”。

```sh
# 替换为数据盘上的专用工作目录；构建、运行包和临时文件放在其中。
work_dir="<data-disk-work-directory>"
export CARGO_TARGET_DIR="$work_dir/target"
export TEMP="$work_dir/tmp" TMP="$work_dir/tmp"
mkdir -p "$TEMP"
git submodule update --init
cargo build --manifest-path Emulator/Cargo.toml --locked --release -j 1 -p astra-emu-manager
cargo build --manifest-path Emulator/ThirdParty/rfvp/Cargo.toml --locked --release -j 1 \
  -p astra-emu-rfvp --features dynamic-plugin-export --target-dir "$work_dir/t-rfvp"
python Tools/Effects/fetch_dxc.py --output "$work_dir/dxc"
python Tools/AstraEMU/stage_core.py \
  --library "$work_dir/t-rfvp/release/astra_emu_rfvp.dll" \
  --plugin-id rfvp --version 0.6.0 \
  --dependency "$ASTRA_SUBTITLE_VCPKG_ROOT/installed/x64-windows-static-md-dynbidi/bin/fribidi-0.dll" \
  --license rfvp/LICENSE=Emulator/ThirdParty/rfvp/LICENSE \
  --license rfvp/THIRD_PARTY_NOTICES.md=Emulator/ThirdParty/rfvp/crates/astra-emu-rfvp/THIRD_PARTY_NOTICES.md \
  --license rfvp/MODIFICATIONS.md=Emulator/ThirdParty/rfvp/crates/astra-emu-rfvp/MODIFICATIONS.md \
  --output "$work_dir/stage-rfvp"
python Tools/AstraEMU/package_client.py \
  --manager "$CARGO_TARGET_DIR/release/astra-emu-manager.exe" \
  --core-dir "$work_dir/stage-rfvp/astra_emu_rfvp" \
  --runtime-file "$work_dir/dxc/dxcompiler.dll" \
  --runtime-file "$work_dir/dxc/dxil.dll" \
  --runtime-file "$work_dir/dxc/LICENSE-MS.txt" \
  --runtime-file "$work_dir/dxc/LICENSE-LLVM.txt" \
  --output "$work_dir/client"
```

`fetch_dxc.py` 校验下载的 zip 与 `Emulator/Assets/Effects/dxc.lock.json` 的 sha256 一致，不符时以 `ASTRA_EMU_DXC_LOCK_HASH` 失败。

`stage_core.py` 把一个已构建的核心整理成核心目录 `<output>/<stem>/`（`<stem>` 为库文件名去掉扩展名）：库文件、`--dependency` 指定的依赖 DLL、可选 `--resources` 目录（复制为 `Resources/`）、`--license` 指定的许可（`<文件>` 或 `<包内相对路径>=<文件>`，放入 `licenses/`），以及 `core.json`（`schema` 为 `astra.emu.core_package.v1`，含 `plugin_id`、`library`、`version`、`abi = astra.emu.native.c.v3` 和每个文件的 sha256）。`--plugin-id` 与 `--version` 必须与核心描述符一致。一个核心目录只能有一个 `astra_emu_*` 库；输出必须是新目录。依赖 DLL 的闭包可用 `python Tools/AstraEMU/check_core_deps.py closure --library <dll> --search-dir <目录>` 列出，整理后用 `check_core_deps.py check --core-dir <目录>` 核对：每个 `dumpbin /dependents` 依赖须在同一核心目录、是包根的 `msvcp140.dll`/`vcruntime140.dll`/`vcruntime140_1.dll`、以 `api-ms-win-` 开头，或是 System32 中非 MSVC 可再发行运行时的系统 DLL。两者都需在 MSVC 开发者环境中运行。

`package_client.py` 的 `--core-dir` 可以重复传入，每个都是 `stage_core.py` 的产物，原样复制到 `cores/<stem>/`；打包器核对 `core.json` 中每个文件的 sha256，目录内有未列出的文件、目录名与库名不符或含多个家族库时拒绝打包。`--runtime-file` 指定随包根目录的依赖库或许可文件；Windows 分发需要加入构建工具链对应的 VC runtime DLL（`msvcp140.dll`、`vcruntime140.dll`、`vcruntime140_1.dll`）。输出目录必须是新目录，避免上一版核心混入新运行包。该工具组装现有产物，不编译、不启动游戏、不发布。

运行包只包含一个 `astra-emu-manager` 客户端可执行文件；补丁命令随 Manager 一起构建，无独立程序参数或兼容别名。打包器拒绝已移除的 `--core`、`--patches`、`--patch-common` 与 `--patch-cli`，也拒绝用 `--runtime-file` 夹带旧 `astra-emu-patch` 可执行文件。打包器只复制文件，不运行二进制。

兼容性、推荐配置、核心目录与补丁目录随包快照通过 `--metadata-dist <目录>` 指定，该目录是 AstraEMU-metadata 仓库 `python Emulator/Metadata/tools/build_index.py --output <dist>` 产出的 `<dist>/v1/`，必须含 `index.json`。打包器只复制 `index.json` 及其列出的文件到包内 `metadata/`，逐个核对 size 与 sha256；路径只允许 `[a-z0-9._/-]`，单文件不超过 8 MiB，总计不超过 64 MiB。省略该选项时运行包不含元数据快照，Manager 显示“未同步”。

Manager 使用的 mlua MIT 许可从 `Emulator/ThirdParty/mlua/LICENSE` 复制。Luau 许可默认读取 `CARGO_HOME`（未设置时为 `~/.cargo`）下 `luau0-src-0.12.3+luau663/LICENSE`；可用 `--luau-license <该源码的LICENSE>` 显式指定。Material Components for Slint 的许可默认取 `Emulator/Source/Manager/astra-emu-manager-ui-slint/material-1.1.0/LICENSE.md`，复制为 `licenses/slint-material/LICENSE.md`；Material Symbols 的许可默认取同一 crate 的 `ui/icons/LICENSE`，复制为 `licenses/material-symbols/LICENSE`；可分别用 `--material-license`、`--symbols-license` 指定。任一许可缺失则打包失败，打包器不会下载或生成许可。

包含原生 ASS 字幕能力的构建还须传入 `--subtitle-share <独立vcpkg安装>/installed/x64-windows-static-md-dynbidi/share`。工具逐字复制 libass、FreeType、FriBidi、HarfBuzz、Brotli、bzip2、libpng、zlib 的实际 `copyright`，任一缺失即失败。FriBidi 以 DLL 形式随使用字幕的核心目录分发（`fribidi-0.dll`），其余字幕依赖静态链接。

Python API 为 `package(manager, core_dirs, output, runtime_files, *, metadata_dist=None, luau_license=None, subtitle_share=None, material_license=..., symbols_license=...)`，可选项均为关键字参数，与上述选项对应。输出使用同父目录 staging，复制失败清理自身临时目录，发布不覆盖打包期间新出现的目标。

公开行为测试：`python -m unittest Tools.tests.test_package_client Tools.tests.test_stage_core Tools.tests.test_check_core_deps Tools.tests.test_release_catalog Tools.tests.test_bundle_sources`。测试使用临时合成输入和模拟的 `dumpbin` 输出验证实际输出与失败保护，不运行游戏、原生核心或补丁脚本。

### 发布工作流

`.github/workflows/emu-release.yml` 在推送 `emu-v*` tag 时构建并发布 GitHub Release；手动 `workflow_dispatch` 只产出 artifact，不建 Release。工作流在 `windows-latest` 上：

- 完整克隆 `microsoft/vcpkg` 并检出固定提交 `664933b8d385dfc01f4e0cdff4c7c74e71f6a740`（即 `astra-subtitle` 的 builtin-baseline，其 FFmpeg 最新版本为 9.0.2#1）；用它安装字幕树（`x64-windows-static-md-dynbidi`）和 `Tools/AstraEMU/vcpkg/ffmpeg/vcpkg.json` 固定的 FFmpeg 9.0.2#1（`x64-windows`，供 Musica 与 Artemis），KrKr 的 CMake 也使用这份检出。
- Manager 与 Musica 在 `Emulator/Cargo.toml` 工作区构建；六个外部核心在各自 fork manifest 上构建（`--locked --release`），每个使用系统盘根下独立的短 `--target-dir`（`t\<name>`），避免 shaderc 等嵌套原生构建超出 Windows MAX_PATH。kirikiroid2-web 按 fork `astra/README.md` 的 CMake 流程以 `-DKRKR2_ASTRA_EMU=ON` 构建目标 `astra_emu_kirikiroid2_web`，产物位于 `.work/krkr-msvc/astra/`。
- 外部核心的许可文件全部取自 fork：rfvp 为 `LICENSE` 与 `crates/astra-emu-rfvp/` 的 `THIRD_PARTY_NOTICES.md`、`MODIFICATIONS.md`；siglus_rs 为 `LICENSE-MPL-2.0`、`astra/` 下两份说明及嵌套 eluna_rs、Kira 许可；art3m1s-core 为 `LICENSE`、`crates/pf8/LICENSE` 与 `crates/astra-emu-art3m1s-core/` 下两份说明；sena-rs 与 ethornell-rs 为各自 `LICENSE-MPL-2.0`，ethornell-rs 另附 `MODIFICATIONS.md`；kirikiroid2-web 为 `LICENSE`、`astra/README.md`、layerExImage 与 DroidSansFallback 许可。依赖的 vcpkg `copyright` 照旧附上。
- 用 `check_core_deps.py closure` 收集依赖，`stage_core.py` 整理七个核心目录，`check_core_deps.py check` 核对依赖闭包；核心版本取自核心 crate 的 Cargo 版本（`release_catalog.py crate-version`）。
- 用 `Emulator/Metadata/tools/build_index.py` 生成元数据快照，`package_client.py` 组装 `AstraEMU-<version>-windows-x86_64.zip`（包根附 VC runtime 与 DXC），`release_catalog.py archive` 为每个核心目录生成 `astra-emu-core-<stem>-<version>-windows-x86_64.zip`（zip 根即核心目录内容），`bundle_sources.py` 生成 FriBidi、FFmpeg 9.0.2、KrKr FFmpeg 3.3.9 的对应源码包（vcpkg 下载的原始归档加 port 目录），并附 `LGPL-2.1.txt` 与 `SHA256SUMS.txt`。
- tag 触发时 `gh release create` 上传全部资产，发布说明为自动生成的核心版本列表；随后用 secret `ASTRA_METADATA_TOKEN` 向 `2018wzh/AstraEMU-metadata` 提交 `engines/cores/<plugin_id>.json` 并开 PR `cores: <tag>`。secret 缺失时该步失败并提示“需要配置 ASTRA_METADATA_TOKEN”，已上传的 Release 资产保留。核心显示名、引擎名与许可登记在 `Tools/AstraEMU/release-cores.json`。

构建 Musica 前还需恢复其编译期嵌入的字体，避免把 LFS 指针编入核心。运行包会带上字体的 OFL 许可。

```sh
git lfs pull --include="Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf,Examples/NativeVN/Assets/Fonts/NotoSansSC-Variable.ttf" --exclude=""
```

Musica 动态核心须同时启用 `dynamic-plugin-export,ffmpeg-vcpkg`，并配置 FFmpeg vcpkg 环境。构建完成后立即用 `stage_core.py` 把该 DLL 及匹配的 FFmpeg 与 FriBidi DLL 整理到新的核心目录，再交给打包器；之后对同一 target 的 CLI/静态依赖构建可能覆盖未启用导出的同名 DLL。`astra_emu_query` 必须能协商 C ABI v3，不能把普通 rlib 依赖构建产生的 DLL 当作产品核心。

KrKr 核心目录同样用 `stage_core.py` 整理：`--library` 为 CMake 输出的 `astra_emu_kirikiroid2_web.dll`，`--resources` 为同目录的 `Resources/`，`--dependency` 为 `check_core_deps.py closure` 在该输出目录、KrKr vcpkg `installed/x64-windows/bin`（FFmpeg 3.3.9 DLL）和 MSVC OpenMP redist 目录（`vcomp140.dll`）中找到的全部 DLL。不再接收旧 Rust child 产物，该工具不代替商业流程和许可证核验。
