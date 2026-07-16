# 旗舰 Demo 状态

标题：**玻璃雨中的信号 / Signal in the Glass Rain**

## 固定状态

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

## 下一步与停止条件

本轮停止在 Cook 成功，不执行 scenario、Player 或 Runtime 测试。后续必须另行取得 Windows/Web native-input、视觉、音频、route、save/load 和同 run identity evidence；在这些证据闭合前，`S3-FLAGSHIP-DEMO-01` 保持 `IN_PROGRESS`。
