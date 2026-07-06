# Web Platform

Web 目标是 wasm32 + WebGPU/WebGL fallback + WebCodecs。Web 平台必须遵守浏览器 sandbox，不提供任意本地文件系统或后台线程假设。

对应 crate 是 `astra-platform-web`。native host 下始终输出缺失 SDK；只有 wasm32 浏览器环境能报告 `sdk_status: present`。同步 `browser_smoke_probe` 不冒充完整 evidence；`browser_smoke_probe_async` 在真实浏览器上下文执行 renderer、media、WebCodecs、WebAudio、storage 和 package source probe。native host 不能用环境变量伪造 browser evidence。

## Current Status

| Work ID | Status | Scope |
| --- | --- | --- |
| `S2-WEB-HOST-01` | `DONE` | Web probe 覆盖 wasm/browser、`renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw`、`package.web_source_read`、input、worker/visibility 和 browser smoke |

## Boundaries

- Package 通过 File API、Origin Private File System 或 HTTP range source 加载。
- DecodeProvider 优先 WebCodecs；不可用时使用受限软件 fallback。
- Audio 使用 WebAudio provider。
- Runtime deterministic tick 不依赖 requestAnimationFrame 时间抖动。

## Testing

Web gate 覆盖 browser launch、WASM package load、input scenario、save persistence、audio unlock、decode capability report 和 provider-free replay。

当前阶段已实现 `astra-platform-web` async browser smoke 和 wasm-only `WebCodecsDecodeProvider`。真实浏览器必须完成 renderer context、public MP4/WebM/MP3 browser media load、WebCodecs config support、OfflineAudioContext render、IndexedDB write/read/delete 和 Blob/File/fetch package source hash。缺任一 required evidence 时，`platform.capability_report` 会阻断 Web release。

```bash
cargo test -p astra-platform-web --target wasm32-unknown-unknown --no-run
wasm-pack test --headless --chrome Engine/Source/Platform/astra-platform-web
```

第一条命令只验证 wasm test 编译；第二条命令必须在真实 browser runner 中产出 `browser_smoke` 等 required evidence 后，才能把 Web release report 判为 pass。

## Capability

Web capability report 必须写明 WebGPU/WebGL profile、WebCodecs availability、OPFS/IndexedDB persistence、File API/fetch package source、audio state、worker support 和 network permission。Required smoke 是 `browser_smoke`、`renderer.browser_context`、`decode.browser_media`、`decode.webcodecs_config`、`audio.webaudio_render`、`save.web_storage_rw` 和 `package.web_source_read`。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。
