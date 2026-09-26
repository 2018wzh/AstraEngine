# NativeVN Flagship 内容工具

这里是《玻璃雨中的信号》内容、项目生成与校验工具。实际 Cook 由仓库的 `astra-cli` 完成。

主要入口：

- `generate_audio.py`：确定性生成 4 首 BGM、3 个 stinger 和 18 个 SE；
- `generate_video.py`：从分层 PNG 重建 12 秒雨幕信号循环的 MP4/WebM；
- `generate_voice.py`：从环境变量读取 ElevenLabs 凭据和两名角色的 Voice Design id，批量生成 ignored 私有候选与脱敏公开 metadata；禁止 voice cloning；
- `generate_release_voice_openrouter.py`：从 `OPENROUTER_API_KEY` 读取凭据，以固定模型和声线生成 180 条发行配音，保留 ignored 原始响应，并输出 48 kHz WAV master、OGG distribution 与逐 cue release manifest；
- `update_manifest.py`：更新音频 hash、byte size、格式与检测结果；
- `analyze_audio.py`：用 ffmpeg 测量响度、true peak、频谱活动、静音、削波和循环接缝，并单独保留人工听审状态；
- `review_audio_openrouter.py`：用 OpenRouter 音频模型逐项完成结构化辅助听审；API key 只从环境变量读取，不能替代人工签核；
- `build_nativevn_project.py`：刷新 runtime localization、统一 WAV/MP4 的 asset sidecar、JP/SC 字体 metadata 与当前四平台 `project.yaml`；只检查 `.astra`、UI/theme/controller 是否存在，不重写作者源码；
- `experience_inputs.py`：生成连续分层演出的物理输入 JSONL，供真实 GPU Headless 产品路径运行；
- `update_content_metadata.py`：更新内容 manifest、provenance、review、prompt 和 alt text 索引；
- `build_contact_sheet.py`：把人工视觉审查表写入 ignored `.local/review/`；
- `validate_content_pack.py`：校验双语引用、路线、媒体、透明通道、hash、授权状态和公开树安全。

内容门禁：

```bash
python Tools/NativeVN/validate_content_pack.py
python -m unittest discover Tools/NativeVN/tests
```

当前发行项目包含用户明确授权的 OpenRouter 配音，逐 cue release eligibility 由 `Manifests/voice-release.json` 管理；`--release` 仍要求完整人工音频听审。旧 ElevenLabs 入口只生成 ignored 私有候选：API key 只从 `ELEVENLABS_API_KEY` 读取；两名角色的 Voice Design id 分别从 `ELEVENLABS_LIN_YAO_VOICE_ID` 和 `ELEVENLABS_ZHOU_HENG_VOICE_ID` 读取。API key 不得写入参数文件、日志、manifest 或仓库。

OpenRouter 辅助听审只从 `OPENROUTER_API_KEY` 读取凭据。当前批准设置是 `xiaomi/mimo-v2.5`、`temperature=0`、固定 seed、`json_object` 和 128 kbps 临时 MP3；该模型是在当前区域对 audio input 做 capability preflight 后选定，`openrouter/auto` 及更高档音频端点因能力或区域限制没有进入正式批次。

`experience_inputs.py` 默认生成阅读、位移、shade 与取消/恢复 checkpoint。`--media` 继续选择影片并等待完整播放结束；`--save-restore` 通过键盘实际写槽和读取，严格要求读取后回剧情。工具不使用直接修改状态的语义快捷命令。合并 a8dd4625c 后严格存读档流程已通过硬件离屏运行；读取后无需额外 Escape。缩略图已通过真实 GPU 看图确认显示剧情；保存页裁剪仍待共享 UI 修复。

`--cold-restore` 是独立进程的读取流程，不能与 `--save-restore` 或 `--media` 合并。先运行 `--save-restore` 输入并关闭进程，再用同一构建和包启动冷恢复输入。仅复用这一轮新建的私有存档：可以沿用保存目录，也可以将其 Saved 子目录原样复制到另一 ignored 运行目录，并比较运行前后的存档 hash；不能读取或覆盖用户原存档。冷启动通过实际加载页读取已有槽，不写槽。同进程和冷启动输入都在读档后继续推进 120 个逻辑 tick，截取恢复时与继续演出后的画面，之后正常关闭。

普通输入新增 `experience.reading.revealing`，在实际文本尚未揭示完成时截图；原有阅读完成、分层位移、属性替换、取消和等待结束片段仍由物理输入推进。2026-09-26 的修复包已完成 DX12 discrete GPU 离屏物理输入回归，并实际查看部分正文、全文及恢复后演出画面；终之空模板仅通过软件回归。生成 JSONL 本身不代表流程通过，画面变化也不能单独替代输入消费和剧情状态检查。
