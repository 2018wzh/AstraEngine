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
