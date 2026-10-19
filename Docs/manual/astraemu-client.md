# AstraEMU 客户端

完整运行包包含唯一客户端可执行文件 `astra-emu-manager`（Windows 为 `.exe`）、`cores/`、许可说明和本手册。无参数启动 Manager GUI；同一程序提供扩展管理及 `patch list` / `patch apply` CLI，CLI 不启动 GUI、核心或游戏。不再构建或随包提供独立补丁可执行文件。解压后直接启动 Manager，不需要设置环境变量或手工复制核心到用户数据目录。只移动可执行文件会丢失随包核心。

资料库出现后，点击“刷新核心”手动加载核心，再点击“扫描游戏”刷新已有资料库，或点击“添加游戏”选择原生游戏目录；也可选择包含多个游戏的父目录。扫描完成后选择游戏，点击“启动”。多个模拟器能识别同一目录时，在“模拟器配置”中选择并保存。游戏不需要转换为 AstraVN package。

游戏输入、原生菜单和原生存读档由核心负责。关闭游戏回到资料库；重新启动创建新的会话。资料库、设置和运行日志保存在系统用户数据目录，不写入程序目录；游戏的既有存档仍按核心自身规则处理。联网元数据和翻译需要在设置中单独启用。

核心自行打开真实音频设备；设备不可用或格式不支持时显示原生错误，不自动切换 NullAudio。Manager 不再驱动核心 Tick 或接收核心 PCM。

FVP 当前原生 App 路径仍嵌入上游四个 MS 命名字体文件，分发授权尚未核验。完整说明见 [第三方声明](../../Emulator/THIRD_PARTY_NOTICES.md)，当前开发产物不能直接当作可分发 Release。


点击“刷新核心”后，客户端从程序相邻的 `cores/` 和用户数据目录的 `cores/` 同时发现核心。核心文件损坏、ABI 不符和重复 plugin ID 都会显示诊断；重复核心不会按目录顺序覆盖。本次运行只加载一次核心，替换核心或修复加载失败后需要重启客户端，不做热替换。

“核心”页可安装本地核心或移除用户安装的核心。安装先完成独立副本，再登记为待安装；已加载后更新须重启。复制或替换失败不会删除已安装核心，随包核心不能从此入口移除。

游戏视图的会话菜单按核心实际声明显示。Musica 提供快速存读档、存读档页、自动播放、快进、回看及设置入口，操作沿原生输入处理；不支持的操作不会伪装为成功。


新 Manager 只加载 C ABI v2 核心，旧 Family vtable 不兼容。核心自行运行 VM、原生菜单、渲染、音视频与保存；Manager 接收包含文字和游戏 UI 的完整帧。CPU lease 与 RFVP 的 D3D12 扩展共用呈现滤镜链。D3D12 使用独立 Device、共享图像及双向 fence，包含两次 GPU copy，不称零拷贝。

当前 RFVP 已验证原生剧情、选择、菜单/缩略图、存读档和关闭重开，以及 D3D12 导入、滤镜、resize、旧租约和设备丢失。Siglus 已通过新 ABI 的标题、影片、正文、存读档和同 DLL 重开代表流程。KrKr 已删除 child/IPC，公开 TJS/PNG/OpenAL 与重开验证通过。七核心可玩范围及已知阻塞统一见[实施计划](../status/implementation-plan.md)；本轮不要求每核心通关，旧 Hosted/Sandbox 结果不代替本次验收。

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

旧补丁的私有容器或 DLL 数据确实无法由核心直接消费时，才使用“应用文件补丁（仅必要时）”或 `astra-emu-manager patch`。GUI 直接调用共享 `astra-emu-patch` 库，与同一可执行文件内的 CLI 复用转换实现，不派生补丁子程序。转换输出为普通原生资源及 typed 配置，游戏启动不执行转换脚本，不加载旧补丁 DLL；也不为接入本地化新增运行时 Lua 格式或转换器依赖。Artemis 自身 Lua 与 Musica 文字 Hook 的 Luau 功能仍各用原有生命周期。没有每游戏一个 recipe 或 13 个脚本的要求。

