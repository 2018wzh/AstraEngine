# Musica core

Musica 提供独立 `FamilyProvider`、`FamilySession` 和可选 `abi_stable` 动态模块。核心自己打开 PAZ、执行 SC VM、混音并维护存档。[astra-emu-sdk](../../SDK/astra-emu-sdk/README.md) 复用 `astra-text` 提供真实字体 shaping/排版/字形资源管理，并管理有界纹理缓存。场景和文字直接由 AstraEngine 的 `WgpuOffscreenRenderer` 在硬件 GPU 上合成，再回读最终帧交给 Family API；不创建 Engine World、package、registry、PlatformHost 或 Headless server。GPU 初始化失败明确报错，不退回 CPU 渲染。

动态插件通过共享诊断桥将核心 tracing/log 接入 Manager；静态 provider 使用宿主订阅器。日志不另建文件 sink，正文和未审查 Debug 内容不会跨 ABI 透传。

## 启动和输入

原生 C ABI v3 descriptor 提供 `profile_file`（界面显示 key.toml，默认 `key.toml`）、`entry_script`（默认 `test.sc`）等 typed 启动配置，完整字段以 Rust descriptor/schema 为准。正常启动默认 `launch_mode=title`，先显示原生标题菜单，点击开始后才执行入口脚本；`direct` 保留为显式直入与原版 eden 存档导入用途。已保存的显式选择不自动覆盖。key.toml 可选显式绝对路径，或游戏目录内安全相对路径；入口是 scr archive 内单个 `.sc` 文件名。不会在入口失败后猜测另一个脚本。

已接入的渲染路径保留既有 1280×720 坐标与 Noto Sans JP 字体绑定。Host resize 只改变窗口，不改游戏内部布局。Session 按 60 Hz 逻辑处理有界 elapsed interval，显示纹理、消息、面板和已验证的 CrossFade2；字形结果与纹理有界缓存。Enter、Space 和鼠标主键推进消息；F5 保存，F9 读取。关闭和销毁都会取消文字请求、取消 PCM 写入并等待 audio worker 结束；同进程不能同时打开两个 Musica session。

正文替换是可选服务；禁用时直接显示原文。启用时只暂停当前消息推进，效果和音频继续。请求具有 session/generation identity，加载和关闭取消旧请求。失败、15 秒 timeout、错误 response 或无法排版的替换保留原文，不自动重试。正文和错误 payload 不写日志。

原生本地化通过 `translation_table`（游戏目录内 v2 JSON）和 `translation_mode=replace|overlay` 配置，默认无表、替换模式。表必须包含 `version:2`、`source_encoding:"gbk"|"cp932"`、`hash_collision_policy:"first_record"`，以及保留原顺序的 `names`、`dialogue` 数组；每条为 `{source_hash,source_sha256,text}`，目标为 UTF-8。名字查找先于正文，同一 31-bit hash 只使用第一条；SHA-256 只作审计，不消歧或丢弃重复记录。hash 来自解码前的实际 authored display 字节（含尚未处理的消息控制），采用 131 rolling、记录侧跳过 NUL、运行时 C 字符串在首个 NUL 结束；不会重哈希 UTF-8，也不改变 VM 变量、选择目标或文件名。原生结果先交给既有 text hook，重试保留同一请求 ID。

`overlay` 保留原正文，名字仍按名字表替换；正文命中更新独立文字层，未命中保留已缓存字幕。Tab 松开切换该层可见性，按下和重复按键不切换。核心槽保存该层文本和可见性，身份包含表原始字节、显示模式、实际 loose SC 字节和配置字体。`font_path` 为空时沿用既有打包字体；配置时读取游戏目录内实际字体的首个 face、内部 family 和 Unicode coverage，交给同一个 Cosmic provider。缺失、损坏字体或缺字明确失败，不把系统同名字体当成替代文件。

