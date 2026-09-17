# Musica Family 插件

Musica 实现当前 Family API，自持 PAZ、脚本 VM、媒体、GPU Scene 和原生存档。Manager 负责安装、配置、输入、最终帧及 PCM 输出，不创建 Engine/VN session。配置在启动时通过 descriptor 校验，诊断经共享日志桥进入 Manager。

## 构建与安装

基础插件不要求 FFmpeg。需要电影播放时显式选择原生依赖：

```sh
cargo build --manifest-path Emulator/Cargo.toml -p astra-emu-musica --release --features dynamic-plugin-export,ffmpeg-vcpkg
```

FFmpeg 使用当前工程的 vcpkg 配置；运行时必须能找到匹配的动态库。插件通过 Manager 本地安装入口装入，更新后重启 Manager。无 FFmpeg 构建遇到 movie 指令返回 `ASTRA_EMU_MUSICA_MOVIE_UNAVAILABLE`，不会把首帧或静音当作播放成功。

Manager 的 Audio 分组提供 `bgm_volume`、`voice_volume`、`se_volume`（0–100）及对应 `*_muted`。这些选项启动前校验并由 Manager 持久化，不写入剧情存档；读档保留当前用户音量。电影 PCM 暂不归入这三个剧情音轨。

## 会话与恢复

`probe/open/advance/visit_frame/close` 使用当前 Family 契约。核心每次运行到消息、选择、等待、错误或终点；场景使用共享 GPU renderer，文字使用 AstraText。音频 worker 混合 BGM、SE、voice 和电影 PCM，经同一有界可取消 Host 队列输出。

原生保存包含 VM、场景参数、音频位置和显式电影游标，不包含解码器、GPU 资源或脚本栈。F5/F9 快捷保存与恢复；读档重建资源并丢弃旧请求。关闭取消本会话的解码请求与 PCM 写入，并等待 worker 释放资源，动态库仍由 Manager 驻留。

`progress_in_background` 默认关闭：失焦暂停剧情、演出、电影与音频，启用后允许失焦继续。初始窗口焦点同样生效；显式窗口挂起始终暂停，重新聚焦不能越过挂起状态。失焦清除快进键及待消费输入，恢复不补跑暂停时长，读档保留当前窗口状态。

电影沿用来源全屏舞台约束；窗口挂起暂停电影，Control 仅按 movie 自身标记跳过。电影播放期间暂停剧情和普通演出时钟，结束后解除 Media wait 并继续脚本。

格式、key、资源、opcode、配置及解码错误明确返回，不更换算法或吞掉失败。已有存档读取失败不覆盖原文件。

详细命令见 [脚本执行](script-execution.md)，共享绘制与媒体见 [呈现与媒体](presentation-and-media.md)，实际完成范围见 [实施状态](../../status/implementation-plan.md)。完整游戏结局与平台验收仍开放。

`text_shadow` 默认开启，按来源实现为正文、说话人和 backlog 添加黑色圆形描边，选择项保持原样。关闭后仍使用同一 GPU 文字渲染路径；存档不保存此显示偏好，读档继续使用当前 Manager 设置。

Manager 的 `script_encoding` 显式选择 `shift_jis`（默认）或 `gbk`。来源的 `[j]` / `[e]` 条件行分别跟随日文／GBK 选择；正文、说话人、选择项和命令操作数共用严格解码。GBK 使用仓库已有 Noto Sans SC，日文使用 Noto Sans JP。每个脚本按来源规则检测编码，读档重查文件并校验保存的编码；未知编码和损坏字节失败，不替换字符。PAZ 索引与 key 材料仍沿用原生 CP932。两种编码都能严格解码时采用启动配置，否则选择错误行更少的一种，再由解析器拒绝剩余错误。检测不使用替换字符或系统代码页，诊断记录选择结果；启动首选不会被上一个脚本的编码覆盖。日文与中文字体在同一文字资源管理器中注册，切换与读档共用资源生命周期。

剧情等待时按 F6 打开原生保存页，F8 打开读取页。左右键翻页，上下键移动槽位，Enter/Space 确认，Escape 返回；也可点击原生槽位及底部按钮。每页 10 槽，共 100 槽，默认打开槽 20 所在页。空槽读取保持当前页面；读取失败不覆盖文件。F5 使用来源的槽 10–19 轮转，F9 读取最近成功的快捷存档；槽 0 不再作为快捷槽。轮转游标跨进程保存，同一脚本行的重复快捷保存不写文件、不推进游标；手动槽位不受轮转影响。

存档卡片保存本地时间、注释字段和 96×54 PNG 缩略图。页面使用原生 sys 素材及共享 GPU 文字；缩略图取打开页面前的剧情画面。新容器标识为 `AMUSSV03`，旧格式明确拒绝且不覆盖，不提供迁移层。手动存档恢复到剧情等待，不恢复存读档页。

四条路线的画廊解锁随成功 advance 写入 `.astra-musica/saves/global-progress.json`，未变化时不写文件。沿用来源白名单，只接受值 1；文件绑定当前游戏身份并原子替换。读旧存档会合并已取得的解锁，普通剧情变量仍由槽位恢复。VM 格式升级为 v22，旧 v21 直接拒绝；画廊页面见下文。

显式 `launch_mode=title` 可进入来源的原生标题图片与菜单，支持鼠标/键盘焦点、开始游戏、读档、Config、退出和剧情 `.end` 返回标题。标题变体取四条路线全局标记；开始游戏重新装载入口脚本、取消旧翻译、清理音频并保留当前配置、已读集合与全局变量。标题读档取消返回标题，成功读档保留 Title 启动方式。默认 `direct` 继续直接进入剧情。VM v24 保存启动方式与鉴赏页状态，旧 v23 及更早明确拒绝；完整标题模式仍待真实游戏验收。

Memories 沿用来源的 CG、Replay、BGM、Movie、Return 顺序，仅最终标题变体开放。CG 使用原生分页素材与缩略图；BGM 保留 47 首曲目、键盘分页及原生鼠标区域，复用会话混音器。回想与电影通过来源脚本表加载，结束返回标题，随后开始仍加载原始主剧情入口。页面使用共享 GPU Scene，停留期间不推进剧情；音乐鉴赏按来源规则处理停止和返回。真实游戏完整鉴赏流程仍待完成。

Config 使用来源的原生素材、控件坐标和共享 GPU Scene。Title 模式可从标题进入，剧情等待时按 F7 打开；鼠标调整参数，Enter/Space 应用，Escape 取消。BGM、语音、音效试听复用同一混音器及音量总线，关闭页面停止试听并恢复已应用的音量。动画与屏幕效果开关控制各自时钟，关闭动画不会阻断滚动等待，等待完成仍呈现最终位置。

`MusicaConfigState` 和编辑草稿由会话持有，设置及草稿不进入剧情存档。Config 打开期间拒绝保存；成功读档清除草稿并保留当前设置，失败则保留草稿。应用时原子写入 `.astra-musica/saves/configuration.json`，绑定当前游戏身份，限制为 8192 字节，损坏、未知字段或其他游戏的数据明确拒绝且不覆盖。Manager 中的偏好参数仅作为未建立原生配置时的初始值；原生配置存在后优先使用它，避免 Manager 补齐的默认值在冷启动时重置设置。

全屏切换尚缺 Family 窗口命令通道，应用该变更会返回 `ASTRA_EMU_MUSICA_WINDOW_COMMAND_UNAVAILABLE`，不写入配置文件。字体选择与正文速度字段保留来源当前语义，不宣称新增字体或逐字渲染能力。真实游戏 Config、窗口切换与完整结局继续验收。