Manager 操作顺序：

1. 结束运行中的游戏，点击“刷新核心”，选中来源游戏，等待加载/扫描结束。
2. 在详情的“应用文件补丁（仅必要时）”中选可信目录记录，或点击“本地 Luau…”选择已审核入口。填写“补丁输入目录”（留空使用来源游戏目录）与尚不存在的“新输出目录”，点击“开始”；需要中止时点击“取消”。
3. 成功后 Manager 导入新独立目录，校验收据、所选核心及 typed 字段，再以 SQLite 事务保存该输出游戏的核心绑定和配置。来源游戏绑定不改。若显示“文件转换完成，导入失败”，保留已完成输出并处理具体错误，再用“添加游戏”选择该输出目录；导入只读取收据，不重跑脚本。

统一入口迁移前的 Manager/CLI 逻辑及真实 SQLite 写入/重开已有检查证据；这些历史结果不替代 Beta2 的实际进程检查，Manager GUI 操作仍待实际观察。目录登记 eden、ef first、樱花萌放、夏空四个必要转换入口；前奏诗已定位回看页标题的原生查找链缺口，仍未登记，LLJ 分支及其他阻塞见实施计划。夏空随附字体只供 ASS，正文沿用 Musica 内置中文字体；不要因字符映射表完整就把字幕子集强制选为正文字体。复制、解析或构建成功均不代表完整本地化人工通过。

`astra-emu-manager patch list` 只读取程序相邻 `patches/catalog.json` 的目录记录，逐行输出 JSON（NDJSON），不执行或探测脚本。没有随包目录时，可用 `astra-emu-manager patch list --scripts <可信目录>` 显式选择。应用命令为：

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

历史 checkpoint 已构建七核心、Manager 和当时的独立转换 CLI，并组装本机验证目录 `AstraEMU-native-first-checkpoint-20261019`；其 PE 依赖/导出及包内 CLI 公开转换已检查。这些是统一入口迁移前的历史证据，不代表 Beta2 已构建或通过核验。该目录是原生优先的验证组合，尚有七个候选的静态消费者/素材阻塞，实际 Manager 界面和游戏仍待用户手动验收。已发布源码不等于可公开分发二进制；RFVP 字体、Ethornell 许可及下述静态 FriBidi LGPL 义务核验前，产物仅作本机私有使用。

下面以 FVP 为例。先初始化主仓固定的完整 submodule；不要换回旧 revision 绕过失败。参与核心保留不同原生 Lua/音频依赖，分别构建，不将 Artemis Lua 5.1 和 Manager Luau 合并进同一次 Cargo 调用。

```sh
# 替换为数据盘上的专用工作目录；构建、运行包和临时文件放在其中。
work_dir="<data-disk-work-directory>"
export CARGO_TARGET_DIR="$work_dir/target"
export TEMP="$work_dir/tmp" TMP="$work_dir/tmp"
mkdir -p "$TEMP"
git submodule update --init
cargo build --manifest-path Emulator/Cargo.toml --locked --release -j 1 \
  -p astra-emu-manager -p astra-emu-fvp \
  --features astra-emu-fvp/dynamic-plugin-export
python Tools/Effects/fetch_dxc.py --output "$work_dir/dxc"
python Tools/AstraEMU/package_client.py \
  --manager "$CARGO_TARGET_DIR/release/astra-emu-manager.exe" \
  --core "$CARGO_TARGET_DIR/release/astra_emu_fvp.dll" \
  --runtime-file "$work_dir/dxc/dxcompiler.dll" \
  --runtime-file "$work_dir/dxc/dxil.dll" \
  --runtime-file "$work_dir/dxc/LICENSE-MS.txt" \
  --runtime-file "$work_dir/dxc/LICENSE-LLVM.txt" \
  --output "$work_dir/client"
```