`scr_archive`、`sys_archive` 可给出游戏目录内物理 PAZ 别名，密钥 role 与 `musica:/scr/`、`musica:/sys/` URI 不变。`script_directory` 选择同目录内标准 loose SC，脚本与 include 均先 loose、再当前输出根的 PAZ；既有逐脚本编码探测保留未替换的 Title。PNG 替换继续使用 key.toml 的 `texture_overrides`。这些路径不需要 Lua parser 或运行时解包器。

原生 `@stage` 使用与 `.stage` 相同的舞台语义，保留原始源字节；其他未知 `@` 命令不据此推断。选择对允许一个末尾分隔空格，显示与分支目标共用相同解析规则。背景层按原生 Opaque 复制 RGB，即使素材 A=0 也不改写像素；前景继续沿原混合语义。

影片字幕由 `ffmpeg-vcpkg` 启用真实 SDK/共享 libass 消费器。`subtitle_map` 为空时不建立轨道；非空必须指向游戏目录内 `{"version":1,"tracks":[{"clock":"movie","resource":"op.avi","ass":"subtitles/op.ass","fonts":["fonts/actual.ttf"]}]}`。只接受已有原生 movie 时钟，BGM 映射明确拒绝。ASS 为 UTF-8（可保留 BOM），字体按文件实际 family 使用，不因未使用样式或零时长事件猜补系统字体。映射、ASS 和字体字节均参与存档身份，所有资源合计有界。

字幕时间来自 `movie.position_us`，storage 来自实际解码视频，raster/偏移来自视频 viewport；颜色使用原生 swscale 实际转换的 matrix/range 和 BGRA8 的 SDR 呈现策略，不按分辨率补猜。显式 HDR、未支持转换仍报 `ASTRA_EMU_MUSICA_SUBTITLE_COLOR`。内容变化才上传 Alpha8 mask，位置变化不重传；mask revision 与轨道 serial 避免 ID 冲突，视频双槽 ID 保证单帧不同时释放/重传同一资源。暂停不推进时钟；读档、seek、换片、跳过和结束释放旧轨道。原生 handles 始终在 session 线程创建、使用和销毁。

Yorino 的可选日文 overlay 目标按原始显示路径保留 `\a`、`\N` 和 ruby 花括号等字面字符，不把目标再次交给 VM 控制解析；原始脚本控制与选择/文件目标保持不变。十五个未完全映射的名字使用点不靠猜测改解码或删表，完整有序名字记录照常保留。



## 私有配置与存档

`mount_musica(game_root, profile_path)` 只读取不超过 1 MiB 的 UTF-8 TOML，按原始文件字节 SHA-256 绑定私有配置身份。`MusicaKeyFile` 使用 `astra.emu.musica.keys.v2`，必须明确声明 `paz_version`、`index_size_xor`、`rc4_skip_crc` 和八个固定 role 的 `archive_keys`。`rc4_skip_crc` 控制 PAZ v2 的 entry CRC32 skip，缺失算法参数直接拒绝，不补默认值。配置包含私有解密信息，不进 Git、日志或报告；读取失败不写文件。

每个 `MusicaArchiveKey` 声明偶数长度的 `index_key_hex` 和 `data_key_hex`，接受大小写十六进制；mov 允许空 data key。可选字段保留各 role 的 `type_passwords`、`archive_xor` 和 256 字节 `video_key_hex`，`texture_overrides` 仍是整份配置的可选映射。`MusicaKeyFile::validate` 纯校验 schema、算法参数、密钥和路径语法；`load_musica_key_file` 有界读取普通文件并校验，不初始化游戏或设备。TOML 错误不回显源行、密钥或路径。

PAZ 探测只识别引擎，不能据此得出密钥已安装。Manager 只选择现有 key.toml，并将明确路径保存到当前游戏的 `profile_file`；JSON profile、GARbro 方案和旧 `keys.v1` TOML 的转换只由开发 CLI 提供。运行时没有导入器、兼容解析或格式回退。转换说明见 [CLI](../../Programs/astra-emu-musica-cli/README.md)；原游戏、密钥与存档保持不变。

