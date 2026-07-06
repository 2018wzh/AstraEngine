# Web Platform

Web 目标是 wasm32 + WebGPU/WebGL fallback + WebCodecs。Web 平台必须遵守浏览器 sandbox，不提供任意本地文件系统或后台线程假设。

对应 crate 是 `astra-platform-web`。没有 wasm/browser SDK 时只输出缺失 SDK 的 capability report，不把 Web release 标为完成。

## Current Status

| Work ID | Status | Scope |
| --- | --- | --- |
| `S2-WEB-HOST-01` | `SPEC_READY` | 计划补 wasm host、WebGPU/WebGL、WebCodecs、WebAudio unlock、OPFS/IndexedDB/File API/HTTP range package source、worker/visibility resume 和 browser smoke |

## Boundaries

- Package 通过 File API、Origin Private File System 或 HTTP range source 加载。
- DecodeProvider 优先 WebCodecs；不可用时使用受限软件 fallback。
- Audio 使用 WebAudio provider。
- Runtime deterministic tick 不依赖 requestAnimationFrame 时间抖动。

## Testing

Web gate 覆盖 browser launch、WASM package load、input scenario、save persistence、audio unlock、decode capability report 和 provider-free replay。

当前阶段只登记 gate 目标，不实现 wasm launcher、browser smoke、WebCodecs provider 或浏览器 storage provider。Web 不能标为 `DONE`，直到 browser smoke 和 WebCodecs/WebAudio/storage evidence 都进入 release report。

## Capability

Web capability report 必须写明 WebGPU/WebGL profile、WebCodecs availability、OPFS/IndexedDB persistence、File API/HTTP range package source、audio unlock state、worker support 和 network permission。字段以 [Platform Host Blueprint](../implementation/platform-host.md) 为准。