Linux/macOS 分别使用无 `.exe` 的可执行文件以及 `libastra_emu_fvp.so` / `libastra_emu_fvp.dylib`；DXC 示例仅适用于 Windows。`--core` 可以重复传入多个已构建的 Family。`--runtime-file` 指定随包的依赖库或许可文件；Windows 分发还需要加入构建工具链对应的 VC runtime DLL。输出目录必须是新目录，避免上一版核心混入新运行包。该工具组装现有产物，不编译、不启动游戏、不发布。

运行包只包含一个 `astra-emu-manager` 客户端可执行文件；补丁命令随 Manager 一起构建，无独立程序参数或兼容别名。打包器拒绝已移除的 `--patch-cli`，也拒绝用 `--runtime-file` 夹带旧 `astra-emu-patch` 可执行文件。打包器只复制文件，不运行二进制，也不替代最终 PE 架构、导入闭包、导出符号和 ZIP/hash 核验。

仅在已有必要且审核通过的转换入口时指定 `--patches Emulator/Patches`。打包器只复制 `catalog.json` 及其列出的顶层 `.luau` 入口；每个确实需要的共享模块分别通过 `--patch-common common/<模块>.luau` 指定。不递归复制脚本目录，不自动包含未列出的脚本、模块、夹具、测试目录或游戏资产。目录是显式可信输入，发布者负责核对每个选中脚本的来源与内容；路径检查不能判断脚本是否夹带商业数据。缺失文件、目录逃逸、链接、测试脚手架路径和不区分大小写的名称冲突均拒绝打包。没有可用 recipe 时省略 `--patches`，不要为填满目录登记原生游戏。

Manager 使用的 mlua MIT 许可从 `Emulator/ThirdParty/mlua/LICENSE` 复制。Luau 许可默认读取 `CARGO_HOME`（未设置时为 `~/.cargo`）下 `luau0-src-0.12.3+luau663/LICENSE`；可用 `--luau-license <该源码的LICENSE>` 显式指定。缺失则需先恢复锁定依赖源码，打包器不会下载或生成许可。

包含原生 ASS 字幕能力的构建还须传入 `--subtitle-share <独立vcpkg安装>/x64-windows-static-md/share`。工具逐字复制 libass、FreeType、FriBidi、HarfBuzz、Brotli、bzip2、libpng、zlib 的实际 `copyright`，任一缺失即失败。静态 FriBidi 的 LGPL 对应源码、重链接等义务须另行核验；当前只组装本机私有包，不据此宣称可以公开分发。FVP 字体与 Ethornell 的既有许可限制仍然保留。

Python API 为 `package(manager, cores, output, runtime_files, patches=None, *, patch_common=None, luau_license=None, subtitle_share=None)`；原有四参数调用继续有效，`patches` 可作第五个位置参数，其余可选项为关键字参数，与上述选项对应。不再接受 `patch_cli` 参数。输出使用同父目录 staging，复制失败清理自身临时目录，发布不覆盖打包期间新出现的目标。

公开行为测试：`python -m unittest Tools.tests.test_package_client`。测试使用临时合成输入验证实际输出与失败保护，不运行游戏、原生核心或补丁脚本。

构建 Musica 前还需恢复其编译期嵌入的字体，避免把 LFS 指针编入核心。运行包会带上字体的 OFL 许可。

```sh
git lfs pull --include="Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf,Examples/NativeVN/Assets/Fonts/NotoSansSC-Variable.ttf" --exclude=""
```

Musica 动态核心须同时启用 `dynamic-plugin-export,ffmpeg-vcpkg`，并配置 FFmpeg vcpkg 环境。构建完成后立即把该 DLL 及匹配的 FFmpeg 依赖复制到独立运行目录，再启动 Manager；之后对同一 target 的 CLI/静态依赖构建可能覆盖未启用导出的同名 DLL。`astra_emu_query` 必须能协商 ABI v2，不能把普通 rlib 依赖构建产生的 DLL 当作产品核心。

KrKr 原生运行目录用 `Tools/AstraEMU/package_krkr_runtime.py --native-runtime <目录> --output <新目录>` 组装，包含实际 DLL、Resources 和运行依赖；不再接收旧 Rust child 产物。该工具不代替商业流程和许可证核验。