同一 key.toml 可以可选地声明 `texture_overrides`。它是 `musica:/` 原资源 URI 到游戏目录内安全相对 PNG 路径的显式映射，例如 `musica:/bg/BG.png` 到 `hd/BG.png`。启动先验证原资源、帧数和映射路径，首次使用先解码原资源；逻辑尺寸与 ANI 原点始终来自原资源，替换图只提供物理像素。静态 PNG 与单帧 ANI 支持 PNG 替换；多帧 ANI、SQZ、缺失文件、非法路径、尺寸超限和解码失败都明确报错，不回退原图。原生和替换纹理共用 SDK `TextureCache`，通过不同 cache identity 区分。高清素材放在 ignored 私有目录，不进入仓库、日志或存档。

PAZ 的边界/重叠校验、分卷读取、Blowfish/RC4、XOR、解压和源文件变化校验保留。SDK 的 `ArchiveManifest` 是核心本地索引，不是 Host VFS 或产品 package。配置相对路径以游戏目录为基准，资源始终限于游戏目录内；公共读取错误使用 `ASTRA_EMU_PROFILE_*`。读取不运行 managed helper、BinaryFormatter 或 executable patch。

核心自有 slot 位于游戏目录 `.astra-musica/saves/slot-000.asav`。容器 `AMUSSV06` 包含长度、SHA-256 与 postcard snapshot，保存 VM、当前文本、翻译层文本/可见性、等待进度以及 sound resource/播放位置/volume/pan/repeat。它绑定同一 archive/profile/localization/font identity；加载重建脚本、场景和声音后提交。临时文件 flush 后原子替换；损坏、异版本或外部格式文件拒绝覆盖。此格式不宣称兼容原版存档。加载恢复声音的当前参数、位置与尚未完成的音量渐变。渐变保存分贝起止值、总采样数、已推进采样数和结束停止意图；按剩余采样继续，暂停不推进渐变。新音量命令替换旧渐变。旧 AMUSSV05 及更早内部格式明确拒绝，不覆盖旧 slot。

## eden 原版存档与检查点

`eden_save::EdenSave` 提供有界读取与容器编码，已补充严格检查点投影和 VM 恢复候选接口，Family 已接入严格子集的设备恢复和当前消息导出；已完成一个静态消息片段的原版读回与继续，范围见下文。`EdenEdition` 显式区分日文 CD-ROM 签名和本地 Steam English 签名：前者为 `;\n!`、`0xAA` 与 `eden 1.00`，后者为四个零字节与 `eden_en 1.00`。签名后依次为 NUL、有界注释、NUL、四字节 route 和单个 zlib 流。不会搜索压缩魔数来猜测偏移。

`EdenSaveEncoding` 由调用方显式选择 Shift-JIS、GBK 或 Windows-1252。解码须无替换字符，重新编码须还原原字节；写入不可表达字符直接失败。变量及 backlog 保留原字段顺序，包括多语言 `L1_0` 等字段，不补默认字段或丢弃未知字段。重复字段、结构注入、截尾、压缩校验失败、追加压缩流和大小超限均拒绝；容器和解压体各限 16 MiB。类型不实现 Debug，避免误打出正文。

