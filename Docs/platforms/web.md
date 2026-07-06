# Web Platform

Web 目标是 wasm32 + WebGPU/WebGL fallback + WebCodecs。Web 平台必须遵守浏览器 sandbox，不提供任意本地文件系统或后台线程假设。

对应 crate 是 `astra-platform-web`。native host 下始终输出缺失 SDK；只有 wasm32 浏览器环境能报告 `sdk_status: present` 并附带 browser smoke。

## Current Status

| Work ID | Status | Scope |
| --- | --- | --- |
| `S2-WEB-HOST-01` | `DONE` | Web probe 覆盖 wasm/browser、WebGPU/WebGL、WebCodecs、WebAudio、OPFS/IndexedDB、File API/fetch package source、input、worker/visibility 和 browser smoke |

## Boundaries

- Package 通过 File API、Origin Private File System 或 HTTP range source 加载。
- DecodeProvider 优先 WebCodecs；不可用时使用受限软件 fallback。
- Audio 使用 WebAudio provider。
- Runtime deterministic tick 不依赖 requestAnimationFrame 时间抖动。

## Testing

Web gate 覆盖 browser launch、WASM package load、input scenario、save persistence、audio unlock、decode capability report 和 provider-free replay。

当前阶段已实现 `astra-platform-web` browser smoke 和 wasm-only `WebCodecsDecodeProvider`。如果真实浏览器缺 WebCodecs、WebAudio、WebGPU/WebGL fallback、Web storage 或 package source API，`platform.capability_report` 仍会阻断 Web release。

## Capability

Web capability report 必须写明 WebGPU/WebGL profile、WebCodecs availability、OPFS/IndexedDB persistence、File API/fetch package source、audio unlock state、worker support 和 network permission。Required smoke 是 `browser_smoke`、`renderer.webgpu_or_webgl`、`decode.webcodecs`、`audio.webaudio_unlock`、`save.web_storage` 和 `package.web_source`。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。
