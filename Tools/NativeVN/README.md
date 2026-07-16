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
- `build_nativevn_project.py`：从 canonical screenplay 和素材生成 `.astra`、runtime localization、UI/theme/controller、asset sidecar 与 `project.yaml`；
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