格式研究参考 [ReMinori](https://github.com/luoyily/ReMinori) 的 `crates/formats/src/save.rs` 和恢复路径，固定参考 revision `79b00990324404bfbd496e7fa76c690cf040f744`，其许可证为 AGPL-3.0。本实现未引入该项目依赖或移植其代码。其原版 writer 和 port writer 是不同格式，README 的解析支持不作为原版互通结论；本地 Steam 汉化文件也不能按日文 CD-ROM 编码处理。

只读 CLI 检查要求显式指定版本和编码，不输出路径、资源名或正文：

```sh
cargo run --manifest-path Emulator/Cargo.toml -p astra-emu-musica-cli -- inspect-eden-save --file private-copy/eden0010.sav --edition english --encoding gbk
```

7 个本地原版存档副本通过严格解码及内存容器往返，覆盖 36–37 个变量、100–7590 条 backlog。七个槽的保存 PC 与消息 ID 已逐一匹配原始脚本，不进行附近搜索或位置截断。

`EdenCheckpoint` 将已知消息检查点字段转换为显式类型。`MusicaVm::from_eden_save` 读取实际挂载脚本后准备恢复候选；`restore_eden_save` 校验历史消息，再一次性替换 VM 状态。Fixture 已验证恢复不执行保存点之前的命令、继续剧情、再次存读档以及失败时保留活动 VM。当前严格投影只接受七个槽中的一个，含 897 条普通消息历史；选择记录、未知字段、旋转、滤色、活动效果和不可恢复时钟等状态仍明确拒绝。

实际挂载 PAZ 的恢复检查已完成该槽的 897 条历史及当前消息 VM。原版脚本名按 ASCII 大小写不敏感规则解析；名称碰撞明确拒绝。Family 支持只读 `eden_import_file`、显式 `eden_import_edition` 和既有 `script_encoding`；导入要求 direct 启动，恢复静态场景、消息和按原版加载边界重新开始的 BGM/语音。已有 GPU fixture 验证导入、物理输入推进、F5 保存、关闭和从该槽导出。真实商业片段的 Family 演出与原版独立副本读回尚未验收，其他游戏不声明原版互通。CLI `check-eden-restore` 只检查 VM 候选，不打开设备或写出存档。

## Musica 成果整合

`.select` 支持 1–4 个 `display:label` 对，原始字节保留以支持源码往返；每个目标标签在解析时校验。VM 持有选择等待和焦点，GPU 文字层显示选项，上下键循环切换，Enter/Space 确认当前项。鼠标悬停更新焦点，主键只确认命中项；命中区域与显示共用布局，行间空白和边界外点击不确认。F5/F9 保存和恢复焦点，显示文字从同一已验证脚本重建；损坏索引或不一致等待状态拒绝恢复。原生 Family GPU fixture 已覆盖选择、焦点变化、中途存读档和确认，鼠标回归覆盖悬停高亮、空白点击和对应分支；真实游戏界面验收尚未完成。

已吸收来源分支未提交修改中的 `.chain file.sc#label` 语法，合入同一个 Musica parser、VM 和原生 session。文件与标签分别校验，标签在目标脚本中解析成功后才切换脚本；缺标签保持原 VM 状态。省略标签仍从脚本起点执行，存档使用现有目标脚本身份和剧情位置，不新增存档格式。

`playbgm2` 使用独立 stream 5，`playse4` 使用 stream 6，分别沿用既有 BGM/SE 参数与播放路径；停止第二 BGM 不影响第一 BGM 和第四 SE。`deletevar` 删除同名局部及全局变量，不存在的名字按来源语义不改变状态。音频指令集中在 `runtime/audio_commands.rs`，两个新增通道共用混音、淡入淡出和存档结构。其余 Musica 执行、演出及系统功能仍待逐项整合，来源工作树保持原状。

`CrossFade` 与 `CrossFade2` 共用现有时序及 GPU 呈现。仅给出效果名称，或使用来源支持的单独 `*` 资源形式时，显式清除当前效果并重绘。原生 Family session 的 GPU fixture 已验证旧效果帧移除；时间推进和存档回归验证清除状态不会重新出现。该测试不替代真实游戏演出验收。

## 音频边界

快照或恢复请求超时会标记音频会话失败、取消 PCM sink 并停止处理后续命令；关闭仍须等待 worker 结束。恢复构建新的混音器后，在替换前再次检查取消，空恢复也不能绕过。阻塞 PCM 的实际超时回归与取消恢复回归已通过。

完整浮点解码已移入 SDK 的可选 `audio` feature，复用 Symphonia 与 Kira，保留帧预算和取消检查。核心继续持有混音、播放位置、淡入淡出及 worker 生命周期；公共解码错误使用 `ASTRA_EMU_AUDIO_*`。

Symphonia 在核心 audio worker 内完整解码，Kira 执行 resampling、混音、pan、loop 和 fade，输出固定 48 kHz stereo F32 PCM 到 Host 有界队列。每个资源输入最多 64 MiB，总 decoded frame budget 为 32 Mi frames（256 MiB），最多 64 个 stream；超限明确失败。当前采用完整资源解码，尚不是长媒体流式 decode。Snapshot/restore 走有界 worker 命令，close 先取消阻塞写入再 join，不使用 detached decoder thread。

## 验证与剩余边界

```bash
cargo test --manifest-path Emulator/Cargo.toml -p astra-emu-musica --all-features
cargo clippy --manifest-path Emulator/Cargo.toml -p astra-emu-musica --all-targets --all-features -- -D warnings
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-musica --features dynamic-plugin-export
```

测试覆盖 archive、图像、parser/VM、native codec 和 profile 边界。创建真实 session 的测试现在需要硬件 GPU；独立文字 GPU 测试显式使用 `--ignored` 运行，覆盖共享字形、移除区域、清空后重建与重复帧。公开最小字节格式 fixture 贯通 encrypted PAZ、SC、PNG、完整 PCM decode、Kira stereo pan、消息物理输入、atomic save/load 相同帧、translation cancel/load 和 close/reopen；单独验证阻塞 audio sink 退出、decode budget 与非挂起 VM 循环边界。Fixture audio sink 不是真实音频设备，动态模块编译也不代表 Manager 内实际安装验收。

当前整合状态和游戏验收以[实施计划](../../../../Docs/status/implementation-plan.md)为准。标题、剧情、选择、系统页、ANI/SQZ、影片及自有存档已有实现与局部回归；完整机制、真实设备音视频、结局、Android 和跨平台仍待验收，未支持命令继续明确失败。

VM 的可序列化状态集中在 runtime/model.rs，演出命令在 runtime/effects.rs，音频命令在 runtime/audio_commands.rs，选择处理在 runtime/choices.rs。调度与存读档保留在 runtime.rs，共享演出序号仍由 VM 分配。运行错误通过 typed diagnostic_code() 返回稳定标识；未实现命令只向 Family/Manager 返回指令序号，不透传脚本文本。

`export-eden-save` 从核心自有槽读取权威 VM 与音频快照，重建已知原版字段；只允许导入后仍可准确表示的普通消息检查点。选择、未知变量、动态演出、活动音频渐变等状态拒绝导出。输出必须为不存在的新文件，不覆盖原版槽。内部 VM 格式升级为 v26，保留具名历史状态而非未知原版字段；旧槽在外层 AMUSSV06 检查时拒绝，读取失败不覆盖文件。导出生成 SAV 与同名 128×80 PNG；缩略图来自该核心槽实际保存的 GPU 画面。两个目标都必须不存在，先发布 PNG、再发布 SAV；中途失败保留已产生的独立文件并报错，不覆盖既有文件。已在独立原版副本完成一个静态消息片段的导入、继续、保存、导出、读取继续；原版复存的剧情、变量、历史及媒体资源字段对齐。未验收可听音频与其他历史类型，不能据此声明全部存档兼容。

原版历史 `CT5` 保存面板切换参数，支持已经核对的 0 和 10；其他值明确拒绝。逻辑画布与 GPU 光栅尺寸分别配置，eden 原版导入要求 1024×640；核心槽保存逻辑画布身份，不同画布加载在修改活动状态前拒绝。

最终合成 RGB 的 Family 帧声明 Ignored alpha，保留原字节与存档/VM 几何，发布时不重复 Opaque alpha 扫描。Host 必要 GPU 归一化仍有成本；性能基线与 eden 旧逻辑尺寸整合缺口见[性能审计](../../../../Docs/status/performance-upstream-audit-2026-10-12.md#已验证范围)。元数据整改不代表演出、保存或几何迁移已完成。
