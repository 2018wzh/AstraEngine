# 旗舰 Demo 状态

标题：**玻璃雨中的信号 / Signal in the Glass Rain**

## 内容基线（历史制作批次）

| Key | Status | 当前含义 |
| --- | --- | --- |
| `content_creation` | `complete` | 180 条中英文对白与中文配音、共通线与三终局、79 张视觉文件、25 个原创 BGM/SE 源及其分发版本、UI 与 12 秒视频均已入包。 |
| `public_release_assets` | `ready_with_authorized_voice` | 用户已明确授权先前确定的 `Eve`/`Rex` OpenRouter 声线输出进入发行版；180 条配音均绑定 hash、master、distribution、模型、声线与请求文本 hash。 |
| `engine_integration` | `cook_ready_with_voice` | `.astra` story、`project.yaml`、UI/theme/controller、双语 localization、283 个 asset sidecar 和 package section已进入真实 Cook 主路径。 |
| `S3-FLAGSHIP-DEMO-01` | `IN_PROGRESS` | 该 gate 必须保持进行中，直到正式内容、引擎主路径和 Windows/Web E3 证据全部闭合。 |

## 证据边界

当前提交的证据包括完整中英文剧本和路线图、角色与场景素材、UI 视觉稿、可重建视频、原创 BGM/SE、自动媒体检测报告、内容 manifest、provenance、alt text、视觉/文本 review，以及真实 NativeVN Cook 输入。音频自动检查已覆盖响度、true peak、频谱活动、静音、削波和循环接缝；OpenRouter 使用区域内实测可用的 `xiaomi/mimo-v2.5`，以 `temperature=0`、固定 seed、`json_object` 对 25 项完成辅助听审并全部通过。模型报告不替代完整人工听审。Cook 证据不能证明以下任何一项：

- Runtime/StateMachine/provider 已运行；
- Player 已启动或 RuntimeWorld 已推进；
- Windows/Web 输入、画面变化、音频 meter、route coverage 或同 run identity 已通过；
- 正式运行发行 gate 已通过。

原始 24 kHz MP3 响应只保留在 ignored 私有目录。发行树提交经过统一响度与边界处理的 48 kHz/24-bit WAV master 和 48 kHz OGG distribution；授权依据、模型、声线、源 hash、请求文本 hash 和逐 cue 绑定记录在 `Manifests/voice-release.json`。

## 后续实施

继续用当前 Cook/package 与真实 Player 验证输入、视觉、音频、路线及存读档。历史内容批次的“停止在 Cook”不再是接续任务的停止条件；尚未执行的产品行为仍保持未验收。

## 2026-09-22 配置迁移

重跑旧配置实际失败于 `profile.migrate`，原因是 platform profile v2 已不受支持。现已改为 v3，拆分 mixer/output、补齐音频限额并声明四平台；修复 advanced-vn 系统页空 allowlist 和错误 profile 绑定。项目刷新不再生成/覆盖 `.astra`，保留故事、注释和稳定 ID。真实 Cook 与后续产品验证结果另行记录，不沿用历史 cook-ready 标记作为当前通过证明。

本批真实 `astra cook` 通过：284 个资产均经当前 Cook，生成两种 profile 的 typed VN package section；随后工具测试通过 23 项、跳过 1 项。没有重生成声音或图片。此记录只关闭配置/源码编译与素材 Cook，不关闭窗口、视听、路线或恢复。

## 连续分层演出改写（进行中）

默认入口改为雨夜相遇的连续片段，复用旧素材，保留稳定命令 ID。修复原故事每场重新声明同名 layer 的运行错误；bootstrap 只声明一次，后续场景更新内容。移除对白/选项页作为真实背景的 UI 视觉稿，改为实际布局面板，避免素材里画出的按钮与可操作按钮混淆。



## 2026-09-22 硬件离屏运行与待合并改动

main surface 修复后真实 Cook/package 与 Vulkan 硬件离屏场景通过。完整 12 秒 VP9 视频分支通过 52 条物理输入、7 个 checkpoint，实际查看首段、中段和结束画面；输出 WAV 为 48kHz 双声道、22.176 秒、非静音，未验证真实设备听音。保存/读取后额外按 Escape 返回剧情的诊断流程通过；严格“读取后自动回剧情”在本批基线仍失败，等待接入主线修复，不能用诊断流程替代该回归。

本批调整 shade、复用人物 entity、保存页文案与缩略图尺寸；进一步将 replacement 延长为 8 秒以观察中途取消，并给保存列表加 viewport 裁剪。这两项最新调整已 Cook/package，尚待 GPU 看图；工具测试另行执行。没有生成新媒体，也没有修改历史审查结论。错误退出仍可能触发 GPU 清理竞争；缩放输出与缩略图捕获入口差异已报告共享 Player owner。TaskGroup 新基线、冷启动恢复、原生窗口/音频、完整路线与性能未验收。


2026-09-22 合并 a8dd4625c 后，NativeVN 新包在 Vulkan 硬件离屏路径通过严格存读档：69 条物理输入、10 个 checkpoint、606 submitted/12 rasterized frames，diagnostics 为空，正常关闭。读取后直接回剧情，无额外 Escape；已查看恢复画面及 shade/取消恢复画面。保存列表的越界绘制仍在：clip_children 对应的 clip_rect_points 在 Yakui→Mesh2D 转换中未使用，截图仍显示槽位覆盖返回按钮；键盘入口缩略图仍捕获保存页。这两项不计通过，已报告共享 UI/Player owner。此轮不代表冷进程恢复、真实音频或完整产品验收。


2026-09-22 接入 392ceec05 后修复 Headless 错误退出与保存缩略图入口：CLI run/serve 持有取消/join owner，最小硬件 GPU await-timeout 负例现在以预期 exit 2 退出，不再挂住；正常 69 输入/10 checkpoint 存读档 GPU 回归 exit 0。物理输入经 prepare_ui_input 按实际系统页转换在呈现前截图，三个宿主接收 Captured 结果；实际查看 slot.01，缩略图已显示保存前的剧情画面。126 项直接调用方测试通过、1 helper 忽略，线程析构 2 项局部测试与相关五 crate all-target Clippy 通过。随后使用另一进程读取同一存档，25 条物理输入、目录与恢复 checkpoint 均通过，已查看恢复画面；存档读取前后 hash 相同。列表 clip 丢失仍未修复，真实窗口/音频与多平台仍未验收。
