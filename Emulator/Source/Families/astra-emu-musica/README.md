# Musica core

Musica 提供独立 `FamilyProvider`、`FamilySession` 和可选 `abi_stable` 动态模块。核心自己打开 PAZ、执行 SC VM、混音并维护存档。[astra-emu-sdk](../../SDK/astra-emu-sdk/README.md) 复用 `astra-text` 提供真实字体 shaping/排版/字形资源管理，并管理有界纹理缓存。场景和文字直接由 AstraEngine 的 `WgpuOffscreenRenderer` 在硬件 GPU 上合成，再回读最终帧交给 Family API；不创建 Engine World、package、registry、PlatformHost 或 Headless server。GPU 初始化失败明确报错，不退回 CPU 渲染。

动态插件通过共享诊断桥将核心 tracing/log 接入 Manager；静态 provider 使用宿主订阅器。日志不另建文件 sink，正文和未审查 Debug 内容不会跨 ABI 透传。

## 启动和输入

`create_musica_provider()` 返回静态 provider；`dynamic-plugin-export` feature 导出独立 FamilyModule。descriptor 使用 Family API v7，接受 `profile_file`（默认 `musica.profile.json`）和 `entry_script`（默认 `test.sc`）两个 typed 启动配置。前者限游戏目录内安全相对路径；后者是 scr archive 内单个 `.sc` 文件名。`test.sc` 延续此前明确优先选择的观察入口，可按游戏修改。不会在入口失败后猜测另一个脚本。

已接入的渲染路径保留既有 1280×720 坐标与 Noto Sans JP 字体绑定。Host resize 只改变窗口，不改游戏内部布局。Session 按 60 Hz 逻辑处理有界 elapsed interval，显示纹理、消息、面板和已验证的 CrossFade2；字形结果与纹理有界缓存。Enter、Space 和鼠标主键推进消息；F5 保存，F9 读取。关闭和销毁都会取消文字请求、取消 PCM 写入并等待 audio worker 结束；同进程不能同时打开两个 Musica session。

正文替换是可选服务；禁用时直接显示原文。启用时只暂停当前消息推进，效果和音频继续。请求具有 session/generation identity，加载和关闭取消旧请求。失败、15 秒 timeout、错误 response 或无法排版的替换保留原文，不自动重试。正文和错误 payload 不写日志。

## 私有配置与存档

`mount_musica(game_root, profile_path)` 加载游戏目录内不超过 1 MiB 的 JSON。`MusicaProfile` 使用 `astra.emu.musica.profile.v1`，包含 `paz_version`、`index_size_xor` 和八个固定 role 的密钥设置。配置包含私有解密信息，不进 Git、日志或报告；读取失败不写文件。旧 Luau patch/YAML 不迁移，使用纯 Rust GARbro importer 重新导入 JSON。

同一 profile 可以可选地声明 `texture_overrides`。它是 `musica:/` 原资源 URI 到游戏目录内安全相对 PNG 路径的显式映射，例如 `musica:/bg/BG.png` 到 `hd/BG.png`。启动先验证原资源、帧数和映射路径，首次使用先解码原资源；逻辑尺寸与 ANI 原点始终来自原资源，替换图只提供物理像素。静态 PNG 与单帧 ANI 支持 PNG 替换；多帧 ANI、SQZ、缺失文件、非法路径、尺寸超限和解码失败都明确报错，不回退原图。原生和替换纹理共用 SDK `TextureCache`，通过不同 cache identity 区分。高清素材放在 ignored 私有目录，不进入仓库、日志或存档。

PAZ 的边界/重叠校验、分卷读取、Blowfish/RC4、XOR、解压和源文件变化校验保留。SDK 的 `ArchiveManifest` 是核心本地索引，不是 Host VFS 或产品 package。私有配置有界读取与明文缓存也由 SDK 共用；配置相对路径以游戏目录为基准，公共读取错误使用 `ASTRA_EMU_PROFILE_*`。导入/读取不运行 managed helper、BinaryFormatter 或 executable patch。

核心自有 slot 位于游戏目录 `.astra-musica/saves/slot-000.asav`。容器 `AMUSSV04` 包含长度、SHA-256 与 postcard snapshot，保存 VM、当前文本、等待进度以及 sound resource/播放位置/volume/pan/repeat。它绑定同一 archive/profile identity；加载重建脚本、场景和声音后提交。临时文件 flush 后原子替换；损坏、异版本或外部格式文件拒绝覆盖。此格式不宣称兼容原版存档。加载恢复声音的当前参数、位置与尚未完成的音量渐变。渐变保存分贝起止值、总采样数、已推进采样数和结束停止意图；按剩余采样继续，暂停不推进渐变。新音量命令替换旧渐变。旧 AMUSSV03 及更早内部格式明确拒绝，不覆盖旧 slot。

## eden 原版存档与检查点

`eden_save::EdenSave` 提供有界读取与容器编码，已补充严格检查点投影和 VM 恢复候选接口，Family 已接入严格子集的设备恢复和当前消息导出；原版读回仍待验收。`EdenEdition` 显式区分日文 CD-ROM 签名和本地 Steam English 签名：前者为 `;\n!`、`0xAA` 与 `eden 1.00`，后者为四个零字节与 `eden_en 1.00`。签名后依次为 NUL、有界注释、NUL、四字节 route 和单个 zlib 流。不会搜索压缩魔数来猜测偏移。

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

现有未验证 stand positioning、movie、choice、系统页相关 opcode 仍明确失败，不能把这一可运行子集说成完整 Musica 兼容。ANI/SQZ decoder 有独立真实格式 fixture，尚未接入 session 动画播放。尚无授权完整游戏、真实设备音视频、原版 save compatibility、冷启动/完整结局、Android 静态注册或跨平台验收。

VM 的可序列化状态集中在 runtime/model.rs，演出命令在 runtime/effects.rs，音频命令在 runtime/audio_commands.rs，选择处理在 runtime/choices.rs。调度与存读档保留在 runtime.rs，共享演出序号仍由 VM 分配。运行错误通过 typed diagnostic_code() 返回稳定标识；未实现命令只向 Family/Manager 返回指令序号，不透传脚本文本。

`export-eden-save` 从核心自有槽读取权威 VM 与音频快照，重建已知原版字段；只允许导入后仍可准确表示的普通消息检查点。选择、未知变量、动态演出、活动音频渐变等状态拒绝导出。输出必须为不存在的新文件，不覆盖原版槽。内部 VM 格式升级为 v25，保留具名历史状态而非未知原版字段；旧槽在外层 AMUSSV04 检查时拒绝，读取失败不覆盖文件。当前导出仅生成 SAV，原版缩略图和原版加载后的媒体语义仍须实际核对。
